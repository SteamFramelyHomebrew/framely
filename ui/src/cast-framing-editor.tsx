import React,{useEffect,useRef} from 'react';
import {IconMinus,IconPlus,IconRestore} from '@tabler/icons-react';
import {CastVideo} from './cast-viewer';
import {fitFrame,frameFromSettings,settingsFromFrame,type FrameRect,type ReferenceView} from './cast-framing';
import type {CastSettings} from './casting';
import {CastRange,updateRangeFill} from './cast-range';
import {t} from './i18n';
import {FrameGesture,fitMinimumFrame,minimumFrameWidth,resizeFrameCentered,type MinimumFrame} from './cast-framing-gesture';
import type {FrameLimiter} from './cast-region';

export function FramingEditor({settings,view,onChange,onError,spatial=false,previewUrl,minimumSize={width:0,height:0},limit,moveHandle}:{settings:CastSettings;view:ReferenceView|null;onChange:(patch:Partial<CastSettings>)=>void;onError:(error:string)=>void;spatial?:boolean;previewUrl?:string;minimumSize?:MinimumFrame;limit?:FrameLimiter;moveHandle?:React.RefObject<HTMLElement|null>}){
 const stage=useRef<HTMLDivElement>(null),box=useRef<HTMLDivElement>(null),zoom=useRef<HTMLInputElement>(null),readout=useRef<HTMLOutputElement>(null);
 const current=useRef<FrameRect>({x:0,y:0,width:1,height:1}),gesture=useRef(new FrameGesture()),pointer=useRef<number|null>(null),release=useRef<(id?:number)=>void>(()=>{});
 const capture=useRef<HTMLElement|null>(null),external=useRef({down:(_e:PointerEvent)=>{},move:(_e:PointerEvent)=>{}});
 const aspect=settings.width/settings.height;
 const ratio=aspect/(view?.aspect??1),maxWidth=Math.min(1,ratio),minWidth=minimumFrameWidth(ratio,minimumSize,view?Math.tan(5*Math.PI/180)/view.tanHalfHorizontal:.1);
 function draw(rect:FrameRect){
  try{if(limit)rect=limit(rect,ratio,minWidth,current.current);}catch(e){if(box.current)box.current.style.visibility='hidden';onError((e as Error).message);return null;}
  current.current=rect;
  if(box.current){box.current.style.visibility='visible';box.current.style.left=`${rect.x*100}%`;box.current.style.top=`${rect.y*100}%`;box.current.style.width=`${rect.width*100}%`;box.current.style.height=`${rect.height*100}%`;}
  if(view){const fov=settingsFromFrame(rect,view).horizontalFov;if(zoom.current){zoom.current.value=String(maxWidth>minWidth?(rect.width-minWidth)/(maxWidth-minWidth)*100:100);updateRangeFill(zoom.current);}if(readout.current)readout.current.textContent=`${Math.round(fov)}°`;}
  return rect;
 }
 function commit(){if(view&&box.current?.style.visibility!=='hidden')onChange(settingsFromFrame(current.current,view));}
 useEffect(()=>{if(view){const old=frameFromSettings(settings,aspect,view),next=draw(fitMinimumFrame(old,ratio,minWidth));if(next&&(Math.abs(next.width-old.width)>1e-9||Math.abs(next.x-old.x)>1e-9||Math.abs(next.y-old.y)>1e-9))commit();}},[view?.tanHalfHorizontal,view?.centerTanX,view?.centerTanY,aspect,settings.horizontalFov,settings.centerX,settings.centerY,minWidth]);
 function scale(width:number){if(view)draw(resizeFrameCentered(current.current,width,ratio,minWidth));}
 function finish(id?:number){if(!gesture.current.finish(id))return;const captured=pointer.current,target=capture.current;pointer.current=null;capture.current=null;target?.removeAttribute('data-dragging');try{if(captured!==null&&target?.hasPointerCapture(captured))target.releasePointerCapture(captured);}catch{}commit();}
 release.current=finish;
 useEffect(()=>{const up=(e:PointerEvent)=>release.current(e.pointerId),cancel=()=>release.current();document.addEventListener('pointerup',up,true);document.addEventListener('pointercancel',up,true);window.addEventListener('blur',cancel);window.addEventListener('framely.pointerRelease',cancel);return()=>{document.removeEventListener('pointerup',up,true);document.removeEventListener('pointercancel',up,true);window.removeEventListener('blur',cancel);window.removeEventListener('framely.pointerRelease',cancel);gesture.current.finish();};},[]);
 function begin(e:Pick<PointerEvent,'button'|'pointerId'|'clientX'|'clientY'|'preventDefault'>,target:HTMLElement,corner:string|null){if(!view||e.button!==0)return;e.preventDefault();finish();gesture.current.begin(e.pointerId,e.clientX,e.clientY,current.current,corner);pointer.current=e.pointerId;capture.current=target;target.setAttribute('data-dragging','');try{target.setPointerCapture(e.pointerId);}catch{}}
 function down(e:React.PointerEvent<HTMLDivElement>){begin(e,e.currentTarget,(e.target as HTMLElement).closest<HTMLElement>('[data-corner]')?.dataset.corner??null);}
 function move(e:Pick<PointerEvent,'pointerId'|'clientX'|'clientY'|'buttons'>){if(!gesture.current.active(e.pointerId))return;if(!(e.buttons&1)){finish(e.pointerId);return;}const bounds=stage.current?.getBoundingClientRect();if(!bounds||!view)return;const next=gesture.current.move(e.pointerId,e.clientX,e.clientY,e.buttons,bounds,ratio,minWidth);if(next)draw(next);
 }
 external.current={down:e=>{if(moveHandle?.current)begin(e,moveHandle.current,null);},move};
 useEffect(()=>{
  const target=moveHandle?.current;if(!target)return;
  const down=(e:PointerEvent)=>external.current.down(e),move=(e:PointerEvent)=>external.current.move(e),end=(e:PointerEvent)=>release.current(e.pointerId);
  target.addEventListener('pointerdown',down);target.addEventListener('pointermove',move);target.addEventListener('lostpointercapture',end);
  return()=>{target.removeEventListener('pointerdown',down);target.removeEventListener('pointermove',move);target.removeEventListener('lostpointercapture',end);release.current();};
 },[moveHandle]);
 function end(e:React.PointerEvent<HTMLDivElement>){finish(e.pointerId);}
 function keyboard(e:React.KeyboardEvent<HTMLDivElement>){if(!view)return;const delta=e.shiftKey?.05:.01,r=current.current;const changes:Record<string,Partial<FrameRect>>={ArrowLeft:{x:r.x-delta},ArrowRight:{x:r.x+delta},ArrowUp:{y:r.y-delta},ArrowDown:{y:r.y+delta}};if(changes[e.key]){e.preventDefault();draw(fitFrame({...r,...changes[e.key]},aspect,view.aspect));commit();}}
 return <div className="cast-framing-editor">
  <div className="cast-framing-stage" ref={stage} style={previewUrl&&view?{aspectRatio:String(view.aspect)}:undefined}>
   {previewUrl?<img src={previewUrl} alt={t('截图取景预览')} draggable={false} className="cast-screenshot-preview-image"/>:!spatial&&<CastVideo active controls={false} aspectRatio={1} onError={onError}/>}
   {view&&<div ref={box} className="cast-crop-box" tabIndex={0} role="group" aria-label={t('取景框，使用方向键移动')} onKeyDown={keyboard} onPointerDown={down} onPointerMove={move} onPointerUp={end} onPointerCancel={end} onLostPointerCapture={end}>
    <div className="cast-crop-grid" aria-hidden="true"/>
    {['nw','ne','sw','se'].map(corner=><span key={corner} data-corner={corner} data-framely-interactive="" className={`cast-crop-handle ${corner}`} aria-hidden="true"/>)}
   </div>}
  </div>
  <div className="cast-zoom-controls">
   <button aria-label={t('缩小取景范围')} disabled={!view} onClick={()=>{scale(current.current.width*.9);commit();}}><IconMinus size={20}/></button>
   <label><span>{t('视野')}</span><CastRange ref={zoom} min={0} max={100} step="any" disabled={!view} aria-label={t('视野')} defaultValue={100} onChange={e=>{if(view)scale(minWidth+(maxWidth-minWidth)*Number(e.target.value)/100);}} onPointerUp={commit} onKeyUp={commit}/><output ref={readout}/></label>
   <button aria-label={t('扩大取景范围')} disabled={!view} onClick={()=>{scale(current.current.width/ .9);commit();}}><IconPlus size={20}/></button>
   <button disabled={!view} onClick={()=>{if(view){draw(frameFromSettings({horizontalFov:null,centerX:0,centerY:0},aspect,view));commit();}}}><IconRestore size={20}/>{t('重置取景')}</button>
  </div>
  <p className="cast-help">{t('拖动取景框移动画面，拖动四角调整范围。也可以用方向键移动。')}</p>
 </div>;
}
