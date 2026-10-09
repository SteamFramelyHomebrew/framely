import React,{useEffect,useState} from 'react';
import {IconBell,IconInbox,IconX,IconTrash,IconArrowUpRight,IconCast} from '@tabler/icons-react';
import {api} from './api';
import {t,currentLanguage} from './i18n';
import {Brand} from './icons';
import type {Notification} from '@framely/sdk';
export type NotificationEntry={plugin:string;pluginName:string;createdAt:number;expiresAt:number|null;toast:boolean;inInbox:boolean;read?:boolean;notification:Notification;update?:{version:string}|null};
function copy(entry:NotificationEntry){return entry.update?{title:t('Framely {0} 可更新',{0:entry.update.version}),body:t('查看新版本并选择安装，或忽略本次提醒。')}:entry.notification;}
function NotificationCard({entry,inbox,now,refresh,available}:{entry:NotificationEntry;inbox:boolean;now:number;refresh:()=>Promise<void>;available:boolean}){
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[imageFailed,setImageFailed]=useState(false);
 const message=copy(entry),remaining=entry.expiresAt===null?null:Math.max(0,Math.ceil(Math.min(entry.notification.durationMs??8000,entry.expiresAt-now)/1000));
 useEffect(()=>setImageFailed(false),[entry.notification.image]);
 async function perform(method:string,action?:string){
  if(busy)return;setBusy(true);setError('');
  try{
   const result=await api(method,{plugin:entry.plugin,id:entry.notification.id,createdAt:entry.createdAt,...(action?{action}:{})});
   await refresh();
   if(entry.update&&result?.openUpdates&&location.pathname!=='/notifications'){
    if(location.pathname==='/manager'){location.hash='updates';window.dispatchEvent(new Event('hashchange'));}
    else if(!['localhost','127.0.0.1','[::1]'].includes(location.hostname))location.assign('/manager#updates');
   }
  }catch(e){setError(String(e));}finally{setBusy(false);}
 }
 return <article className={`notification-card ${inbox?'inbox-message':'toast'}`}>
  <header className="notification-heading">
   <span className="notification-source-icon">{entry.notification.id.startsWith('cast.')?<IconCast size={22} stroke={1.8}/>:entry.update?<Brand/>:<IconBell size={22} stroke={1.8}/>}</span>
   <div className="notification-origin"><b>{entry.pluginName}</b>{inbox?<time dateTime={new Date(entry.createdAt).toISOString()}>{new Intl.DateTimeFormat(currentLanguage(),{month:'short',day:'numeric',hour:'2-digit',minute:'2-digit'}).format(entry.createdAt)}</time>:remaining!==null&&<span className="notification-countdown" role="timer">{t('{0} 秒后关闭',{0:remaining})}</span>}</div>
   <button className="notification-dismiss" disabled={busy} aria-label={inbox?t('移除通知'):t('关闭通知')} onClick={()=>void perform(inbox?'notification.remove':'notification.dismiss')}>{inbox?<IconTrash size={21} stroke={1.8}/>:<IconX size={23} stroke={1.8}/>}</button>
  </header>
  <div className="notification-message"><h2>{message.title}</h2><p>{message.body}</p>{entry.notification.image&&!imageFailed&&<img className="notification-image" src={entry.notification.image} alt={t('通知图片')} onError={()=>setImageFailed(true)} onLoad={e=>{if(e.currentTarget.naturalWidth<=1&&e.currentTarget.naturalHeight<=1)setImageFailed(true);}}/>}</div>
  {!available&&<p className="notification-unavailable">{t('启用此插件后可使用通知操作。')}</p>}
  {!!entry.notification.actions?.length&&<footer className="toast-actions">{entry.notification.actions.map(a=>{
   const label=entry.update?(a.id==='ignore'?t('忽略'):t('打开更新')):a.label;
   return <button key={a.id} className={a.id==='open'?'notification-primary':''} aria-label={label} title={label} disabled={busy||!available} onClick={()=>void perform('notification.action',a.id)}><span className="notification-action-label">{label}</span>{entry.update&&a.id==='open'&&<IconArrowUpRight size={19} stroke={1.8}/>}</button>;
  })}</footer>}
  {error&&<p className="error" role="alert">{error}</p>}
 </article>;
}
export function NotificationCenter({items,inbox=false,refresh,plugins={},safeMode=false,controls}:{items:NotificationEntry[];inbox?:boolean;refresh:()=>Promise<void>;plugins?:Record<string,{enabled:boolean}>;safeMode?:boolean;controls?:React.ReactNode}){
 const[now,setNow]=useState(Date.now());
 useEffect(()=>{if(inbox)return;const timer=setInterval(()=>setNow(Date.now()),250);return()=>clearInterval(timer);},[inbox]);
 const visible=items.filter(n=>inbox?n.inInbox:n.toast&&(n.expiresAt===null||n.expiresAt>now)).sort((a,b)=>b.createdAt-a.createdAt);
 if(!inbox&&!visible.length)return null;
 return <section className={inbox?'notification-inbox':'toasts'} aria-label={t('通知')}>
  {inbox&&<header className="inbox-heading"><div><h1>{t('通知')}</h1><p>{t('收件箱')}<span>{visible.length}</span></p></div><IconInbox size={30} stroke={1.8}/></header>}
  {inbox&&controls}
  {visible.map(entry=><NotificationCard key={`${entry.plugin}:${entry.notification.id}:${entry.createdAt}`} entry={entry} inbox={inbox} now={now} refresh={refresh} available={entry.plugin===''||!!entry.update||!!plugins[entry.plugin]?.enabled&&!safeMode}/>)}
  {inbox&&!visible.length&&<div className="inbox-empty"><IconInbox size={44} stroke={1.5}/><h2>{t('收件箱为空')}</h2><p>{t('需要保留的通知会出现在这里。')}</p></div>}
 </section>;
}
