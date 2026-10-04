import {installKeyboard} from './keyboard';
import {installScrollbars} from './scrollbars';
import {installHoverFeedback} from './hover';
export {};
type Pending={resolve:(v:unknown)=>void;reject:(e:Error)=>void;timer:ReturnType<typeof setTimeout>};
const pending=new Map<number,Pending>();let next=1;
const listeners=new Set<(event:unknown)=>void>();
function request(op:string,params:unknown={}):Promise<any>{const id=next++;return new Promise((resolve,reject)=>{const timer=setTimeout(()=>{pending.delete(id);reject(new Error('Framely request timed out'));},90000);pending.set(id,{resolve,reject,timer});parent.postMessage({channel:'framely.plugin',id,op,params},'*');});}
window.addEventListener('message',e=>{if(e.source!==parent||e.data?.channel!=='framely.reply')return;const data=e.data;if(data.event){listeners.forEach(f=>f(data.event));return;}const p=pending.get(data.id);if(!p)return;pending.delete(data.id);clearTimeout(p.timer);if(data.error)p.reject(new Error(data.error));else p.resolve(data.result);});
(window as any).__framelyBridge={request,subscribe:(f:(e:unknown)=>void)=>{listeners.add(f);return()=>listeners.delete(f);}};
installKeyboard(params=>request('keyboard',params));

window.addEventListener('pagehide',()=>{for(const p of pending.values()){clearTimeout(p.timer);p.reject(new Error('Plugin page closed'));}pending.clear();});

installHoverFeedback(()=>{void request('haptic').catch(console.error);});

installScrollbars();
