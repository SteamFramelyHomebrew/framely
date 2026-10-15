import {build} from 'esbuild';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
const bundle=await build({stdin:{contents:`export {normalizeRegion} from './ui/src/cast-region';export {frameFromSettings,settingsFromFrame,framePresets,frameRatios} from './ui/src/cast-framing';`,resolveDir:process.cwd()},bundle:true,platform:'node',format:'cjs',write:false});
const module={exports:{}};new Function('module','exports','require',bundle.outputFiles[0].text)(module,module.exports,createRequire(import.meta.url));
const {normalizeRegion,frameFromSettings,settingsFromFrame,framePresets}=module.exports;
assert.ok(module.exports.frameRatios.some(r=>r.label==='3:4'&&r.value===3/4));
const reference={tanHalfHorizontal:1.4,centerTanX:0,centerTanY:0,aspect:1};
for(const aspect of [4/3,3/4,16/9,9/16,1])for(const size of framePresets(aspect)){
 const input={...size,horizontalFov:72,centerX:12,centerY:-8};
 const saved=normalizeRegion(input,reference),rect=frameFromSettings(saved,saved.width/saved.height,reference),effective=settingsFromFrame(rect,reference);
 for(const key of ['horizontalFov','centerX','centerY'])assert.ok(Math.abs(saved[key]-effective[key])<1e-9,`${key}: displayed box differs from saved capture`);
 assert.ok(Math.abs(rect.width/rect.height-saved.width/saved.height)<1e-10);
}
const fitted=normalizeRegion({width:1080,height:1920,horizontalFov:150,centerX:60,centerY:60},reference);
assert.ok(fitted.horizontalFov<150);assert.ok(fitted.centerX<=60&&fitted.centerY<=60);
console.log('PASS: frame ratio/resolution changes save exactly the displayed crop');
