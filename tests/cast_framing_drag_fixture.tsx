import React,{useRef,useState} from 'react';
import {createRoot} from 'react-dom/client';
import {FramingEditor} from '../ui/src/cast-framing-editor';
import type {CastSettings} from '../ui/src/casting';
import {configureLanguage} from '../ui/src/i18n';
import '../ui/src/style.css';
import '../ui/src/casting.css';
import '../ui/src/cast-spatial-framing.css';
configureLanguage('zh-CN',[]);
let commits:Partial<CastSettings>[]=[];
const initial:CastSettings={source:'screen',codec:'h265',output:'eye',eye:'right',width:1600,height:1200,fps:60,bitrateMbps:8,horizontalFov:60,centerX:0,centerY:0,systemAudio:true,microphone:false,airplay:false,dlna:false,receiverName:'Framely'};
function Fixture(){
 const handle=useRef<HTMLButtonElement>(null),[settings,setSettings]=useState(initial);
 return <div className="app"><FramingEditor spatial moveHandle={handle} settings={settings} view={{tanHalfHorizontal:1.4,centerTanX:0,centerTanY:0,aspect:1}} onChange={patch=>{commits.push(patch);setSettings(s=>({...s,...patch}));}} onError={error=>{throw Error(error);}}/><button ref={handle}>拖动调整录制位置</button></div>;
}
const wait=(ms=80)=>new Promise(r=>setTimeout(r,ms));
const check=(value:unknown,message:string)=>{if(!value)throw Error(message);};
const close=(a:number,b:number)=>Math.abs(a-b)<.03;
const pointer=(target:EventTarget,type:string,x:number,y:number,buttons=1,id=8)=>target.dispatchEvent(new PointerEvent(type,{bubbles:true,cancelable:true,button:0,buttons,pointerId:id,clientX:x,clientY:y}));
(window as any).runInstallReviewChecks=async()=>{try{
 for(let i=0;i<100&&!document.querySelector('.cast-crop-box');i++)await wait(20);
 await wait(100);
 const box=document.querySelector<HTMLElement>('.cast-crop-box')!,handle=document.querySelector<HTMLButtonElement>('.app>button')!;
 const before=box.getBoundingClientRect();
 pointer(handle,'pointerdown',200,300);pointer(handle,'pointermove',230,280);
 const moved=box.getBoundingClientRect();check(close(moved.x-before.x,30)&&close(moved.y-before.y,-20),`Handle did not translate crop: ${moved.x-before.x}, ${moved.y-before.y}`);
 check(close(moved.width,before.width)&&close(moved.height,before.height),'Pan resized crop');
 pointer(document,'pointerup',230,280,0);await wait();
 check(commits.length===1&&commits[0].centerX!>0&&commits[0].centerY!<0,'Pan not saved');
 check(close(commits[0].horizontalFov!,60),'Pan changed FOV');
 const count=commits.length;pointer(handle,'pointermove',400,400,0);pointer(handle,'pointermove',450,450,1);await wait();
 check(commits.length===count&&close(box.getBoundingClientRect().x,moved.x),'Hover continued pan');
 pointer(box,'pointerdown',200,300);pointer(box,'pointermove',190,310);pointer(document,'pointerup',190,310,0);await wait();
 check(commits.length===count+1,'Frame interior no longer draggable');
 pointer(handle,'pointerdown',200,300);pointer(handle,'pointermove',210,310);window.dispatchEvent(new Event('framely.pointerRelease'));await wait();
 check(!handle.hasAttribute('data-dragging'),'Native release left drag armed');
 const released=box.getBoundingClientRect();pointer(handle,'pointermove',500,500,1);await wait();
 check(close(box.getBoundingClientRect().x,released.x),'Native release did not stop pan');
 pointer(handle,'pointerdown',200,300);pointer(handle,'pointermove',220,320);pointer(document,'pointercancel',220,320,0);await wait();
 check(!handle.hasAttribute('data-dragging'),'Cancel left drag armed');
 console.log('FRAMELY_BRIDGE_PASS');
 }catch(e){console.error('FRAMELY_BRIDGE_FAIL '+e);}
};
createRoot(document.getElementById('root')!).render(<Fixture/>);
console.log('FRAMELY_VIEW_READY');
