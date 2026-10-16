import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import {webcrypto} from 'node:crypto';
const read=name=>fs.readFileSync(new URL('../src/apk/'+name,import.meta.url),'utf8');
const palette=vm.runInNewContext('('+read('steam_artwork_palette.js')+')');
const rgba=color=>Uint8Array.from(Array.from({length:64*64},()=>color).flat());
const pink=palette(rgba([255,105,153,255]),64,64);
assert.equal(pink.selected,'#ff6999');assert.equal(pink.accent,'#cc547a');
assert.equal(pink.neutral,false);
assert.equal(JSON.stringify(pink),JSON.stringify(palette(rgba([255,105,153,255]),64,64)));
assert.equal(palette(rgba([250,250,250,255]),64,64).neutral,true);
assert.equal(palette(rgba([255,0,0,0]),64,64).neutral,true);
const blue=rgba([26,114,230,255]);blue.set([255,0,0,255],0);
assert.equal(palette(blue,64,64).selected,'#1a72e6');
assert.throws(()=>palette(new Uint8Array(2),64,64));

// Real Steam contract: unsupported type 4 aliases the landscape destination.
const suffix={0:'p',1:'_hero',2:'_logo',3:'',4:''},id=0x92345678;
const encode=color=>Buffer.from(JSON.stringify({width:1,height:1,color})).toString('base64');
class Image {
 set src(uri){const v=JSON.parse(Buffer.from(uri.split(',')[1],'base64').toString());Object.assign(this,v);queueMicrotask(()=>this.onload());}
}
class Canvas {
 constructor(color){this.width=1;this.height=1;this.color=color;}
 getContext(){return {drawImage:image=>{this.color=image.color;},getImageData:()=>({data:Uint8ClampedArray.from(this.color)})};}
}
class FileReader {
 readAsDataURL(blob){blob.arrayBuffer().then(b=>{this.result='data:image/png;base64,'+Buffer.from(b).toString('base64');this.onload();});}
}
const files=new Map(),writes=[];let race=null,failOnce=false,failKind=null;
const fetch=async url=>{const key=url.split('?')[0];if(race){const run=race;race=null;run(key);}const data=files.get(key);return data?new Response(Buffer.from(data,'base64')):new Response(null,{status:404});};
const SteamClient={Apps:{SetCustomArtworkForApp:async(app,data,ext,kind)=>{
 assert.equal(app,id);assert.equal(ext,'png');
 assert.ok([0,1,2,3].includes(kind),'Never send the shortcut icon as custom artwork');
 if(kind===failKind)throw Error('persistent simulated Steam failure');
 if(failOnce){failOnce=false;throw Error('simulated Steam failure');}
 files.set(`/customimages/${id}${suffix[kind]}.png`,data);writes.push(kind);
}}};
const apply=vm.runInNewContext('('+read('steam_artwork_apply.js')+')',{Image,FileReader,document:{createElement:()=>new Canvas()},TextEncoder,Uint8Array,crypto:webcrypto,fetch,SteamClient,setTimeout,clearTimeout,queueMicrotask});
let color=[100,50,10,255];
const render=async()=>[0,1,2,3].map(kind=>({kind,data:encode(color),canvas:new Canvas(color)}));
const params={image:'unused',ownership:null,legacy:[]};
let owned=await apply(params,id,render,palette);
assert.deepEqual(writes,[0,1,2,3]);assert.equal(Object.keys(owned.hashes).length,4);
params.ownership=owned;writes.length=0;
await apply(params,id,render,palette);assert.equal(writes.length,0,'Identical artwork should not be rewritten');
files.set(`/customimages/${id}.png`,encode([250,0,0,255]));
color=[20,60,90,255];writes.length=0;
owned=await apply(params,id,render,palette);
assert.ok(!writes.includes(3),'User landscape must survive');assert.equal(owned.hashes[3],undefined);assert.equal(writes.length,3);
params.ownership=owned;files.set(`/customimages/${id}p.jpg`,encode([0,250,0,255]));
color=[30,70,90,255];writes.length=0;
owned=await apply(params,id,render,palette);assert.ok(!writes.includes(0),'Alternate custom extension must survive');
params.ownership={...owned,appId:id+1};writes.length=0;
await apply(params,id,render,palette);assert.equal(writes.length,0,'Ownership from another shortcut must not be trusted');
files.clear();params.ownership=null;params.legacy=[{kind:0,data:encode([1,2,3,255])}];
files.set(`/customimages/${id}p.png`,params.legacy[0].data);writes.length=0;failOnce=true;
owned=await apply(params,id,render,palette);assert.equal(writes.length,4,'Exact legacy artwork should migrate, with bounded retry');
params.ownership=owned;params.legacy=[];writes.length=0;
// Change a file between inspection and the last pre-write check.
let inspections=0;
const racingFetch=async url=>{if(url.startsWith(`/customimages/${id}p.png`)&&++inspections===2)files.set(`/customimages/${id}p.png`,encode([210,20,20,255]));return fetch(url);};
const racingApply=vm.runInNewContext('('+read('steam_artwork_apply.js')+')',{Image,FileReader,document:{createElement:()=>new Canvas()},TextEncoder,Uint8Array,crypto:webcrypto,fetch:racingFetch,SteamClient,setTimeout,clearTimeout,queueMicrotask});
color=[90,80,70,255];await racingApply(params,id,render,palette);assert.ok(!writes.includes(0),'Concurrent user change must survive');
files.clear();writes.length=0;params.ownership=null;failKind=1;
const checkpoint=await apply(params,id,render,palette);
assert.equal(checkpoint.failed,true);assert.ok(checkpoint.hashes[0]);assert.deepEqual(writes,[0]);
params.ownership=checkpoint;failKind=null;writes.length=0;
const recovered=await apply(params,id,render,palette);
assert.equal(recovered.failed,undefined);assert.deepEqual(writes,[1,2,3]);
params.ownership=recovered;params.ownership.hashes[4]=recovered.hashes[3];params.ownership.hashes[3]='f'.repeat(64);
color=[70,60,50,255];writes.length=0;
const repaired=await apply(params,id,render,palette);
assert.ok(writes.includes(3),'Exact type-4 icon overwrite must be repaired');assert.equal(repaired.hashes[4],undefined);
const unsupported=await apply(params,id,async()=>[{kind:4,data:encode(color),canvas:new Canvas(color)}],palette);
assert.equal(unsupported.failed,true,'Unsupported type must be rejected before writing');
console.log('Steam artwork: generic palette, 80% intensity, deterministic output, ownership, custom images, legacy upgrade, retry, race and partial-failure recovery checks passed');
