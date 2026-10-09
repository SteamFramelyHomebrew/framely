// Exercise the actual notification views and quick menu in CEF, including saved messages and expiry.
import React,{useState} from 'react';
import {createRoot} from 'react-dom/client';
import {NotificationCenter,type NotificationEntry} from '../ui/src/notifications';
import {configureLanguage} from '../ui/src/i18n';
import '../ui/src/style.css';
import '../ui/src/notification-surfaces.css';
const root=createRoot(document.getElementById('root')!);
const wait=(ms=80)=>new Promise(r=>setTimeout(r,ms));
const plugin={manifest:{id:'capture.clip',name:'Clip capture',description:'Capture your session',version:'1.0.0',author:'Framely',ui:{windows:{}}},enabled:true,favorite:false,order:0};
const plugins:Record<string,typeof plugin>={'capture.clip':plugin,'capture.off':{...plugin,enabled:false,order:1,manifest:{...plugin.manifest,id:'capture.off',name:'Disabled capture'}}};
const entry=(id:string,extra:Partial<NotificationEntry>={}):NotificationEntry=>({plugin:'capture.clip',pluginName:'Clip capture',createdAt:Date.now(),expiresAt:Date.now()+8000,toast:true,inInbox:true,notification:{id,title:'录像已保存',body:'录像已经保存，可以打开文件或继续录制。',inbox:true,actions:[{id:'open',label:'打开录像'}]},...extra});
let entries=[entry('saved'),entry('transient',{inInbox:false,notification:{id:'transient',title:'临时消息',body:'只显示浮动通知'}}),entry('update',{plugin:'',pluginName:'Framely',update:{version:'0.4.3-preview.1'},notification:{id:'update',title:'Update',body:'New release',actions:[{id:'ignore',label:'Ignore'},{id:'open',label:'Open updates'}]}})];
let language='zh-CN',fail=false,calls:any[]=[],render:(inbox:boolean)=>void;
const status=()=>({version:'0.4.2-preview.12',agreement:{accepted:true,version:'fixture'},database:{plugins,sources:[],safeMode:false,language},notifications:entries.filter(n=>n.toast),inbox:entries.filter(n=>n.inInbox),notificationsCount:entries.length,running:[]});
window.fetch=async(_url,init)=>{
 const{method,params}=JSON.parse(String(init?.body));let result:any=true;
 if(method==='status')result=status();
 else if(method==='language.list')result={packs:[]};
 else if(method==='language.save'){language=params.language;calls.push({method,params});}
 else if(method==='host.manager.open'){calls.push({method,params});}
 else if(method==='ui.events')result={events:[],cursor:0};
 else if(method==='ui.visibility.get')result={known:false,pageVisible:false};
 else if(method==='notification.read'){
  for(const item of params.entries){const n=entries.find(n=>n.plugin===item.plugin&&n.notification.id===item.id&&n.createdAt===item.createdAt);if(n)n.read=true;}
 }
 else if(method.startsWith('notification.')){
  calls.push({method,params});
  const n=entries.find(n=>n.plugin===params.plugin&&n.notification.id===params.id);
  if(!n)throw Error('unknown notification');
  if(params.createdAt!==n.createdAt)throw Error('stale notification');
  if(fail)return new Response(JSON.stringify({error:'Fixture action failed'}));
  if(method==='notification.action'){
   const a=n.notification.actions!.find(a=>a.id===params.action)!;
   n.toast=a.closeOnClick===false?n.toast:false;n.inInbox=a.removeFromInboxOnClick===false?n.inInbox:false;
   if(n.update&&a.id==='open')result={openUpdates:true};
  }else if(method==='notification.dismiss')n.toast=false;
  else if(method==='notification.remove'){n.toast=false;n.inInbox=false;}
 }else if(!['host.haptic','host.keyboard','host.manager.open','host.menu.close'].includes(method))throw Error('unexpected API '+method);
 return new Response(JSON.stringify({result}));
};
function Fixture(){const[value,setValue]=useState(entries),[inbox,setInbox]=useState(true);render=inbox=>{setValue([...entries]);setInbox(inbox);};return <div className="app quick" style={{maxWidth:600,margin:'0 auto'}}><main className="content"><NotificationCenter items={value} inbox={inbox} plugins={plugins} refresh={async()=>setValue([...entries])}/></main></div>;}
function button(label:string){const b=document.querySelector(`button[aria-label="${label}"]`);if(!b)throw Error('missing button '+label);return b as HTMLButtonElement;}
async function until(check:()=>boolean){for(let i=0;i<60;i++){if(check())return;await wait();}throw Error('UI timeout');}
(window as any).runInstallReviewChecks=async()=>{try{
 configureLanguage('zh-CN',[]);render(true);await wait();
 if(document.querySelectorAll('.inbox-message').length!==2||document.body.textContent!.includes('临时消息'))throw Error('non-inbox notification leaked into inbox');
 if(!document.body.textContent!.includes('录像已保存')||!document.body.textContent!.includes('打开更新'))throw Error('Chinese notification content missing');
 await wait(150);console.log('FRAMELY_PREVIEW_INSTALL_INBOX_ZH');
 fail=true;button('打开录像').click();await until(()=>!!document.querySelector('[role=alert]'));
 if(!entries[0].inInbox||!entries[0].toast)throw Error('failed action removed message');
 fail=false;button('打开录像').click();await until(()=>document.querySelectorAll('.inbox-message').length===1);
 if(entries[0].inInbox||entries[0].toast)throw Error('default action did not remove message');
 entries=[entry('keep',{notification:{id:'keep',title:'录制进行中',body:'查看状态不会关闭这条通知。',actions:[{id:'open',label:'查看状态',closeOnClick:false,removeFromInboxOnClick:false}]} })];
 render(false);await wait();button('查看状态').click();await wait();
 if(!entries[0].toast||!entries[0].inInbox||!document.querySelector('.toast'))throw Error('opt-out action dismissed message');
 if(!document.querySelector('[role=timer]')?.textContent?.includes('秒后关闭'))throw Error('countdown not shown');
 button('关闭通知').click();await until(()=>!document.querySelector('.toast'));
 if(!entries[0].inInbox)throw Error('floating close removed saved message');
 render(true);await wait();button('移除通知').click();await until(()=>!!document.querySelector('.inbox-empty'));
 entries=[entry('expiry',{expiresAt:Date.now()+350})];render(false);await wait(700);
 if(document.querySelector('.toasts')||!entries[0].inInbox)throw Error('automatic close left an empty popup or removed inbox copy');
 entries=[entry('persistent',{expiresAt:null,notification:{id:'persistent',title:'Persistent status',body:'Stays visible until dismissed',durationMs:0}})];render(false);await wait();
 if(!document.querySelector('.toast')||document.querySelector('[role=timer]'))throw Error('persistent notification has a countdown');
 entries=[entry('long',{notification:{id:'long',title:'A recording with a long title is ready to review',body:'Recording details. '.repeat(80),actions:[{id:'open',label:'Open recording'},{id:'copy',label:'Copy path'},{id:'continue',label:'Continue recording'}]}})];render(false);await wait();
 const compact=document.querySelector('.toast') as HTMLElement,message=compact.querySelector('.notification-message') as HTMLElement;
 if(compact.getBoundingClientRect().height>233||message.scrollHeight<=message.clientHeight)throw Error('long notification expanded instead of scrolling inside compact popup');
 for(const action of compact.querySelectorAll('.toast-actions button')){const bounds=action.getBoundingClientRect();if(bounds.height<44||bounds.bottom>compact.getBoundingClientRect().bottom)throw Error('compact notification action is clipped or too small');}
 entries=[entry('saved',{notification:{id:'saved',title:'Recording saved',body:'Your recording is ready. Open it whenever you are ready.',actions:[{id:'open',label:'Open recording'}]}}),entry('update',{plugin:'',pluginName:'Framely',update:{version:'0.4.3-preview.1'},notification:{id:'update',title:'Update',body:'New release',actions:[{id:'ignore',label:'Ignore'},{id:'open',label:'Open updates'}]}})];
 configureLanguage('en-US',[]);render(true);await wait(180);
 if(!document.body.textContent!.includes('Inbox')||!document.body.textContent!.includes('Open updates'))throw Error('English translation missing');
 console.log('FRAMELY_PREVIEW_INSTALL_INBOX_EN');
 render(false);await wait(180);console.log('FRAMELY_PREVIEW_INSTALL_TOAST_EN');
 if([...document.querySelectorAll('.toast')].some(card=>card.getBoundingClientRect().height>233))throw Error('floating notification exceeds compact height');
 if([...document.querySelectorAll('.toast-actions button')].some(b=>b.scrollWidth>b.clientWidth))throw Error('notification action overflowed');
 // Mount the real quick menu and assert the navigation actually reaches the inbox.
 root.unmount();await import('../ui/src/main');
 await until(()=>!!document.querySelector('nav button'));
 const installed=[...document.querySelectorAll('nav button')].find(b=>b.textContent==='已安装') as HTMLButtonElement;
 installed.click();await wait();
 if(document.querySelectorAll('.plugin-row').length!==1)throw Error('quick installed list includes disabled plugins');
 const notifications=[...document.querySelectorAll('nav button')].find(b=>b.textContent?.startsWith('通知')) as HTMLButtonElement;
 if(!notifications)throw Error('quick inbox tab missing');notifications.click();await wait(180);
 if(document.querySelectorAll('.inbox-message').length!==2||document.querySelectorAll('nav button').length!==5)throw Error('quick inbox navigation broken');
 if(!document.querySelector('[role=switch][aria-label="允许通知"]')||!document.querySelector('[aria-label="通知设置"]'))throw Error('quick notification controls missing');
 console.log('FRAMELY_PREVIEW_INSTALL_QUICK_INBOX');
 button('打开更新').click();await until(()=>!document.querySelector('.inbox-message')?.textContent?.includes('可更新'));
 if(!calls.some(c=>c.method==='notification.action'&&c.params.action==='open'&&c.params.plugin===''))throw Error('update action not dispatched');
 const settings=[...document.querySelectorAll('nav button')].find(b=>b.textContent==='设置') as HTMLButtonElement;
 settings.click();await until(()=>!!document.querySelector('.quick-settings'));
 if(document.querySelector('.quick-settings input[type=file]')||document.querySelector('.quick-settings [download]'))throw Error('Quick settings exposes language installation or template download');
 const safe=document.querySelector('[role=switch][aria-label="安全模式"]') as HTMLElement,languageCard=document.querySelector('.language-settings') as HTMLElement;
 if(languageCard.getBoundingClientRect().top-safe.getBoundingClientRect().bottom<24)throw Error('Safe mode is too close to language divider');
 button('界面语言').click();await wait();
 const english=[...document.querySelectorAll('[role=option]')].find(b=>b.textContent==='English') as HTMLButtonElement;
 if(!english)throw Error('Language picker has no English option');english.click();
 await until(()=>!!document.querySelector('[aria-label="Interface language"]'));
 if(!calls.some(c=>c.method==='language.save'&&c.params.language==='en-US'))throw Error('Quick language selection did not save');
 const update=document.querySelector('.quick-update-entry') as HTMLButtonElement;
 if(!update?.textContent?.includes('Check for updates'))throw Error('Quick update entry missing');update.click();await wait();
 if(!calls.some(c=>c.method==='host.manager.open'&&c.params.page==='updates'))throw Error('Quick update entry did not open update settings');
 console.log('FRAMELY_PREVIEW_INSTALL_QUICK_SETTINGS_EN');
 console.log('FRAMELY_BRIDGE_PASS');
}catch(e){console.error('FRAMELY_BRIDGE_FAIL '+e);}};
root.render(<Fixture/>);wait(150).then(()=>console.log('FRAMELY_VIEW_READY'));
