import test from 'node:test';
import assert from 'node:assert/strict';
import {build} from 'esbuild';
const result=await build({entryPoints:['ui/src/upload.ts'],bundle:true,write:false,platform:'node',format:'esm'});
const {uploadFile}=await import('data:text/javascript;base64,'+Buffer.from(result.outputFiles[0].contents).toString('base64'));
test('250 MiB selection uses bounded blobs and never reads the entire file',async()=>{
 const size=250*1024*1024,chunk=512*1024;let received=0,calls=0,last=0;
 const file={size,arrayBuffer(){throw Error('whole file read');},slice(start,end){assert.equal(start,received);return new Blob([new Uint8Array(Math.min(end,size)-start)]);}};
 const id=await uploadFile(file,new AbortController().signal,p=>last=p,{start:async n=>{assert.equal(n,size);return {upload:'ticket',chunkSize:chunk};},chunk:async(id,offset,blob)=>{assert.equal(id,'ticket');assert.equal(offset,received);assert.ok(blob.size<=chunk);received+=blob.size;calls++;},abort:async()=>assert.fail('unexpected abort')});
 assert.equal(id,'ticket');assert.equal(received,size);assert.equal(calls,500);assert.equal(last,100);
});
test('cancellation clears partial upload before another chunk',async()=>{
 const controller=new AbortController();let calls=0,aborted='';
 await assert.rejects(uploadFile({size:100,slice:()=>new Blob(['a'])},controller.signal,()=>{controller.abort();},{start:async()=>({upload:'partial',chunkSize:1}),chunk:async()=>calls++,abort:async id=>aborted=id}),{name:'AbortError'});
 assert.equal(calls,1);assert.equal(aborted,'partial');
});
test('invalid server chunk size and failed requests clean up uploads',async()=>{
 for(const chunkSize of [0,1024*1024,1]){let aborted=false;await assert.rejects(uploadFile({size:2,slice:()=>new Blob(['a'])},new AbortController().signal,()=>{},{start:async()=>({upload:'partial',chunkSize}),chunk:async()=>{throw Error('network');},abort:async()=>{aborted=true;}}));assert.ok(aborted);}
});
