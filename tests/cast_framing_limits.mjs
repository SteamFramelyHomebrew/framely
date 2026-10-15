import {build} from 'esbuild';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFile} from 'node:fs/promises';
const bundle=await build({stdin:{contents:`export * from './ui/src/cast-framing-limits';export {normalizeRegion} from './ui/src/cast-region';export {frameFromSettings,settingsFromFrame} from './ui/src/cast-framing';`,resolveDir:process.cwd()},bundle:true,platform:'node',format:'cjs',write:false});
const module={exports:{}};new Function('module','exports','require',bundle.outputFiles[0].text)(module,module.exports,createRequire(import.meta.url));
const {FramingLimits,framingMenuScale,normalizeRegion,frameFromSettings,settingsFromFrame}=module.exports;
const close=(a,b)=>assert.ok(Math.abs(a-b)<1e-8,`${a} != ${b}`);
for(const extent of [.9,1.695,1.734])close(framingMenuScale(extent)*extent,.9);
for(const eye of [0,1]){
 const size=64,bits=[];for(let y=0;y<size;y++)for(let x=0;x<size;x++){
  const u=(x+.5)/size,v=(y+.5)/size,lower=.1+Math.abs(v-.5)*.2,upper=.9-Math.abs(v-.5)*.5;
  bits.push(v>.12&&v<.88&&(eye===0?u>lower&&u<upper:1-u>lower&&1-u<upper));
 }
 let cells='';for(let i=0;i<bits.length;i+=4)cells+=(+bits[i]+(+bits[i+1]<<1)+(+bits[i+2]<<2)+(+bits[i+3]<<3)).toString(16);
 const limit=new FramingLimits({size,extent:1.7,cells},1.7);
 for(const ratio of [4/3,3/4,16/9,9/16,1]){
  const min=Math.max(.10,.12*ratio),reset=limit.reset(ratio,min);
  assert.ok(limit.contains(reset));close(reset.width/reset.height,ratio);
  const grown=limit.fit({...reset,x:reset.x-.2,y:reset.y-.2/ratio,width:reset.width+.4,height:reset.height+.4/ratio},ratio,min,reset);
  assert.ok(limit.contains(grown));close(grown.x+grown.width/2,reset.x+reset.width/2);close(grown.y+grown.height/2,reset.y+reset.height/2);
  const moved=limit.fit({...reset,x:-2},ratio,min,reset);assert.ok(limit.contains(moved));close(moved.width,reset.width);
  const smaller=limit.fit({...reset,x:reset.x+reset.width/4,y:reset.y+reset.height/4,width:reset.width/2,height:reset.height/2},ratio,min,reset);
  assert.ok(limit.contains(smaller));assert.ok(smaller.width>=min-1e-10);
  const reference={tanHalfHorizontal:1.7,centerTanX:0,centerTanY:0,aspect:1};
  const sizeSettings={width:Math.round(ratio*1600),height:1600,...settingsFromFrame(grown,reference)};
  const saved=normalizeRegion(sizeSettings,reference,{width:.1,height:.12},limit.fit.bind(limit));
  const displayed=frameFromSettings(saved,saved.width/saved.height,reference);assert.ok(limit.contains(displayed));
 }
 assert.throws(()=>limit.reset(1,.95));
}
if(process.env.FRAMELY_VIEW_FIXTURE){
 const views=JSON.parse(await readFile(process.env.FRAMELY_VIEW_FIXTURE,'utf8'));
 const checks=[];
 for(const view of views){const limit=new FramingLimits(view.visibility,view.editorReferenceTan),reference={tanHalfHorizontal:view.editorReferenceTan,centerTanX:0,centerTanY:0,aspect:1};
  for(const [width,height] of [[2560,1920],[1920,2560],[2560,1440],[1440,2560],[1920,1920]]){
   const ratio=width/height,rect=limit.reset(ratio,Math.max(.1,.1*ratio));assert.ok(limit.contains(rect));
   checks.push({eye:view.eye,width,height,...settingsFromFrame(rect,reference)});
  }
 }
 console.log(JSON.stringify(checks));
}else console.log('PASS: menu angular size fixed; both-eye masks clamp growth/movement/reset and saved crops stay inside actual visibility');
