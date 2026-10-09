import React,{useEffect,useRef} from 'react';
import {IconMinus,IconPlus,IconRestore} from '@tabler/icons-react';
import {CastVideo} from './cast-viewer';
import {clamp,fitFrame,frameFromSettings,settingsFromFrame,type FrameRect,type ReferenceView} from './cast-framing';
import type {CastSettings} from './casting';
import {CastRange,updateRangeFill} from './cast-range';
import {t} from './i18n';

export function FramingEditor({settings,view,onChange,onError}:{settings:CastSettings;view:ReferenceView|null;onChange:(patch:Partial<CastSettings>)=>void;onError:(error:string)=>void}){
 const stage=useRef<HTMLDivElement>(null),box=useRef<HTMLDivElement>(null),zoom=useRef<HTMLInputElement>(null),readout=useRef<HTMLOutputElement>(null);
 const current=useRef<FrameRect>({x:0,y:0,width:1,height:1}),gesture=useRef<{x:number;y:number;rect:FrameRect;corner:string|null}|null>(null);
 const aspect=settings.width/settings.height;
 const maxWidth=view?Math.min(1,aspect/view.aspect):1,minWidth=view?Math.min(maxWidth,Math.tan(5*Math.PI/180)/view.tanHalfHorizontal):.1;
 function draw(rect:FrameRect){
  current.current=rect;
  if(box.current){box.current.style.left=`${rect.x*100}%`;box.current.style.top=`${rect.y*100}%`;box.current.style.width=`${rect.width*100}%`;box.current.style.height=`${rect.height*100}%`;}
  if(view){const fov=settingsFromFrame(rect,view).horizontalFov;if(zoom.current){zoom.current.value=String(maxWidth>minWidth?(rect.width-minWidth)/(maxWidth-minWidth)*100:100);updateRangeFill(zoom.current);}if(readout.current)readout.current.textContent=`${Math.round(fov)}°`;}
 }
 function commit(){if(view)onChange(settingsFromFrame(current.current,view));}
 useEffect(()=>{if(view)draw(frameFromSettings(settings,aspect,view));},[view?.tanHalfHorizontal,view?.centerTanX,view?.centerTanY,aspect,settings.horizontalFov,settings.centerX,settings.centerY]);
 function scale(width:number){if(!view)return;const r=current.current;const next=fitFrame({...r,width,x:r.x+(r.width-width)/2,y:r.y+(r.height-width*view.aspect/aspect)/2},aspect,view.aspect,Math.tan(5*Math.PI/180)/view.tanHalfHorizontal);draw(next);}
 function down(e:React.PointerEvent<HTMLDivElement>){if(!view||e.button!==0)return;e.preventDefault();e.currentTarget.setPointerCapture(e.pointerId);gesture.current={x:e.clientX,y:e.clientY,rect:{...current.current},corner:(e.target as HTMLElement).closest<HTMLElement>('[data-corner]')?.dataset.corner??null};}
 function move(e:React.PointerEvent<HTMLDivElement>){const g=gesture.current,bounds=stage.current?.getBoundingClientRect();if(!g||!bounds||!view)return;const dx=(e.clientX-g.x)/bounds.width,dy=(e.clientY-g.y)/bounds.height;
  if(!g.corner){draw(fitFrame({...g.rect,x:g.rect.x+dx,y:g.rect.y+dy},aspect,view.aspect));return;}
  const left=g.corner.includes('w'),top=g.corner.includes('n'),ratio=aspect/view.aspect;
  const delta=Math.abs(dx)>Math.abs(dy*ratio)?dx*(left?-1:1):dy*ratio*(top?-1:1);
  const anchorX=left?g.rect.x+g.rect.width:g.rect.x,anchorY=top?g.rect.y+g.rect.height:g.rect.y;
  const maximum=Math.min(left?anchorX:1-anchorX,(top?anchorY:1-anchorY)*ratio);
  const width=clamp(g.rect.width+delta,Math.min(maximum,Math.tan(5*Math.PI/180)/view.tanHalfHorizontal),maximum),height=width/ratio;
  draw({x:left?anchorX-width:anchorX,y:top?anchorY-height:anchorY,width,height});
 }
 function end(e:React.PointerEvent<HTMLDivElement>){if(!gesture.current)return;gesture.current=null;if(e.currentTarget.hasPointerCapture(e.pointerId))e.currentTarget.releasePointerCapture(e.pointerId);commit();}
 function keyboard(e:React.KeyboardEvent<HTMLDivElement>){if(!view)return;const delta=e.shiftKey?.05:.01,r=current.current;const changes:Record<string,Partial<FrameRect>>={ArrowLeft:{x:r.x-delta},ArrowRight:{x:r.x+delta},ArrowUp:{y:r.y-delta},ArrowDown:{y:r.y+delta}};if(changes[e.key]){e.preventDefault();draw(fitFrame({...r,...changes[e.key]},aspect,view.aspect));commit();}}
 return <div className="cast-framing-editor">
  <div className="cast-framing-stage" ref={stage}>
   <CastVideo active controls={false} aspectRatio={1} onError={onError}/>
   {view&&<div ref={box} className="cast-crop-box" tabIndex={0} role="group" aria-label={t('取景框，使用方向键移动')} onKeyDown={keyboard} onPointerDown={down} onPointerMove={move} onPointerUp={end} onPointerCancel={end}>
    <div className="cast-crop-grid" aria-hidden="true"/>
    {['nw','ne','sw','se'].map(corner=><span key={corner} data-corner={corner} className={`cast-crop-handle ${corner}`} aria-hidden="true"/>)}
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
