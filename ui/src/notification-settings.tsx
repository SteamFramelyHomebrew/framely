import React,{useState,useEffect} from 'react';
import {IconBell,IconSettings,IconSearch} from '@tabler/icons-react';
import {api,viewKey} from './api';
import {t} from './i18n';
import {PluginImage,installedIcon} from './store';
import type {NotificationEntry} from './notifications';

export type NotificationPreference={popups:boolean;badge:boolean};
export type NotificationConfig=NotificationPreference&{plugins:Record<string,NotificationPreference>};
export const defaultNotificationConfig:NotificationConfig={popups:true,badge:true,plugins:{}};
type Installed={manifest:{id:string;name:string;description?:string;icon?:string|null;version:string};enabled:boolean;order:number};
function Switch({checked,label,disabled,onChange}:{checked:boolean;label:string;disabled:boolean;onChange:()=>void}){
 return <button className={`notification-switch ${checked?'on':''}`} role="switch" aria-checked={checked} aria-label={label} disabled={disabled} onClick={onChange}><span/></button>;
}
function useSave(refresh:()=>Promise<void>){
 const[busy,setBusy]=useState(false),[error,setError]=useState('');
 async function run(task:()=>Promise<unknown>){if(busy)return;setBusy(true);setError('');try{await task();await refresh();}catch(e){setError(String(e));}finally{setBusy(false);}}
 const save=(patch:Partial<NotificationPreference>&{plugin?:string})=>run(()=>api('notification.settings.save',patch));
 const open=()=>run(()=>api('host.manager.open',{page:'notification-settings'}));
 return {busy,error,save,open};
}
export function PopupPermission({config,refresh}:{config:NotificationConfig;refresh:()=>Promise<void>}){
 const{busy,error,save,open}=useSave(refresh);
 return <div className="inbox-permission"><div className="notification-option"><IconBell size={23} stroke={1.8}/><div><b>{t('允许通知')}</b><p>{config.popups?t('允许在视野中显示悬浮通知。'):t('悬浮已关闭，新通知仅在此处查看。')}</p></div><Switch checked={config.popups} label={t('允许通知')} disabled={busy} onChange={()=>void save({popups:!config.popups})}/><button className="notification-settings-link" aria-label={t('通知设置')} disabled={busy} onClick={()=>void open()}><IconSettings size={23} stroke={1.8}/></button></div>{error&&<p className="error" role="alert">{error}</p>}</div>;
}
export function NotificationSettings({config,plugins,refresh}:{config:NotificationConfig;plugins:Record<string,Installed>;refresh:()=>Promise<void>}){
 const{busy,error,save}=useSave(refresh),[query,setQuery]=useState('');
 const installed:Installed[]=[{manifest:{id:'',name:'Framely',description:t('本体通知，包括检查更新提醒。'),version:''},enabled:true,order:-1},...Object.values(plugins).sort((a,b)=>a.order-b.order)],filtered=installed.filter(p=>`${p.manifest.name} ${p.manifest.id} ${p.manifest.description??''}`.toLowerCase().includes(query.trim().toLowerCase()));
 return <section className="notification-settings">
  <div className="list-heading"><div><h1>{t('通知设置')}</h1><p>{t('选择通知如何提醒你。')}</p></div></div>
  <div className="notification-global-options">
   <div className="notification-option"><div><b>{t('悬浮通知')}</b><p>{t('关闭后，新通知将保存在通知收件箱，不显示浮窗。')}</p></div><Switch checked={config.popups} label={t('悬浮通知')} disabled={busy} onChange={()=>void save({popups:!config.popups})}/></div>
   <div className="notification-option"><div><b>{t('启动图标角标')}</b><p>{t('在 Framely 启动图标上显示未读收件箱消息数。')}</p></div><Switch checked={config.badge} label={t('启动图标角标')} disabled={busy} onChange={()=>void save({badge:!config.badge})}/></div>
  </div>
  {error&&<p className="error" role="alert">{error}</p>}
  <div className="notification-plugin-heading"><h2>{t('应用通知')}</h2><div className="search-field"><IconSearch size={22} stroke={1.8}/><input aria-label={t('搜索通知应用')} placeholder={t('搜索应用')} value={query} onChange={e=>setQuery(e.target.value)}/></div></div>
  <p className="notification-settings-help">{t('全局开关优先；关闭全局开关不会改变应用偏好。')}</p>
  <div className="notification-plugin-columns"><span>{t('应用')}</span><span>{t('角标')}</span><span>{t('悬浮')}</span></div>
  <div className="notification-plugin-list">{filtered.map(plugin=>{
   const preference=config.plugins[plugin.manifest.id]??{popups:true,badge:true};
   return <article className="notification-plugin-row" key={plugin.manifest.id}><div className="notification-plugin-name"><PluginImage src={plugin.manifest.id?installedIcon(plugin):'/assets/branding/framely-app-icon.svg'} name={plugin.manifest.name} size={40}/><div><b>{plugin.manifest.name}</b><small>{!plugin.manifest.id?plugin.manifest.description:plugin.enabled?plugin.manifest.id:t('已停用')}</small></div></div><Switch checked={preference.badge} label={t('{0}：角标',{0:plugin.manifest.name})} disabled={busy} onChange={()=>void save({plugin:plugin.manifest.id,badge:!preference.badge})}/><Switch checked={preference.popups} label={t('{0}：悬浮',{0:plugin.manifest.name})} disabled={busy} onChange={()=>void save({plugin:plugin.manifest.id,popups:!preference.popups})}/></article>;
  })}</div>
  {!filtered.length&&<p className="empty">{t('未找到应用')}</p>}
 </section>;
}

// A hidden native quick panel must not silently clear the Dock badge.
export function useReadInbox(enabled:boolean,items:NotificationEntry[],refresh:()=>Promise<void>){
 const[error,setError]=useState('');
 const unread=items.filter(n=>n.inInbox&&!n.read).map(n=>({plugin:n.plugin,id:n.notification.id,createdAt:n.createdAt}));
 const snapshot=JSON.stringify(unread);
 useEffect(()=>{
  if(!enabled||!unread.length)return;
  let live=true,busy=false;
  async function read(){
   if(!live||busy||document.visibilityState==='hidden')return;busy=true;
   try{
    if(['localhost','127.0.0.1','[::1]'].includes(location.hostname)){
     const visibility=await api('ui.visibility.get',{view:viewKey()});
     if(visibility.known&&!visibility.pageVisible)return;
    }
    if(!live)return;
    await api('notification.read',{entries:JSON.parse(snapshot)});
    if(live){setError('');await refresh();}
   }catch(e){if(live)setError(String(e));}finally{busy=false;}
  }
  void read();const timer=setInterval(()=>void read(),1000);
  return()=>{live=false;clearInterval(timer);};
 },[enabled,snapshot]);
 return error;
}
