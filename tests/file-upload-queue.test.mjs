import test from 'node:test';import assert from 'node:assert/strict';import {build} from 'esbuild';
const bundle=async(path)=>{const r=await build({entryPoints:[path],bundle:true,write:false,platform:'node',format:'esm'});return import('data:text/javascript;base64,'+Buffer.from(r.outputFiles[0].contents).toString('base64'));};
const {FileUploadQueue,ChunkSlots}=await bundle('ui/src/file-upload-queue.ts');
const {droppedUploadEntries,selectedUploadEntries}=await bundle('ui/src/file-upload-entries.ts');
const wait=()=>new Promise(r=>setImmediate(r));const until=async(f)=>{for(let i=0;i<200;i++){if(f())return;await wait();}assert.fail('Timed out');};
const file=(name,size=8)=>({name,webkitRelativePath:'',size,slice:(a,b)=>new Blob([new Uint8Array(b-a)])});
test('directory readers are fully drained and empty folders retained',async()=>{
 const regular={name:'readme',isFile:true,file:done=>done(file('readme'))};let reads=0;
 const root={name:'folder',isDirectory:true,createReader:()=>({readEntries:done=>done(reads++===0?[regular]:reads===2?[{name:'empty',isDirectory:true,createReader:()=>({readEntries:done=>done([])})}]:[])})};
 const entries=await droppedUploadEntries({items:[{kind:'file',webkitGetAsEntry:()=>root}],files:[]},new AbortController().signal);
 assert.deepEqual(entries.map(e=>e.name),['folder','folder/readme','folder/empty']);assert.equal(reads,3);
 const chosen=file('x');chosen.webkitRelativePath='中文/a/x';assert.deepEqual((await selectedUploadEntries([chosen])).map(e=>e.name),['中文','中文/a','中文/a/x']);
 await assert.rejects(selectedUploadEntries([{...chosen,webkitRelativePath:'../bad'}]),/Invalid upload path/);
});
test('directory dependencies, fixed destinations, skips, failures and retries are independent',async()=>{
 let fail=true;const calls=[],chunks=[];
 const q=new FileUploadQueue({api:async p=>{calls.push(p);if(p.operation==='upload.directory')return {status:'done'};if(p.operation==='upload.start'){if(p.name==='skip')return {status:'skipped',path:'/first/skip'};if(p.name==='bad'&&fail)throw Error('permission');return {id:p.name,chunkSize:4};}return {status:'done',path:'/first/'+p.id};},chunk:async(id,offset)=>chunks.push([id,offset])});
 q.add('/first',{dev:1,ino:2},'skip',[{name:'root',directory:true},{name:'root/file',directory:false,file:file('file')},{name:'skip',directory:false,file:file('skip')},{name:'bad',directory:false,file:file('bad')}]);
 await until(()=>!q.pending);assert.deepEqual(q.batches[0].tasks.map(t=>t.state),['done','done','skipped','failed']);assert.equal(chunks.filter(c=>c[0]==='skip').length,0);
 assert.ok(calls.findIndex(c=>c.operation==='upload.directory')<calls.findIndex(c=>c.name==='root/file'));assert.ok(calls.filter(c=>c.operation==='upload.start').every(c=>c.directory==='/first'));
 fail=false;q.retry();await until(()=>!q.pending);assert.equal(q.batches[0].tasks[3].state,'done');q.clear();assert.equal(q.batches.length,0);
});
test('file concurrency changes without aborting active tasks and global chunks never exceed eight',async()=>{
 let active=0,max=0;const pending=[];
 const q=new FileUploadQueue({api:async p=>p.operation==='upload.start'?{id:p.name,chunkSize:4}:{status:'done'},chunk:async(id,o,b,signal)=>{active++;max=Math.max(max,active);try{await new Promise((resolve,reject)=>{pending.push(resolve);signal.addEventListener('abort',()=>reject(signal.reason),{once:true});});}finally{active--;}}});
 q.setLimit(8);q.add('/target',{dev:1,ino:2},'keep',Array.from({length:12},(_,i)=>({name:String(i),directory:false,file:file(String(i))})));
 await until(()=>active===8);assert.equal(q.batches[0].tasks.filter(t=>t.state==='uploading').length,8);q.setLimit(1);
 for(let round=0;round<50&&q.pending;round++){pending.splice(0).forEach(f=>f());await wait();}
 await until(()=>!q.pending);assert.equal(max,8);assert.ok(q.batches[0].tasks.every(t=>t.state==='done'));
});
test('cancel drains requests and releases chunk slots, allowing another batch',async()=>{
 const q=new FileUploadQueue({api:async p=>p.operation==='upload.start'?{id:p.name,chunkSize:4}:{status:'done'},chunk:async(id,o,b,signal)=>{if(id==='first')await new Promise((resolve,reject)=>signal.addEventListener('abort',()=>reject(signal.reason),{once:true}));}});
 q.setLimit(1);const first=q.add('/one',{dev:1,ino:1},'keep',[{name:'first',directory:false,file:file('first')}]);await wait();q.cancel(undefined,first.id);
 q.add('/two',{dev:1,ino:2},'keep',[{name:'second',directory:false,file:file('second')}]);await until(()=>!q.pending);
 assert.equal(q.batches[0].tasks[0].state,'cancelled');assert.equal(q.batches[1].tasks[0].state,'done');
 const slots=new ChunkSlots(1),hold=await slots.acquire(new AbortController().signal),cancel=new AbortController();const blocked=slots.acquire(cancel.signal);cancel.abort();await assert.rejects(blocked);hold();(await slots.acquire(new AbortController().signal))();
});
test('a restored browser page can add a fresh batch after closing its old queue',async()=>{
 const q=new FileUploadQueue({api:async p=>p.operation==='upload.start'?{id:p.name,chunkSize:4}:{status:'done'},chunk:async()=>{}});
 q.close();q.reopen();q.add('/restored',{dev:1,ino:2},'keep',[{name:'new',directory:false,file:file('new',0)}]);
 await until(()=>!q.pending);assert.equal(q.batches[0].tasks[0].state,'done');
});
