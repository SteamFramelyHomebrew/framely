import React,{useEffect,useRef,useState} from 'react';
import {api,viewKey} from './api';
import {t,currentLanguage} from './i18n';
export let pluginLanguagePreference='auto';
export function setPluginLanguagePreference(value:string){pluginLanguagePreference=value;}
export function PluginFrame({plugin,entry='quick',actionContext,onActionDone}:{plugin:string;entry?:string;actionContext?:any;onActionDone?:(error?:string)=>void}){
 const ref=useRef<HTMLIFrameElement>(null);
 const [error,setError]=useState(''),[source,setSource]=useState<string|null>(null),[sandbox,setSandbox]=useState('allow-scripts');
 const [attempt,setAttempt]=useState(0);
 useEffect(()=>{
  let live=true;setError('');setSource(null);
  const methods:Record<string,string>={call:'plugin.call',dependencies:'plugin.dependencies','window.open':'window.open','window.close':'window.close','notification.send':'notification.send','notification.remove':'notification.remove','notification.dismiss':'notification.dismiss'};
  async function message(e:MessageEvent){
   if(e.source!==ref.current?.contentWindow||e.data?.channel!=='framely.plugin')return;
   const {id,op,params}=e.data;if(!Number.isSafeInteger(id)||typeof op!=='string')return;
   try{let result;
    if(op==='launcher.ready'){if(!entry.startsWith('action-')||!actionContext)throw new Error('Not an action host');ref.current?.contentWindow?.postMessage({channel:'framely.reply',event:{type:'launcher.action',data:actionContext}},'*');result=true;}
    else if(op==='launcher.done'){if(!entry.startsWith('action-')||!actionContext)throw new Error('Not an action host');onActionDone?.(params?.error);result=true;}
    else if(op==='ui.launchContext')result=actionContext??await api('plugin.launch.context',{plugin,entry});
    else if(op==='language.get')result={preference:pluginLanguagePreference,language:currentLanguage()};
    else if(op==='ui.close'){
     const view=viewKey();
     if(view==='menu')result=await api('host.menu.close');
     else if(view==='launcher')result=await api('host.launcher.close');
     else if(view==='framely.manager')result=await api('host.manager.close');
     else result=await api('window.close',{plugin,window:entry});
    }
    else if(op==='ui.visibility.get')result=await api('ui.visibility.get',{view:viewKey()});
    else if(op==='haptic')result=await api('host.haptic',{view:viewKey()});
    else if(op==='keyboard')result=await api('host.keyboard',{...params,view:viewKey()});
    else{const method=methods[op];if(!method)throw new Error(t('插件请求不支持的能力'));result=await api(method,{...params,plugin,...(op==='window.open'&&actionContext?{launchContext:actionContext}:{})});}
    if(live)ref.current?.contentWindow?.postMessage({channel:'framely.reply',id,result},'*');
   }catch(e){if(live)ref.current?.contentWindow?.postMessage({channel:'framely.reply',id,error:String(e)},'*');}
  }
  const event=(e:Event)=>{const value=(e as CustomEvent).detail;if(value.kind==='ui.visibility.changed'){api('ui.visibility.get',{view:viewKey()}).then(data=>{if(live)ref.current?.contentWindow?.postMessage({channel:'framely.reply',event:{type:'ui.visibility.changed',data}},'*');}).catch(()=>{});}if(value.kind==='plugin.dependency.changed')ref.current?.contentWindow?.postMessage({channel:'framely.reply',event:{type:'dependencies.changed'}},'*');if(value.plugin===plugin&&value.kind==='plugin.launch.context'&&value.entry===entry)ref.current?.contentWindow?.postMessage({channel:'framely.reply',event:{type:'ui.launch',data:value.context}},'*');if(value.plugin===plugin&&value.kind==='plugin.event')ref.current?.contentWindow?.postMessage({channel:'framely.reply',event:{type:value.event,data:value.data}},'*');};
  const language=()=>ref.current?.contentWindow?.postMessage({channel:'framely.reply',event:{type:'language.changed',data:{preference:pluginLanguagePreference,language:currentLanguage()}}},'*');
  const launched=(e:Event)=>ref.current?.contentWindow?.postMessage({channel:'framely.reply',event:{type:'ui.launch',data:(e as CustomEvent).detail}},'*');
  window.addEventListener('framely.launch',launched);window.addEventListener('framely.event',event);window.addEventListener('framely.language',language);window.addEventListener('message',message);
  void (async()=>{
   await api('plugin.open',{plugin});const status=await api('status');
   const local=!!(status.database.plugins[plugin]?.manifest.ui.windows[entry] as any)?.localWeb;
   let url=actionContext?`/plugin-action-frame/${plugin}/${entry.slice(7)}`:`/plugin-frame/${plugin}/${entry}`,flags=local?'allow-scripts allow-downloads':'allow-scripts';
   if(local&&entry!=='quick'){
    if(!['localhost','127.0.0.1','[::1]'].includes(location.hostname))throw new Error('Open this window on the Frame device');
    const value=await api('plugin.call',{plugin,method:'window.get',params:{window:entry}});
    const target=new URL(value.url);
    if(target.protocol!=='http:'||target.hostname!=='localhost'||!target.port||target.port===location.port||target.username||target.password||target.pathname!==`/framely-window/${entry}`||target.search||target.hash)throw new Error('Invalid local plugin window URL');
    url=target.href;flags='allow-scripts allow-same-origin allow-forms allow-downloads allow-popups allow-popups-to-escape-sandbox';
   }
   if(live){setSandbox(flags);setSource(url);}
  })().catch(e=>{if(live)setError(String(e));});
  return()=>{live=false;window.removeEventListener('message',message);window.removeEventListener('framely.event',event);window.removeEventListener('framely.language',language);};
 },[plugin,entry,attempt]);
 return error?<div><p className="error">{error}</p><button onClick={()=>setAttempt(v=>v+1)}>{t('重试')}</button></div>:source?<iframe title={t('插件页面')} ref={ref} sandbox={sandbox} src={source} allow="clipboard-read; clipboard-write; fullscreen"/>:<p>{t('正在加载…')}</p>;
}
