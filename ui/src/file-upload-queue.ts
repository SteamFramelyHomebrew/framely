import {uploadFile} from './upload';
import type {UploadEntry} from './file-upload-entries';
export type UploadState='waiting'|'uploading'|'done'|'skipped'|'failed'|'cancelled';
export type UploadTask=UploadEntry&{id:string;state:UploadState;received:number;size:number;savedPath?:string;ticket?:string;controller?:AbortController};
export type UploadBatch={id:string;directory:string;rootIdentity:{dev:number;ino:number};policy:'keep'|'skip'|'overwrite';tasks:UploadTask[];totalFiles:number;clearedUploadedFiles:number};
export const uploadBatchFileCounts=(batch:UploadBatch)=>({uploaded:batch.clearedUploadedFiles+batch.tasks.filter(t=>!t.directory&&t.state==='done').length,total:batch.totalFiles});
export type QueueTransport={api:(params:unknown,signal?:AbortSignal)=>Promise<any>;chunk:(id:string,offset:number,data:Blob,signal:AbortSignal)=>Promise<void>;changed?:()=>void};
export class ChunkSlots{
 private active=0;
 private waiting:{signal:AbortSignal;resolve:(release:()=>void)=>void;reject:(reason:unknown)=>void;abort:()=>void}[]=[];
 constructor(private limit=8){}
 acquire(signal:AbortSignal):Promise<()=>void>{signal.throwIfAborted();return new Promise((resolve,reject)=>{const item={signal,resolve,reject,abort:()=>{this.waiting=this.waiting.filter(v=>v!==item);reject(signal.reason);}};signal.addEventListener('abort',item.abort,{once:true});this.waiting.push(item);this.drain();});}
 private drain(){while(this.active<this.limit&&this.waiting.length){const item=this.waiting.shift()!;item.signal.removeEventListener('abort',item.abort);if(item.signal.aborted){item.reject(item.signal.reason);continue;}this.active++;let released=false;item.resolve(()=>{if(released)return;released=true;this.active--;this.drain();});}}
}
export class FileUploadQueue{
 batches:UploadBatch[]=[];
 private directories=new Map<string,Map<string,UploadTask>>();private waiting=new Map<string,Set<UploadTask>>();
 private limit=3;private active=0;private serial=0;private closed=false;private slots=new ChunkSlots();private listeners=new Set<()=>void>();
 constructor(private transport:QueueTransport){}
 subscribe(listener:()=>void){this.listeners.add(listener);return()=>{this.listeners.delete(listener);};}
 private emit(){for(const listener of this.listeners)listener();}
 setLimit(limit:number){if(!Number.isInteger(limit)||limit<1||limit>8)throw new Error('Invalid upload concurrency');this.limit=limit;this.schedule();}
 add(directory:string,rootIdentity:UploadBatch['rootIdentity'],policy:UploadBatch['policy'],entries:UploadEntry[]){
  if(this.closed)throw new Error('Upload queue is closed');
  const batch:UploadBatch={id:String(++this.serial),directory,rootIdentity,policy,totalFiles:entries.filter(e=>!e.directory).length,clearedUploadedFiles:0,tasks:entries.map(e=>({...e,id:String(++this.serial),state:e.error?'failed':'waiting',received:0,size:e.file?.size??0}))};
  this.directories.set(batch.id,new Map(batch.tasks.filter(t=>t.directory).map(t=>[t.name,t])));this.waiting.set(batch.id,new Set(batch.tasks.filter(t=>t.state==='waiting')));
  this.batches.push(batch);this.emit();this.schedule();return batch;
 }
 private dependency(batch:UploadBatch,task:UploadTask){const parts=task.name.split('/'),parents:UploadTask[]=[];for(let i=1;i<parts.length;i++){const parent=this.directories.get(batch.id)?.get(parts.slice(0,i).join('/'));if(parent)parents.push(parent);}return parents;}
 private schedule(){if(this.closed)return;
  for(const batch of this.batches)for(const task of this.waiting.get(batch.id)??[]){
   if(this.active>=this.limit)return;
   if(task.state!=='waiting')continue;
   const parents=this.dependency(batch,task);
   if(parents.some(p=>p.state==='failed'||p.state==='cancelled')){task.state='failed';this.waiting.get(batch.id)?.delete(task);task.error='An upload parent directory failed';this.emit();continue;}
   if(this.active>=this.limit||parents.some(p=>p.state!=='done'))continue;
   this.waiting.get(batch.id)?.delete(task);task.state='uploading';task.controller=new AbortController();this.active++;this.emit();
   void this.run(batch,task).finally(()=>{this.active--;task.controller=undefined;task.ticket=undefined;this.emit();this.schedule();});
  }
 }
 private async run(batch:UploadBatch,task:UploadTask){
  const signal=task.controller!.signal;let id='';const common={directory:batch.directory,rootIdentity:batch.rootIdentity,name:task.name};
  try{
   const start=await this.transport.api({...common,operation:task.directory?'upload.directory':'upload.start',size:task.size,conflict:batch.policy},signal);
   id=start.id??'';task.ticket=id;
   signal.throwIfAborted();
   if(task.directory||start.status==='skipped'){task.state=task.directory?'done':'skipped';task.savedPath=start.path;return;}
   if(!id)throw new Error('Missing upload ticket');
   await uploadFile(task.file!,signal,(_percent,received)=>{task.received=received;this.emit();},{
    start:async()=>({upload:id,chunkSize:start.chunkSize}),
    chunk:async(ticket,offset,data,chunkSignal)=>{const release=await this.slots.acquire(chunkSignal);try{await this.transport.chunk(ticket,offset,data,chunkSignal);}finally{release();}},
    abort:ticket=>this.transport.api({operation:'upload.cancel',id:ticket})
   },{concurrency:2});
   signal.throwIfAborted();
   // Once committed, report the server result even if cancel races the reply.
   const result=await this.transport.api({operation:'upload.finish',id,approve:batch.policy==='overwrite'});
   id='';task.state=result.status??'done';task.savedPath=result.path;
  }catch(error){task.state=signal.aborted?'cancelled':'failed';task.error=signal.aborted?undefined:String(error);}
  finally{if(id)await this.transport.api({operation:'upload.cancel',id}).catch(()=>{});this.transport.changed?.();}
 }
 cancel(taskId?:string,batchId?:string){for(const batch of this.batches){if(batchId&&batch.id!==batchId)continue;for(const task of batch.tasks){if(taskId&&task.id!==taskId)continue;if(task.state==='waiting'){task.state='cancelled';this.waiting.get(batch.id)?.delete(task);}else if(task.state==='uploading')task.controller?.abort();}}this.emit();this.schedule();}
 retry(taskId?:string,batchId?:string){for(const batch of this.batches){if(batchId&&batch.id!==batchId)continue;for(const task of batch.tasks)if((!taskId||task.id===taskId)&&task.state==='failed'){task.state='waiting';this.waiting.get(batch.id)?.add(task);task.received=0;task.error=undefined;}}this.emit();this.schedule();}
 clear(){this.batches=this.batches.filter(b=>{b.clearedUploadedFiles+=b.tasks.filter(t=>!t.directory&&t.state==='done').length;b.tasks=b.tasks.filter(t=>t.state==='waiting'||t.state==='uploading');const pending=b.tasks.length>0;if(!pending){this.waiting.delete(b.id);this.directories.delete(b.id);}return pending;});this.emit();}
 get pending(){return this.batches.some(b=>b.tasks.some(t=>t.state==='waiting'||t.state==='uploading'));}
 get tickets(){return this.batches.flatMap(b=>b.tasks.flatMap(t=>t.ticket?[t.ticket]:[]));}
 reopen(){this.closed=false;this.schedule();}
 close(){this.closed=true;this.cancel();}
}
