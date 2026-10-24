import React,{useEffect,useState} from 'react';
import {createPortal} from 'react-dom';
import {api} from './api';
import {ApkManager} from './apk-manager';
import {configureLanguage,t,type LanguagePack} from './i18n';

export function ApkUpdate({id}:{id:string}){
 const [pointer,setPointer]=useState<{x:number;y:number}|null>(null);
 useEffect(()=>{
  const move=(event:MouseEvent)=>setPointer(event.clientX>=0&&event.clientY>=0&&event.clientX<innerWidth&&event.clientY<innerHeight?{x:event.clientX,y:event.clientY}:null);
  const leave=()=>setPointer(null);
  document.addEventListener('mousemove',move);
  document.addEventListener('mouseleave',leave);
  window.addEventListener('blur',leave);
  return()=>{document.removeEventListener('mousemove',move);document.removeEventListener('mouseleave',leave);window.removeEventListener('blur',leave);};
 },[]);
 const [ready,setReady]=useState(false),[error,setError]=useState(''),[attempt,setAttempt]=useState(0);
 useEffect(()=>{
  let live=true;setError('');
  void Promise.all([api<{database:{language?:string}}>('status'),api<{packs?:LanguagePack[]}>('language.list')]).then(([status,languages])=>{
   if(!live)return;
   configureLanguage(status.database.language??'auto',languages.packs??[]);setReady(true);
  }).catch(e=>{if(live)setError(String(e));});
  return()=>{live=false;};
 },[attempt]);
 const cursor=pointer?createPortal(<span className="apk-update-pointer" style={{left:pointer.x,top:pointer.y}} aria-hidden="true"/>,document.body):null;
 if(!ready)return <>{cursor}<main className="apk-update-status" aria-busy={!error}>{error&&<><p role="alert">{error}</p><button onClick={()=>setAttempt(v=>v+1)}>{t('重试')}</button></>}</main></>;
 return <>{cursor}<ApkManager updateId={id}/></>;
}
