import {build} from 'esbuild';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
const result=await build({stdin:{contents:`export * from './ui/src/cast-framing-gesture';export {normalizeRegion} from './ui/src/cast-region';export {frameFromSettings} from './ui/src/cast-framing';`,resolveDir:process.cwd()},bundle:true,platform:'node',format:'cjs',write:false});
const module={exports:{}};new Function('module','exports','require',result.outputFiles[0].text)(module,module.exports,createRequire(import.meta.url));
const {FrameGesture,fitMinimumFrame,minimumFrameWidth,normalizeRegion,frameFromSettings}=module.exports;
const close=(a,b)=>assert.ok(Math.abs(a-b)<1e-10,`${a} != ${b}`);
for(const ratio of [4/3,3/4,16/9,9/16,1]){
 const minimum=minimumFrameWidth(ratio,{width:.24,height:.28},.05);
 const rect=fitMinimumFrame({x:.3,y:.3,width:.4,height:.4/ratio},ratio,minimum),cx=rect.x+rect.width/2,cy=rect.y+rect.height/2;
 for(const corner of ['nw','ne','sw','se']){
  const sx=corner.includes('w')?-1:1,sy=corner.includes('n')?-1:1,gesture=new FrameGesture();
  gesture.begin(1,500,500,rect,corner);
  const smaller=gesture.move(1,500-sx*40,500-sy*40,1,{width:1000,height:1000},ratio,minimum);
  const larger=gesture.move(1,500+sx*40,500+sy*40,1,{width:1000,height:1000},ratio,minimum);
  assert.ok(smaller.width<=rect.width&&larger.width>=rect.width);
  for(const next of [smaller,larger]){close(next.x+next.width/2,cx);close(next.y+next.height/2,cy);assert.ok(next.width>=.24-1e-10&&next.height>=.28-1e-10);close(next.width/next.height,ratio);}
  assert.equal(gesture.move(1,800,800,0,{width:1000,height:1000},ratio,minimum),null);
  assert.equal(gesture.active(),false); // lost pointerup cannot leave a drag armed
  assert.equal(gesture.move(1,300,300,1,{width:1000,height:1000},ratio,minimum),null);
  gesture.begin(2,500,500,rect,corner);assert.equal(gesture.finish(1),false);assert.equal(gesture.finish(2),true);
  assert.equal(gesture.move(2,300,300,0,{width:1000,height:1000},ratio,minimum),null);
 }
 const reference={tanHalfHorizontal:1.4,centerTanX:0,centerTanY:0,aspect:1};
 const saved=normalizeRegion({width:Math.round(1600*ratio),height:1600,horizontalFov:10,centerX:0,centerY:0},reference,{width:.24,height:.28});
 const displayed=frameFromSettings(saved,saved.width/saved.height,reference);assert.ok(displayed.width>=.24-1e-10&&displayed.height>=.28-1e-10);
}
console.log('PASS: all four corners grow/shrink around center, menu minimum survives save/ratio changes, release/cancel/hover cannot continue dragging');
