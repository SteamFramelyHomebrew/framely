import React,{useEffect,useRef,useState} from 'react';
import {api} from './api';
import {t} from './i18n';
import './apk-permission.css';

type Pending={id:string;appName:string;package:string};
type Frame={id:string;appName:string;package:string;image:string;left:number;top:number;width:number;height:number};

export function ApkPermission(){
 const [frame,setFrame]=useState<Frame|null>(null);
 const [error,setError]=useState('');
 const [busy,setBusy]=useState(false);
 const [pointer,setPointer]=useState<{x:number;y:number}|null>(null);
 const active=useRef(true);
 const loading=useRef(false);
 const choosing=useRef(false);
 async function refresh(){
  if(loading.current||choosing.current)return;
  loading.current=true;
  try{
   const pending=await api<Pending|null>('apk.permission.pending');
   if(!pending){if(active.current)setFrame(null);return;}
   const next=await api<Frame>('apk.permission.frame',{id:pending.id});
   if(active.current){setFrame(next);setError('');}
  }catch(e){if(active.current)setError(String(e));}
  finally{loading.current=false;}
 }
 useEffect(()=>{
  active.current=true;
  void refresh();
  const timer=window.setInterval(()=>void refresh(),2000);
  return()=>{active.current=false;window.clearInterval(timer);};
 },[]);
 async function choose(event:React.MouseEvent<HTMLImageElement>){
  if(event.button!==0||!frame||choosing.current)return;
  event.preventDefault();
  const box=event.currentTarget.getBoundingClientRect();
  const x=Math.floor(frame.left+(event.clientX-box.left)/box.width*frame.width);
  const y=Math.floor(frame.top+(event.clientY-box.top)/box.height*frame.height);
  choosing.current=true;setBusy(true);setError('');
  try{await api('apk.permission.click',{id:frame.id,x,y});setFrame(null);}
  catch(e){setError(String(e));}
  finally{choosing.current=false;setBusy(false);window.setTimeout(()=>void refresh(),350);}
 }
 return <main className="apk-permission-view" onMouseMove={event=>{const box=event.currentTarget.getBoundingClientRect();setPointer({x:event.clientX-box.left,y:event.clientY-box.top});}} onMouseLeave={()=>setPointer(null)}>
  <header><span className="apk-permission-mark" aria-hidden="true"/><div><strong>{t('安卓权限请求')}</strong><small>{frame?.appName??t('正在读取权限请求')}</small></div></header>
  <section className="apk-permission-display" aria-label={t('安卓权限选择')}>
   {frame?<img src={frame.image} alt={t('安卓原生权限对话框')} onMouseDown={choose} draggable={false} style={{cursor:busy?'wait':'pointer'}}/>:<p>{t('正在等待安卓权限画面…')}</p>}
  </section>
  <footer>{error?<span role="alert">{error}</span>:<span>{t('使用手柄激光选择原生权限选项')}</span>}</footer>
  {pointer&&<span className="apk-permission-pointer" style={{left:pointer.x,top:pointer.y}} aria-hidden="true"/>}
 </main>;
}
