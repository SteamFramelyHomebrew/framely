import React,{useState} from 'react';
import {createRoot} from 'react-dom/client';
import {CastQuick} from '../ui/src/cast-quick';
import {Casting,type CastSettings} from '../ui/src/casting';
import {configureLanguage} from '../ui/src/i18n';
import '../ui/src/style.css';

let config:CastSettings={source:'screen',codec:'h264',output:'eye',eye:'right',width:1920,height:1440,fps:30,bitrateMbps:8,horizontalFov:null,centerX:0,centerY:0,systemAudio:true,microphone:false,airplay:false,dlna:false,receiverName:'Framely'};
let recording={active:true,id:'same-recording',elapsedSeconds:75},fail=true;
const calls:string[]=[];
window.fetch=async(_url,init)=>{
 const {method,params}=JSON.parse(String(init?.body));calls.push(method);
 if(method==='cast.recordingIndicator.visible.save'){
  if(fail)return new Response(JSON.stringify({error:'Simulated save failure'}));
  config={...config,recordingIndicatorVisible:params.enabled};
 }
 if(method==='cast.record.stop')recording={...recording,active:false};
 return new Response(JSON.stringify({result:method==='cast.status'?{running:false,recording,dominantEye:1,watchPath:'/cast/watch'}:true}));
};
const root=createRoot(document.getElementById('root')!);
function Preview({manager}:{manager:boolean}){const [value,setValue]=useState(config);const refresh=async()=>{setValue({...config});};return <div className="app"><main className="content">{manager?<Casting config={value} refresh={refresh} network={{enabled:true,port:15915,passwordEnabled:false}}/>:<CastQuick config={value} refresh={refresh}/>}</main></div>;}
const wait=()=>new Promise(r=>setTimeout(r,40));
const until=async(check:()=>unknown)=>{for(let i=0;i<100;i++){if(check())return;await wait();}throw Error('UI timeout '+JSON.stringify({config,recording,calls,checked:toggle()?.getAttribute('aria-checked')})+' '+document.body.innerText);};
const toggle=()=>document.querySelector<HTMLButtonElement>('[role=switch][aria-label="显示录制指示器"]')!;
const stop=()=>Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(b=>b.textContent?.includes('停止录制'))!;
(async()=>{try{
 configureLanguage('zh-CN',[]);root.render(<Preview manager={false}/>);
 await until(()=>toggle()&&!toggle().disabled&&stop());
 if(toggle().getAttribute('aria-checked')!=='true')throw Error('Legacy default is not visible');
 toggle().click();await until(()=>document.body.textContent?.includes('Simulated save failure')&&!toggle().disabled);
 if(toggle().getAttribute('aria-checked')!=='true')throw Error('Failed save changed switch');
 fail=false;toggle().click();await until(()=>toggle().getAttribute('aria-checked')==='false'&&!toggle().disabled);
 if(recording.id!=='same-recording'||recording.elapsedSeconds!==75||!recording.active||stop().disabled)throw Error('Visibility changed the recording');
 stop().click();await until(()=>!recording.active&&!toggle().disabled&&!stop());
 recording={...recording,active:true};root.render(<Preview key="manager" manager/>);
 await until(()=>document.querySelector('.cast-page-heading')&&toggle()&&toggle().getAttribute('aria-checked')==='false'&&!toggle().disabled&&stop());
 toggle().click();await until(()=>toggle().getAttribute('aria-checked')==='true'&&!toggle().disabled);
 if(calls.some(m=>['cast.settings.save','cast.start','cast.stop','cast.record.start'].includes(m)))throw Error('Visibility restarted capture');
 stop().click();await until(()=>!recording.active);
 console.log('FRAMELY_BRIDGE_PASS');
}catch(error){console.error('FRAMELY_BRIDGE_FAIL '+error);}})();
