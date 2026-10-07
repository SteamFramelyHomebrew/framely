import React,{useEffect,useRef,useState} from 'react';
import {createPortal} from 'react-dom';
import {IconChevronLeft,IconChevronRight,IconDownload,IconX,IconZoomIn,IconZoomOut,IconArrowsMaximize} from '@tabler/icons-react';
import {managerApi} from './manager-api';
import {t} from './i18n';
export type MediaEntry={path:string;name:string};
const images=new Set(['png','jpg','jpeg','gif','webp','avif','bmp']),videos=new Set(['mp4','webm','mov','m4v']);
export function mediaKind(entry:MediaEntry){const ext=entry.name.split('.').pop()?.toLowerCase()??'';return images.has(ext)?'image':videos.has(ext)?'video':'';}
type Transform={scale:number;x:number;y:number};
const fitted:Transform={scale:1,x:0,y:0};
export function FileMediaPreview({entries,initial,onClose,onDownload}:{entries:MediaEntry[];initial:number;onClose:()=>void;onDownload:(path:string)=>void}){
 const[index,setIndex]=useState(initial),[source,setSource]=useState<{path:string;url:string}|null>(null),[error,setError]=useState(''),[ready,setReady]=useState(false),[transform,setTransform]=useState(fitted);
 const entry=entries[index],video=mediaKind(entry)==='video',url=source?.path===entry.path?source.url:'';
 const overlay=useRef<HTMLDivElement>(null),stage=useRef<HTMLDivElement>(null),view=useRef(fitted),points=useRef(new Map<number,{x:number;y:number}>());
 const callbacks=useRef({onClose,onDownload});callbacks.current={onClose,onDownload};
 function apply(next:Transform){view.current=next;setTransform(next);}
 function zoom(scale:number,anchor={x:0,y:0}){const current=view.current,next=Math.min(8,Math.max(.25,scale)),ratio=next/current.scale;apply({scale:next,x:anchor.x-(anchor.x-current.x)*ratio,y:anchor.y-(anchor.y-current.y)*ratio});}
 function move(direction:number){setIndex(current=>Math.max(0,Math.min(entries.length-1,current+direction)));}
 useEffect(()=>{
  const previous=document.activeElement as HTMLElement|null;overlay.current?.focus();
  const overflow=document.body.style.overflow;document.body.style.overflow='hidden';
  return()=>{document.body.style.overflow=overflow;if(previous?.isConnected)previous.focus();};
 },[]);
 useEffect(()=>{
  setSource(null);setError('');setReady(false);apply(fitted);points.current.clear();let live=true,token:string|undefined;
  const release=()=>{if(token){void managerApi('files',{operation:'preview.release',token}).catch(()=>{});token=undefined;}};
  // A completed request is always released, even if the user closes while it is pending.
  void managerApi<{url:string;token?:string}>('files',{operation:'preview',path:entry.path}).then(result=>{token=result.token;if(live)setSource({path:entry.path,url:result.url});else release();}).catch(e=>{if(live)setError(String(e));});
  return()=>{live=false;release();};
 },[entry.path]);
 function keyDown(event:React.KeyboardEvent){
  if(event.key==='Tab'){
   const buttons=Array.from(overlay.current!.querySelectorAll<HTMLElement>('button:not(:disabled),video[controls]'));
   const first=buttons[0],last=buttons.at(-1);if(event.shiftKey&&(document.activeElement===first||document.activeElement===overlay.current)){event.preventDefault();last?.focus();}else if(!event.shiftKey&&(document.activeElement===last||document.activeElement===overlay.current)){event.preventDefault();first?.focus();}return;
  }
  // Let the native player retain its own keyboard controls.
  if((event.target as HTMLElement).tagName==='VIDEO'&&event.key!=='Escape')return;
  if(event.key==='Escape'){event.preventDefault();event.stopPropagation();callbacks.current.onClose();}
  else if(event.key==='ArrowLeft'){event.preventDefault();move(-1);}
  else if(event.key==='ArrowRight'){event.preventDefault();move(1);}
  else if(event.key==='+'||event.key==='='){event.preventDefault();zoom(view.current.scale*1.25);}
  else if(event.key==='-'){event.preventDefault();zoom(view.current.scale/1.25);}
  else if(event.key==='0'){event.preventDefault();apply(fitted);}
 }
 useEffect(()=>{const element=stage.current!;const wheel=(event:WheelEvent)=>{event.preventDefault();event.stopPropagation();const bounds=element.getBoundingClientRect();zoom(view.current.scale*Math.exp(-event.deltaY*.002),{x:event.clientX-bounds.left-bounds.width/2,y:event.clientY-bounds.top-bounds.height/2});};element.addEventListener('wheel',wheel,{passive:false});return()=>element.removeEventListener('wheel',wheel);},[]);
 function position(event:React.PointerEvent){return{x:event.clientX,y:event.clientY};}
 function geometry(){const p=[...points.current.values()];return{center:{x:(p[0].x+(p[1]?.x??p[0].x))/2,y:(p[0].y+(p[1]?.y??p[0].y))/2},distance:p[1]?Math.hypot(p[1].x-p[0].x,p[1].y-p[0].y):0};}
 function start(event:React.PointerEvent){if(event.button!==0||!ready||(event.target as HTMLElement).closest('button'))return;const player=(event.target as HTMLElement).closest('video');if(player&&event.clientY>player.getBoundingClientRect().bottom-48)return;event.preventDefault();points.current.set(event.pointerId,position(event));event.currentTarget.setPointerCapture(event.pointerId);}
 function drag(event:React.PointerEvent){if(!points.current.has(event.pointerId))return;event.preventDefault();const before=geometry();points.current.set(event.pointerId,position(event));const after=geometry(),current=view.current;
  if(before.distance&&after.distance){const bounds=stage.current!.getBoundingClientRect(),anchor={x:before.center.x-bounds.left-bounds.width/2,y:before.center.y-bounds.top-bounds.height/2};zoom(current.scale*after.distance/before.distance,anchor);}
  apply({...view.current,x:view.current.x+after.center.x-before.center.x,y:view.current.y+after.center.y-before.center.y});
 }
 function end(event:React.PointerEvent){points.current.delete(event.pointerId);if(event.currentTarget.hasPointerCapture(event.pointerId))event.currentTarget.releasePointerCapture(event.pointerId);}
 return createPortal(<div className="file-manager file-media-overlay" data-framely-no-scroll-drag ref={overlay} role="dialog" aria-modal="true" aria-label={entry.name} tabIndex={-1} onKeyDown={keyDown}>
  <header className="file-media-header"><div><strong title={entry.name}>{entry.name}</strong><span>{index+1} / {entries.length}</span></div><div className="file-media-actions">
   <button aria-label={t('缩小')} title={t('缩小')} disabled={!ready||transform.scale<=.25} onClick={()=>zoom(view.current.scale/1.25)}><IconZoomOut size={21}/></button><output aria-label={t('缩放比例')}>{Math.round(transform.scale*100)}%</output>
   <button aria-label={t('放大')} title={t('放大')} disabled={!ready||transform.scale>=8} onClick={()=>zoom(view.current.scale*1.25)}><IconZoomIn size={21}/></button>
   <button aria-label={t('适应窗口')} title={t('适应窗口')} disabled={!ready} onClick={()=>apply(fitted)}><IconArrowsMaximize size={21}/></button>
   <button aria-label={t('下载文件')} title={t('下载文件')} onClick={()=>callbacks.current.onDownload(entry.path)}><IconDownload size={21}/></button>
   <button aria-label={t('关闭')} title={t('关闭')} onClick={()=>callbacks.current.onClose()}><IconX size={22}/></button>
  </div></header>
  <div className="file-media-stage" ref={stage} onPointerDown={start} onPointerMove={drag} onPointerUp={end} onPointerCancel={end} onLostPointerCapture={event=>points.current.delete(event.pointerId)} onDoubleClick={event=>{if((event.target as HTMLElement).tagName!=='VIDEO')view.current.scale===1?zoom(2):apply(fitted);}}>
   {!ready&&!error&&<div className="file-media-loading" role="status">{t('加载中…')}</div>}
   {error?<div className="file-media-error" role="alert"><p>{error}</p><button onClick={()=>callbacks.current.onDownload(entry.path)}><IconDownload size={20}/>{t('下载文件')}</button></div>:url&&<div className="file-media-canvas" style={{transform:`translate(${transform.x}px,${transform.y}px) scale(${transform.scale})`,visibility:ready?'visible':'hidden'}}>
    {video?<video key={url} src={url} controls playsInline preload="metadata" onLoadedData={()=>setReady(true)} onError={()=>setError(t('浏览器无法播放此格式，请下载后打开。'))}/>:<img key={url} src={url} alt={entry.name} draggable={false} onLoad={()=>setReady(true)} onError={()=>setError(t('浏览器无法预览此格式，请下载后打开。'))}/>}
   </div>}
  </div>
  <button className="file-media-prev" aria-label={t('上一个文件')} disabled={index===0} onClick={()=>move(-1)}><IconChevronLeft size={26}/></button>
  <button className="file-media-next" aria-label={t('下一个文件')} disabled={index===entries.length-1} onClick={()=>move(1)}><IconChevronRight size={26}/></button>
  <footer className="file-media-footer">{t('滚轮缩放 · 拖动移动 · ← → 切换 · Esc 关闭')}</footer>
 </div>,document.body);
}
