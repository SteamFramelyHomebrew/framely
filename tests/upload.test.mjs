import test from 'node:test';
import assert from 'node:assert/strict';
import {build} from 'esbuild';
const result=await build({entryPoints:['ui/src/upload.ts'],bundle:true,write:false,platform:'node',format:'esm'});
const {uploadFile}=await import('data:text/javascript;base64,'+Buffer.from(result.outputFiles[0].contents).toString('base64'));
test('250 MiB selection uses bounded blobs and never reads the entire file',async()=>{
 const size=250*1024*1024,chunk=4*1024*1024;let received=0,calls=0,last=0,nextOffset=0;
 const file={size,arrayBuffer(){throw Error('whole file read');},slice(start,end){assert.equal(start,nextOffset);nextOffset+=chunk;return new Blob([new Uint8Array(Math.min(end,size)-start)]);}};
 const id=await uploadFile(file,new AbortController().signal,p=>last=p,{start:async n=>{assert.equal(n,size);return {upload:'ticket',chunkSize:chunk};},chunk:async(id,offset,blob)=>{assert.equal(id,'ticket');assert.equal(offset,received);assert.ok(blob.size<=chunk);received+=blob.size;calls++;},abort:async()=>assert.fail('unexpected abort')});
 assert.equal(id,'ticket');assert.equal(received,size);assert.equal(calls,63);assert.equal(last,100);
});
test('cancellation stops scheduling new chunks and clears the upload',async()=>{
 const controller=new AbortController();let calls=0,aborted='';
 await assert.rejects(uploadFile({size:100,slice:()=>new Blob(['a'])},controller.signal,()=>{controller.abort();},{start:async()=>({upload:'partial',chunkSize:1}),chunk:async()=>calls++,abort:async id=>aborted=id}),{name:'AbortError'});
 assert.equal(calls,4);assert.equal(aborted,'partial');
});
test('invalid server chunk size and failed requests clean up uploads',async()=>{
 for(const chunkSize of [0,8*1024*1024,1]){let aborted=false;await assert.rejects(uploadFile({size:2,slice:()=>new Blob(['a'])},new AbortController().signal,()=>{},{start:async()=>({upload:'partial',chunkSize}),chunk:async()=>{throw Error('network');},abort:async()=>{aborted=true;}}));assert.ok(aborted);}
});

test('four workers allow out-of-order completion with monotonic acknowledged progress',async()=>{
 const pending=new Map(),seen=[],progress=[];let active=0,maxActive=0;
 const task=uploadFile({size:18,slice:(a,b)=>new Blob([new Uint8Array(b-a)])},new AbortController().signal,p=>progress.push(p),{
  start:async()=>({upload:'parallel',chunkSize:4}),
  chunk:async(id,offset,blob)=>{seen.push(offset);active++;maxActive=Math.max(maxActive,active);await new Promise(resolve=>pending.set(offset,resolve));active--;},
  abort:async()=>assert.fail('unexpected abort')
 });
 const settle=()=>new Promise(resolve=>setImmediate(resolve));
 await settle();assert.deepEqual(seen,[0,4,8,12]);
 pending.get(12)();await settle();assert.deepEqual(seen,[0,4,8,12,16]);assert.deepEqual(progress,[22]);
 pending.get(16)();await settle();assert.deepEqual(progress,[22,33]);
 for(const offset of [8,4,0]){pending.get(offset)();await settle();}
 assert.equal(await task,'parallel');assert.equal(maxActive,4);assert.equal(progress.at(-1),100);
 assert.deepEqual(progress,[...progress].sort((a,b)=>a-b));
});
test('a failed worker cancels and drains other requests before dropping the upload',async()=>{
 let calls=0,active=0,aborted=false;
 await assert.rejects(uploadFile({size:20,slice:(a,b)=>new Blob([new Uint8Array(b-a)])},new AbortController().signal,()=>assert.fail('no acknowledgements'),{
  start:async()=>({upload:'failed',chunkSize:4}),
  chunk:async(id,offset,blob,signal)=>{calls++;active++;try{if(offset===8)throw Error('write failed');await new Promise((resolve,reject)=>{if(signal.aborted)reject(signal.reason);else signal.addEventListener('abort',()=>reject(signal.reason),{once:true});});}finally{active--; }},
  abort:async id=>{assert.equal(id,'failed');assert.equal(active,0);aborted=true;}
 }),/write failed/);
 assert.ok(calls<=4);assert.ok(aborted);
});

test('empty files finish without chunk requests',async()=>{
 assert.equal(await uploadFile({size:0,slice:()=>assert.fail('no chunks')},new AbortController().signal,()=>assert.fail('no progress'),{
  start:async()=>({upload:'empty',chunkSize:4*1024*1024}),chunk:async()=>assert.fail('no requests'),abort:async()=>assert.fail('no abort')
 }),'empty');
});
test('byte progress tracks successful chunks exactly',async()=>{
 const received=[];
 await uploadFile({size:10,slice:(a,b)=>new Blob([new Uint8Array(b-a)])},new AbortController().signal,(percent,bytes)=>received.push([percent,bytes]),{
  start:async()=>({upload:'bytes',chunkSize:4}),chunk:async()=>{},abort:async()=>assert.fail('no abort')
 });
 assert.deepEqual(received,[[40,4],[80,8],[100,10]]);
});
