import {build} from 'esbuild';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';

const listeners=new Map(),windowListeners=new Map();
Object.defineProperty(globalThis,'navigator',{value:{languages:['zh-CN']},configurable:true});
globalThis.document={documentElement:{lang:''},addEventListener(name,fn){listeners.set(name,fn);}};
globalThis.window={addEventListener(name,fn){windowListeners.set(name,fn);}};
class Element {
 constructor(attributes={},parentElement=null){this.attributes=attributes;this.parentElement=parentElement;}
 closest(selector){for(let node=this;node;node=node.parentElement){const a=node.attributes;if(selector.startsWith('[inert]')){if(a.inert||a.disabled||a['aria-disabled']==='true')return node;}else if(a['data-framely-interactive']!==undefined||a.tabindex!==undefined)return node;}return null;}
}
globalThis.Element=Element;globalThis.getComputedStyle=()=>({cursor:'default'});
const result=await build({stdin:{contents:`import React from 'react';import {renderToStaticMarkup} from 'react-dom/server';import {FramingEditor} from './ui/src/cast-framing-editor';export {installHoverFeedback} from './sdk/src/hover';export const render=()=>renderToStaticMarkup(<FramingEditor spatial settings={{width:1200,height:900,horizontalFov:60,centerX:0,centerY:0}} view={{tanHalfHorizontal:1.4,centerTanX:0,centerTanY:0,aspect:1}} onChange={()=>{}} onError={()=>{}}/>);`,loader:'tsx',resolveDir:process.cwd()},bundle:true,platform:'node',format:'cjs',write:false});
const module={exports:{}};new Function('module','exports','require',result.outputFiles[0].text)(module,module.exports,createRequire(import.meta.url));
const markup=module.exports.render();
assert.equal((markup.match(/data-framely-interactive=""/g)||[]).length,4,'all four corner markers use the shared haptic hook');
const pulses=[];module.exports.installHoverFeedback(kind=>pulses.push(kind));
const frame=new Element({tabindex:0}),nw=new Element({'data-framely-interactive':''},frame),ne=new Element({'data-framely-interactive':''},frame),child=new Element({},nw);
const over=target=>listeners.get('pointerover')({target});
const out=(target,relatedTarget)=>listeners.get('pointerout')({target,relatedTarget});
over(frame);out(frame,nw);over(nw);out(nw,child);over(child);out(child,ne);over(ne);
assert.deepEqual(pulses,['hover','hover','hover'],'entering each corner pulses once; children do not repeat');
frame.attributes.inert=true;out(ne,nw);over(nw);listeners.get('click')({target:nw});assert.equal(pulses.length,3,'pending/closed controls cannot pulse');
frame.attributes.inert=false;windowListeners.get('blur')();over(nw);listeners.get('click')({target:nw});assert.deepEqual(pulses.slice(-2),['hover','activate']);
console.log('PASS: four framing corners use existing preference-aware feedback, one hover pulse per corner, inert editors have no feedback');
