// Exercise the real notification controls, including failures and hidden headset views.
import React,{useState} from 'react';
import {createRoot} from 'react-dom/client';
import {NotificationSettings,PopupPermission,useReadInbox,type NotificationConfig} from '../ui/src/notification-settings';
import {NotificationCenter,type NotificationEntry} from '../ui/src/notifications';
import {configureLanguage} from '../ui/src/i18n';
import '../ui/src/style.css';
const root=createRoot(document.getElementById('root')!);
const wait=(ms=80)=>new Promise(r=>setTimeout(r,ms));
const plugins={
 'capture.clip':{manifest:{id:'capture.clip',name:'Clip capture',description:'Capture your session',version:'1.0.0'},enabled:true,order:0},
 'capture.off':{manifest:{id:'capture.off',name:'Frame diagnostics',description:'Device diagnostics',version:'1.0.0'},enabled:false,order:1},
};
let config:NotificationConfig={popups:true,badge:true,plugins:{}},fail=false,visible=false,readCalls=0;
const messages:NotificationEntry[]=[{plugin:'capture.clip',pluginName:'Clip capture',createdAt:Date.now(),expiresAt:null,toast:false,inInbox:true,notification:{id:'saved',title:'录像已保存',body:'录像已经保存，可以稍后打开。',inbox:true}}];
const calls:any[]=[];
window.fetch=async(_url,init)=>{
 const{method,params}=JSON.parse(String(init?.body));calls.push({method,params});let result:any=true;
 if(method==='status')result={version:'0.4.2-preview.12',agreement:{accepted:true,version:'fixture'},database:{plugins,sources:[],safeMode:false,language:'en-US',notificationSettings:config},notifications:[],inbox:messages,notificationBadge:0,running:[]};
 else if(method==='language.list')result={packs:[]};
 else if(method==='ui.events')result={events:[],cursor:0};
 else if(method==='notification.settings.save'){
  if(fail)return new Response(JSON.stringify({error:'Fixture settings failed'}));
  config={...config,plugins:{...config.plugins}};
  const preference=typeof params.plugin==='string'?(config.plugins[params.plugin]={...(config.plugins[params.plugin]??{popups:true,badge:true})}):config;
  for(const key of ['popups','badge'] as const)if(typeof params[key]==='boolean')preference[key]=params[key];
 }else if(method==='ui.visibility.get')result={known:true,pageVisible:visible};
 else if(method==='notification.read'){
  readCalls++;
  for(const item of params.entries){const n=messages.find(n=>n.plugin===item.plugin&&n.notification.id===item.id&&n.createdAt===item.createdAt);if(n)n.read=true;}
 }else if(!['host.manager.open','host.haptic'].includes(method))throw Error('Unexpected API '+method);
 return new Response(JSON.stringify({result}));
};
let switchMode:(value:'settings'|'quick')=>void,refresh:()=>Promise<void>;
function Fixture(){
 const[mode,setMode]=useState<'settings'|'quick'>('settings'),[,update]=useState(0);
 switchMode=setMode;refresh=async()=>update(v=>v+1);
 useReadInbox(mode==='quick',messages,refresh);
 return <div className={`app ${mode==='settings'?'manager':'quick'}`}><main className="content" style={mode==='quick'?{maxWidth:600,margin:'0 auto',width:'100%'}:undefined}>{mode==='settings'?<NotificationSettings config={config} plugins={plugins} refresh={refresh}/>:<NotificationCenter inbox items={messages} plugins={plugins} refresh={refresh} controls={<PopupPermission config={config} refresh={refresh}/>}/>}</main></div>;
}
function control(label:string){const node=document.querySelector(`[role=switch][aria-label="${label}"]`);if(!node)throw Error('Missing switch '+label);return node as HTMLButtonElement;}
async function until(check:()=>boolean){for(let i=0;i<60;i++){if(check())return;await wait();}throw Error('UI timeout');}
(window as any).runInstallReviewChecks=async()=>{try{
 configureLanguage('zh-CN',[]);await refresh();await wait(150);
 if(document.querySelectorAll('.notification-plugin-row').length!==3||document.querySelectorAll('[role=switch]').length!==8)throw Error('Installed plugins or notification controls missing');
 if(!document.body.textContent!.includes('已停用'))throw Error('Disabled plugin omitted');
 if(!document.body.textContent!.includes('本体通知，包括检查更新提醒。'))throw Error('Framely notification source missing');
 const globals=document.querySelector('.notification-global-options') as HTMLElement,list=document.querySelector('.notification-plugin-list') as HTMLElement;
 if(Math.abs(globals.getBoundingClientRect().width-list.getBoundingClientRect().width)>1)throw Error('Global options and app list widths differ');
 const globalSwitch=control('悬浮通知').getBoundingClientRect(),popupSwitch=control('Framely：悬浮').getBoundingClientRect();
 if(Math.abs(globalSwitch.left-popupSwitch.left)>1)throw Error('Global and app popup switch columns are misaligned');
 control('Framely：悬浮').click();await until(()=>control('Framely：悬浮').getAttribute('aria-checked')==='false');
 control('Framely：角标').click();await until(()=>control('Framely：角标').getAttribute('aria-checked')==='false');
 if(!config.popups||!config.badge||config.plugins[''].popups||config.plugins[''].badge)throw Error('Core preferences changed global policy');
 if(!calls.some(c=>c.method==='notification.settings.save'&&c.params.plugin===''&&c.params.popups===false))throw Error('Framely popup preference was saved as global');
 console.log('FRAMELY_PREVIEW_INSTALL_NOTIFICATION_SETTINGS_ZH');
 fail=true;control('悬浮通知').click();await until(()=>!!document.querySelector('[role=alert]'));
 if(!config.popups||control('悬浮通知').getAttribute('aria-checked')!=='true')throw Error('Failed save changed switch');
 fail=false;control('悬浮通知').click();await until(()=>control('悬浮通知').getAttribute('aria-checked')==='false');
 if(!config.badge)throw Error('Popup switch changed badge policy');
 control('Clip capture：角标').click();await until(()=>control('Clip capture：角标').getAttribute('aria-checked')==='false');
 if(config.plugins['capture.clip'].popups!==true)throw Error('Plugin badge switch changed popup policy');
 control('Frame diagnostics：悬浮').click();await until(()=>control('Frame diagnostics：悬浮').getAttribute('aria-checked')==='false');
 control('启动图标角标').click();await until(()=>control('启动图标角标').getAttribute('aria-checked')==='false');
 if(config.plugins['capture.clip'].badge!==false||config.plugins['capture.off'].popups!==false)throw Error('Global switch erased plugin preferences');
 const search=document.querySelector('[aria-label="搜索通知应用"]') as HTMLInputElement;
 Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')!.set!.call(search,'Frame diagnostics');search.dispatchEvent(new Event('input',{bubbles:true}));await wait();
 if(document.querySelectorAll('.notification-plugin-row').length!==1)throw Error('Plugin search failed');
 Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')!.set!.call(search,'');search.dispatchEvent(new Event('input',{bubbles:true}));await wait();
 configureLanguage('en-US',[]);await refresh();await wait(180);
 if(!document.body.textContent!.includes('Launcher badge')||!document.body.textContent!.includes('App notifications'))throw Error('English settings copy missing');
 console.log('FRAMELY_PREVIEW_INSTALL_NOTIFICATION_SETTINGS_EN');
 for(const row of document.querySelectorAll('.notification-plugin-row'))if(row.scrollWidth>row.clientWidth)throw Error('Plugin switches overflow the settings row');
 switchMode('quick');configureLanguage('zh-CN',[]);await wait(180);
 if(control('允许通知').getAttribute('aria-checked')!=='false'||!document.body.textContent!.includes('悬浮已关闭'))throw Error('Quick popup switch out of sync');
 if(readCalls)throw Error('Hidden headset view marked notifications as read');
 console.log('FRAMELY_PREVIEW_INSTALL_NOTIFICATION_PERMISSION_ZH');
 control('允许通知').click();await until(()=>control('允许通知').getAttribute('aria-checked')==='true');
 if(!config.popups||config.badge)throw Error('Quick switch changed unrelated setting');
 (document.querySelector('[aria-label="通知设置"]') as HTMLButtonElement).click();await wait();
 if(!calls.some(c=>c.method==='host.manager.open'&&c.params.page==='notification-settings'))throw Error('Notification settings entry target missing');
 visible=true;await until(()=>readCalls>0);
 if(!messages[0].read||!document.querySelector('.inbox-message'))throw Error('Reading inbox removed message');
 root.unmount();history.replaceState(null,'','/manager#notification-settings');await import('../ui/src/main');
 await until(()=>!!document.querySelector('.notification-settings'));
 const nav=[...document.querySelectorAll('nav button')].find(b=>b.textContent==='Notifications');
 if(!nav||nav.getAttribute('aria-current')!=='page')throw Error('Manager notification settings navigation missing');
 if(document.querySelectorAll('.notification-plugin-row').length!==3)throw Error('Manager notification settings omitted notification source');
 await wait(180);console.log('FRAMELY_PREVIEW_INSTALL_NOTIFICATION_MANAGER_EN');
 console.log('FRAMELY_BRIDGE_PASS');
}catch(e){console.error('FRAMELY_BRIDGE_FAIL '+e);}};
root.render(<Fixture/>);wait(150).then(()=>console.log('FRAMELY_VIEW_READY'));
