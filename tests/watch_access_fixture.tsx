import React from 'react';
import {createRoot} from 'react-dom/client';
import {Casting,type CastSettings} from '../ui/src/casting';
import {configureLanguage} from '../ui/src/i18n';
import '../ui/src/style.css';

const config:CastSettings={source:'screen',codec:'h264',output:'eye',eye:'right',width:1920,height:1440,fps:30,bitrateMbps:8,horizontalFov:null,centerX:0,centerY:0,systemAudio:true,microphone:false,airplay:false,dlna:false,receiverName:'Framely'};
let running=false,enabled=false,fail=false,loaded=false,release:(()=>void)|undefined,copied='',phase='loading';
const calls:string[]=[];
const status=()=>({running,preview:running,error:null,watchAccessEnabled:enabled,watchPath:enabled?'/cast/watch?token=test-token':'/cast/watch',watchAddresses:[`http://192.168.5.67:15915/cast/watch${enabled?'?token=test-token':''}`]});
Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText:async(value:string)=>{copied=value;}}});
window.fetch=async(_input,init)=>{
 const {method,params={}}=JSON.parse(String(init?.body));calls.push(method);
 if(method==='cast.status'&&!loaded)await new Promise<void>(resolve=>{release=resolve;});
 if(method==='cast.watchAccess.save'){
  await new Promise(r=>setTimeout(r,250));
  if(fail)return new Response(JSON.stringify({error:'Simulated watch-access save failure'}));
  enabled=params.enabled;
 }
 return new Response(JSON.stringify({result:method==='cast.status'?status():true}));
};
const wait=()=>new Promise(r=>setTimeout(r,25));
const toggle=()=>document.querySelector<HTMLButtonElement>('[role=switch][aria-label="免验证"]');
const until=async(check:()=>unknown)=>{for(let i=0;i<200;i++){if(check())return;await wait();}throw Error('UI timeout at '+phase+': '+JSON.stringify({running,enabled,fail,copied,calls})+' '+document.body.innerText);};
(async()=>{try{
 configureLanguage('zh-CN',[]);
 createRoot(document.getElementById('root')!).render(<div className="app"><main className="content"><Casting config={config} refresh={async()=>{}} network={{enabled:false,port:15915,passwordEnabled:false}}/></main></div>);
 await until(()=>toggle()&&release);
 if(!toggle()!.disabled||document.querySelector('.cast-share-link'))throw Error('Loading state exposes an actionable or incorrect link');
 loaded=true;release!();
 await until(()=>toggle()&&!toggle()!.disabled&&document.querySelector('.cast-share-link'));
 if(running||toggle()!.getAttribute('aria-checked')!=='false')throw Error('Stopped stream must expose switch');
 phase='enable';toggle()!.click();await until(()=>toggle()!.disabled);
 await until(()=>toggle()!.getAttribute('aria-checked')==='true'&&!toggle()!.disabled);
 const link=()=>document.querySelector<HTMLAnchorElement>('.cast-share-link a')!;
 const shown=()=>document.querySelector('.cast-share-link code')!.textContent;
 if(shown()!==link().href||!shown()!.endsWith('?token=test-token'))throw Error('Displayed/open links differ');
 phase='copy';document.querySelector<HTMLButtonElement>('[aria-label="复制观看地址"]')!.click();
 await wait();
 await until(()=>copied===shown()&&!toggle()!.disabled&&document.querySelector('[aria-label="复制观看地址"]')?.textContent?.includes('已复制'));
 phase='failed-save';fail=true;toggle()!.click();
 await until(()=>document.body.textContent?.includes('Simulated watch-access save failure')&&!toggle()!.disabled);
 if(toggle()!.getAttribute('aria-checked')!=='true'||shown()!==copied)throw Error('Failed save changed access state');
 phase='running';fail=false;running=true;
 await until(()=>document.querySelector('.cast-status.live'));
 if(!toggle()||toggle()!.getAttribute('aria-checked')!=='true')throw Error('Running state hid switch');
 phase='stopped';running=false;await until(()=>!document.querySelector('.cast-status.live'));
 if(!toggle()||toggle()!.disabled)throw Error('Stopping stream hid or disabled switch');
 phase='disable';toggle()!.click();await until(()=>toggle()!.getAttribute('aria-checked')==='false'&&!toggle()!.disabled);
 if(shown()!.includes('token=')||calls.some(c=>['cast.start','cast.stop','cast.settings.save'].includes(c)))throw Error('Access setting changed capture or retained old token');
 console.log('FRAMELY_BRIDGE_PASS');
}catch(error){console.error('FRAMELY_BRIDGE_FAIL '+error);}})();
