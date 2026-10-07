import React,{useEffect,useRef,useState} from 'react';
import {IconPlayerPlayFilled} from '@tabler/icons-react';
import {managerApi} from './manager-api';

type Entry={path:string;name:string;directory:boolean;size:number;modified?:number;mediaRevision?:string|null};
type Cached={url:string|null;bytes:number;refs:number;retired:boolean;failedUntil:number};
type Watcher={notify:(url:string|null)=>void;cached?:Cached};
type Work={key:string;entry:Entry;watchers:Set<Watcher>;controller:AbortController};
const images=new Set(['png','jpg','jpeg','gif','webp','avif','bmp']);
const videos=new Set(['mp4','webm','mov','m4v']);
const cache=new Map<string,Cached>(),work=new Map<string,Work>(),queue:Work[]=[];
const visibility=new Map<Element,(visible:boolean)=>void>();
let observer:IntersectionObserver|undefined,active=0;
function kind(entry:Entry){if(entry.directory)return '';const ext=entry.name.split('.').pop()?.toLowerCase()??'';return images.has(ext)?'image':videos.has(ext)?'video':'';}
function releaseCached(value:Cached){value.refs--;if(value.retired&&value.refs===0&&value.url)URL.revokeObjectURL(value.url);}
function retire(value:Cached){value.retired=true;if(value.refs===0&&value.url)URL.revokeObjectURL(value.url);}
function trimCache(){let bytes=[...cache.values()].reduce((sum,item)=>sum+item.bytes,0);while(cache.size>128||bytes>16*1024*1024){const key=cache.keys().next().value!;const value=cache.get(key)!;cache.delete(key);bytes-=value.bytes;retire(value);}}
function waitFor(element:HTMLImageElement|HTMLVideoElement,event:string,signal:AbortSignal):Promise<void>{
 signal.throwIfAborted();return new Promise((resolve,reject)=>{
  const cleanup=()=>{element.removeEventListener(event,done);element.removeEventListener('error',failed);signal.removeEventListener('abort',abort);};
  const done=()=>{cleanup();resolve();},failed=()=>{cleanup();reject(new Error('Media decoding failed'));},abort=()=>{cleanup();reject(signal.reason);};
  element.addEventListener(event,done,{once:true});element.addEventListener('error',failed,{once:true});signal.addEventListener('abort',abort,{once:true});
 });
}
async function snapshot(source:CanvasImageSource,width:number,height:number,signal:AbortSignal):Promise<Blob>{
 signal.throwIfAborted();if(!width||!height)throw Error('Empty media frame');
 const scale=Math.min(1,320/width,320/height),canvas=document.createElement('canvas');
 canvas.width=Math.max(1,Math.round(width*scale));canvas.height=Math.max(1,Math.round(height*scale));
 const context=canvas.getContext('2d');if(!context)throw Error('Canvas unavailable');
 context.drawImage(source,0,0,canvas.width,canvas.height);
 const blob=await new Promise<Blob>((resolve,reject)=>canvas.toBlob(value=>value?resolve(value):reject(Error('Thumbnail encoding failed')),'image/webp',.8));
 signal.throwIfAborted();return blob;
}
async function decode(url:string,video:boolean,signal:AbortSignal):Promise<Blob>{
 if(!video){
  const image=new Image();image.decoding='async';
  try{const loaded=waitFor(image,'load',signal);image.src=url;await loaded;return await snapshot(image,image.naturalWidth,image.naturalHeight,signal);}
  finally{image.removeAttribute('src');}
 }
 const player=document.createElement('video');player.muted=true;player.playsInline=true;player.preload='metadata';
 try{
  const metadata=waitFor(player,'loadedmetadata',signal);player.src=url;await metadata;
  if(player.duration<=0)throw Error('Empty video');
  // Some local WebM files omit duration metadata but still support seeking.
  const target=Number.isFinite(player.duration)?Math.min(1,Math.max(0,player.duration-.05)):1;
  if(target>0){const sought=waitFor(player,'seeked',signal);player.currentTime=target;await sought;}
  if(player.readyState<2)await waitFor(player,'loadeddata',signal);
  return await snapshot(player,player.videoWidth,player.videoHeight,signal);
 }finally{player.pause();player.removeAttribute('src');player.load();}
}
async function generate(task:Work):Promise<Blob>{
 const signal=task.controller.signal;let token:string|undefined;
 try{
  const result=await managerApi<{url:string;token?:string}>('files',{operation:'preview',path:task.entry.path},signal);token=result.token;
  signal.throwIfAborted();return await decode(result.url,kind(task.entry)==='video',signal);
 }finally{
  // The cached thumbnail no longer needs the source media token. Source bytes
  // remain on Frame except for this normal authenticated browser preview.
  if(token)void managerApi('files',{operation:'preview.release',token}).catch(()=>{});
 }
}
function pump(){
 while(active<2&&queue.length){
  const task=queue.shift()!;if(!task.watchers.size||task.controller.signal.aborted)continue;active++;
  const timeout=setTimeout(()=>task.controller.abort(new DOMException('Thumbnail timed out','TimeoutError')),10000);
  void generate(task).then(blob=>{
   if(task.controller.signal.aborted||!task.watchers.size)return;
   const cached:Cached={url:URL.createObjectURL(blob),bytes:blob.size,refs:0,retired:false,failedUntil:0};
   cache.set(task.key,cached);for(const watcher of task.watchers){watcher.cached=cached;cached.refs++;watcher.notify(cached.url);}trimCache();
  }).catch(()=>{
   if(task.controller.signal.aborted||!task.watchers.size)return;
   const cached:Cached={url:null,bytes:0,refs:0,retired:false,failedUntil:Date.now()+60000};
   cache.set(task.key,cached);for(const watcher of task.watchers){watcher.cached=cached;cached.refs++;watcher.notify(null);}trimCache();
  }).finally(()=>{clearTimeout(timeout);if(work.get(task.key)===task)work.delete(task.key);active--;pump();});
 }
}
function subscribe(key:string,entry:Entry,notify:Watcher['notify']){
 const cached=cache.get(key);
 if(cached&&(!cached.failedUntil||cached.failedUntil>Date.now())){
  cache.delete(key);cache.set(key,cached);cached.refs++;notify(cached.url);return()=>releaseCached(cached);
 }
 if(cached){cache.delete(key);retire(cached);}
 let task=work.get(key);
 if(!task||task.controller.signal.aborted){task={key,entry,watchers:new Set(),controller:new AbortController()};work.set(key,task);queue.push(task);}
 const watcher:Watcher={notify};task.watchers.add(watcher);pump();
 return()=>{task!.watchers.delete(watcher);if(watcher.cached)releaseCached(watcher.cached);else if(!task!.watchers.size){task!.controller.abort();const index=queue.indexOf(task!);if(index>=0)queue.splice(index,1);if(work.get(key)===task)work.delete(key);}};
}
export function FileThumbnail({entry,children,active=true}:{entry:Entry;children:React.ReactNode;active?:boolean}){
 const node=useRef<HTMLSpanElement>(null),[visible,setVisible]=useState(false),[url,setUrl]=useState<string|null>(null);
 const type=kind(entry),key=JSON.stringify([entry.path,entry.mediaRevision??[entry.size,entry.modified],type]);
 useEffect(()=>{
  if(!type||!node.current||typeof IntersectionObserver==='undefined')return;
  observer??=new IntersectionObserver(entries=>{for(const item of entries)visibility.get(item.target)?.(item.isIntersecting);},{threshold:.01});
  const target=node.current;visibility.set(target,setVisible);observer.observe(target);
  return()=>{observer?.unobserve(target);visibility.delete(target);};
 },[type]);
 useEffect(()=>{
  setUrl(null);if(!active||!visible||!type)return;
  let dispose:(()=>void)|undefined,live=true;
  const timer=setTimeout(()=>{dispose=subscribe(key,entry,value=>{if(live)setUrl(value);});},120);
  return()=>{live=false;clearTimeout(timer);dispose?.();};
 },[key,visible,type,active]);
 return <span className="file-thumbnail" ref={node}>{url?<><img src={url} className="file-media-thumbnail" alt="" draggable={false}/>{type==='video'&&<IconPlayerPlayFilled className="file-video-marker" size={14} aria-hidden="true"/>}</>:children}</span>;
}
