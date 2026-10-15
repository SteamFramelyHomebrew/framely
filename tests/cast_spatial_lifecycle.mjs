import {build} from 'esbuild';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {resolve} from 'node:path';

Object.defineProperty(globalThis,'navigator',{value:{languages:['zh-CN']},configurable:true});
globalThis.document={documentElement:{lang:'',classList:{add(){},remove(){}}}};
globalThis.window={addEventListener(){},removeEventListener(){}};
const reactPath=resolve('node_modules/react/index.js');
// A small hook runner exercises this page's async lifecycle without mounting
// video/CEF or adding a DOM-test dependency. Children remain React elements.
const hooks=`
import React from ${JSON.stringify(reactPath)};
export * from ${JSON.stringify(reactPath)};
export default React;
let values=[],cursor=0,effects=[];
const same=(a,b)=>a&&b&&a.length===b.length&&a.every((v,i)=>Object.is(v,b[i]));
export function useState(initial){const i=cursor++;if(!(i in values))values[i]=typeof initial==='function'?initial():initial;return [values[i],value=>{values[i]=typeof value==='function'?value(values[i]):value;}];}
export function useRef(initial){const i=cursor++;return values[i]??(values[i]={current:initial});}
export function useMemo(fn,deps){const i=cursor++;if(!same(values[i]?.deps,deps))values[i]={deps,value:fn()};return values[i].value;}
export function useEffect(fn,deps){const i=cursor++;if(!same(values[i]?.deps,deps)){const old=values[i];values[i]={deps};effects.push(()=>{old?.cleanup?.();values[i].cleanup=fn();});}}
export const useLayoutEffect=useEffect;
export function render(Component){cursor=0;return Component();}
export function flush(){const pending=effects;effects=[];for(const effect of pending)effect();}
export function reset(){for(const value of values)value?.cleanup?.();values=[];cursor=0;effects=[];}
`;
const api=`let handler;export const calls=[];export function setHandler(next){handler=next;calls.length=0;}export function api(method,params,signal){calls.push({method,params,signal});return Promise.resolve().then(()=>handler(method,params));}`;
const result=await build({stdin:{contents:`export {SpatialFraming} from './ui/src/cast-spatial-framing';export {render,flush,reset} from 'react';export {calls,setHandler} from './ui/src/api';`,resolveDir:process.cwd()},bundle:true,platform:'node',format:'cjs',loader:{'.css':'empty'},write:false,plugins:[{name:'page-harness',setup(build){build.onResolve({filter:/^react$/},()=>({path:'hooks',namespace:'test'}));build.onLoad({filter:/^hooks$/,namespace:'test'},()=>({contents:hooks,loader:'js',resolveDir:process.cwd()}));build.onLoad({filter:/\/ui\/src\/api\.ts$/},()=>({contents:api,loader:'js'}));}}]});
const module={exports:{}};new Function('module','exports','require',result.outputFiles[0].text)(module,module.exports,createRequire(import.meta.url));
const {SpatialFraming,render,flush,reset,calls,setHandler}=module.exports;
const geometry={id:'editor-1',referenceTan:1.4,settings:{width:1200,height:900,horizontalFov:60,centerX:0,centerY:0},view:{tanHalfHorizontal:.7,centerTanX:0,centerTanY:0,eye:1,visibility:{size:32,extent:1.4,cells:'f'.repeat(256)}}};
const tick=()=>new Promise(resolve=>setImmediate(resolve));
function walk(node,visit){if(!node||typeof node!=='object')return;visit(node);for(const child of [node.props?.children].flat(Infinity))walk(child,visit);}
function draw(){const tree=render(SpatialFraming);walk(tree,node=>{if(node.props?.ref){const panel=node.type==='section';node.props.ref.current={getBoundingClientRect:()=>({width:panel?340:1600,height:panel?300:1600})};}});flush();return tree;}
function button(tree,label){let found;walk(tree,node=>{if(node.type==='button'&&node.props.children===label)found=node;});assert.ok(found,label);return found;}
async function mount(finish){reset();setHandler((method,params)=>method==='status'?{database:{language:'zh-CN'}}:method==='language.list'?{packs:[]}:method==='cast.framing.get'?geometry:method==='cast.framing.reference'?true:finish(method,params));for(let i=0;i<4;i++){draw();await tick();}return draw();}

let resolveApply;
let tree=await mount(method=>{assert.equal(method,'cast.framing.apply');return new Promise(resolve=>{resolveApply=resolve;});});
const apply=button(tree,'应用取景').props.onClick;
apply();apply();await tick();tree=draw();
assert.equal(tree.props.inert,true);
assert.equal(calls.filter(call=>call.method==='cast.framing.apply').length,1,'double click cannot apply twice');
resolveApply(true);await tick();assert.equal(draw(),null,'successful apply immediately removes the stale editor');
apply();await tick();assert.equal(calls.filter(call=>call.method==='cast.framing.apply').length,1);

tree=await mount(()=>{throw Error('Fixture save rejected');});
button(tree,'应用取景').props.onClick();await tick();tree=draw();
assert.ok(tree,'save failure leaves the editor usable');assert.equal(tree.props.inert,false);
let error;walk(tree,node=>{if(node.props?.role==='alert')error=node.props.children;});assert.equal(error,'Fixture save rejected');
assert.equal(button(tree,'应用取景').props.disabled,false);

tree=await mount(()=>true);button(tree,'取消').props.onClick();await tick();assert.equal(draw(),null,'cancel has the same terminal state');

tree=await mount(()=>{throw Error('Saved, but restart failed');});
setHandler(method=>{if(method==='cast.framing.apply')throw Error('Saved, but restart failed');if(method==='cast.framing.get')throw Error('画幅调整窗口已关闭');throw Error(method);});
button(tree,'应用取景').props.onClick();await tick();assert.equal(draw(),null,'post-save restart failure cannot revive a closed editor');

const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
function frame(tree){let found;walk(tree,node=>{if(node.props?.spatial&&node.props?.settings&&node.props?.onChange)found=node;});assert.ok(found,'framing editor');return found;}
tree=await mount(()=>true);await delay(120);
let references=calls.filter(call=>call.method==='cast.framing.reference');
assert.equal(references.length,1);assert.deepEqual(references[0].params,{id:geometry.id,centerX:0,centerY:0});
frame(tree).props.onChange({centerX:5,centerY:-3});tree=draw();
frame(tree).props.onChange({centerX:6,centerY:-4});tree=draw();await delay(120);
references=calls.filter(call=>call.method==='cast.framing.reference');
assert.equal(references.length,2,'rapid committed centers are debounced');
assert.ok(Math.abs(references[1].params.centerX-6)<1e-8&&Math.abs(references[1].params.centerY+4)<1e-8,'live draft is sent in degrees');
assert.ok(references[0].signal.aborted,'previous reference request is canceled after a new center');
frame(tree).props.onChange({centerX:7});tree=draw();
button(tree,'取消').props.onClick();await tick();assert.equal(draw(),null);await delay(120);
assert.equal(calls.filter(call=>call.method==='cast.framing.reference').length,2,'closing cancels the pending depth-probe update');
console.log('PASS: terminal editor lifecycle, error recovery, debounced live-center depth updates and closing cancellation');
