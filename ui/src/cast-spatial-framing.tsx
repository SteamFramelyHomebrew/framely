import React,{useEffect,useMemo,useRef,useState} from 'react';
import {api} from './api';
import {t,configureLanguage,type LanguagePack} from './i18n';
import {Select} from './localized-select';
import type {CastSettings} from './casting';
import {FramingEditor} from './cast-framing-editor';
import {framePresets,frameRatios,settingsFromFrame,type ReferenceView} from './cast-framing';
import {normalizeRegion} from './cast-region';
import {FramingLimits,framingMenuScale,type VisibilityGrid} from './cast-framing-limits';
import {minimumFrameWidth} from './cast-framing-gesture';
import './casting.css';
import './cast-spatial-framing.css';
type Geometry={id:string;settings:CastSettings;referenceTan:number;view:ReferenceView&{visibility:VisibilityGrid;eye:0|1}};
export async function loadSpatialFraming(request:typeof api=api){
 const [geometry,status,language]=await Promise.all([request<Geometry>('cast.framing.get'),request<{database:{language?:string}}>('status'),request<{packs?:LanguagePack[]}>('language.list')]);
 configureLanguage(status.database.language??'auto',language.packs??[]);
 return geometry;
}
export function SpatialFraming(){
 const [geometry,setGeometry]=useState<Geometry|null>(null),[draft,setDraft]=useState<CastSettings|null>(null),[error,setError]=useState(''),[busy,setBusy]=useState(false),[closed,setClosed]=useState(false);
 const finishing=useRef(false),finished=useRef(false);
 const root=useRef<HTMLElement>(null),menu=useRef<HTMLElement>(null),[minimum,setMinimum]=useState({width:0,height:0,referenceTan:0});
 useEffect(()=>{const measure=()=>{const area=root.current?.getBoundingClientRect(),panel=menu.current?.getBoundingClientRect();if(area?.width&&area.height&&panel)setMinimum({width:(panel.width+32)/area.width,height:(panel.height+32)/area.height,referenceTan:geometry?.referenceTan??1});};measure();const observer=typeof ResizeObserver==='undefined'?null:new ResizeObserver(measure);if(menu.current)observer?.observe(menu.current);if(root.current)observer?.observe(root.current);window.addEventListener('resize',measure);return()=>{observer?.disconnect();window.removeEventListener('resize',measure);};},[geometry?.referenceTan]);
 const reference:ReferenceView={tanHalfHorizontal:geometry?.referenceTan??1,centerTanX:0,centerTanY:0,aspect:1};
 const limits=useMemo(()=>geometry?new FramingLimits(geometry.view.visibility,geometry.referenceTan):null,[geometry]);
 const limit=useMemo(()=>limits?.fit.bind(limits),[limits]);
 useEffect(()=>{let alive=true;document.documentElement.classList.add('cast-framing-view');
  void loadSpatialFraming().then(g=>{if(!alive)return;setGeometry(g);setDraft({...g.settings,horizontalFov:2*Math.atan(g.view.tanHalfHorizontal)*180/Math.PI,centerX:Math.atan(g.view.centerTanX)*180/Math.PI,centerY:Math.atan(g.view.centerTanY)*180/Math.PI});}).catch(e=>{if(alive)setError(String(e.message??e));});
  return()=>{alive=false;document.documentElement.classList.remove('cast-framing-view');};
 },[]);
 function change(patch:Partial<CastSettings>){if(!draft||finishing.current||finished.current||minimum.referenceTan!==geometry?.referenceTan)return false;try{setDraft(normalizeRegion({...draft,...patch},reference,minimum,limit,draft));return true;}catch(e){setError(t((e as Error).message));return false;}}
 function resetSize(){if(!draft||!limits)return;try{const ratio=draft.width/draft.height,minWidth=minimumFrameWidth(ratio,minimum,Math.tan(5*Math.PI/180)/reference.tanHalfHorizontal);if(change(settingsFromFrame(limits.reset(ratio,minWidth),reference)))setError('');}catch(e){setError(t((e as Error).message));}}
 function closeEditor(){finished.current=true;setClosed(true);setGeometry(null);setDraft(null);}
 async function finish(apply:boolean){if(!geometry||finishing.current||finished.current)return;finishing.current=true;setBusy(true);setError('');try{
  await api(apply?'cast.framing.apply':'cast.framing.cancel',apply&&draft?{id:geometry.id,expectedEye:geometry.view.eye,...normalizeRegion(draft,reference,minimum,limit,draft)}:{id:geometry.id});
  // The native host closes asynchronously. Never leave the completed editor
  // interactive while it is waiting for that close command or status update.
  closeEditor();
 }catch(e){setError(String((e as Error).message??e));
  // Saving can succeed even if restarting the stream fails afterwards. The
  // backend has already closed that editor, so do not revive its stale id.
  try{const latest=await api<Geometry>('cast.framing.get');if(latest.id!==geometry.id)closeEditor();}
  catch(stateError){if((stateError as Error).message===t('画幅调整窗口已关闭'))closeEditor();}
 }finally{finishing.current=false;setBusy(false);}}
 useEffect(()=>{const close=()=>void finish(false);window.addEventListener('framely.back',close);return()=>window.removeEventListener('framely.back',close);},[geometry,busy]);
 const aspect=draft?draft.width/draft.height:4/3;
 const ready=!!geometry&&minimum.referenceTan===geometry.referenceTan;
 if(closed)return null;
 return <main ref={root} className="cast-native-framing" aria-label={t('画幅调整')} inert={busy}>
  {draft&&geometry&&ready&&<FramingEditor spatial limit={limit} minimumSize={minimum} settings={draft} view={reference} onChange={change} onError={message=>setError(t(message))}/>}
  <section ref={menu} className="cast-native-framing-controls" style={{'--framing-menu-scale':framingMenuScale(reference.tanHalfHorizontal)} as React.CSSProperties}>
   <h1>{t('画幅调整')}</h1><p>{t('框内即录制区域，拖动移动，拖动四角等比例缩放。')}</p>
   <div className="cast-native-framing-field"><span>{t('画面比例')}</span><Select label={t('画面比例')} disabled={busy||!draft} value={String(frameRatios.find(r=>Math.abs(r.value-aspect)<.004)?.value??aspect)} onChange={value=>{const ratio=Number(value),edge=Math.max(draft!.width,draft!.height);change(ratio>=1?{width:edge,height:Math.round(edge/ratio/2)*2}:{width:Math.round(edge*ratio/2)*2,height:edge});}} options={[
    ...(!frameRatios.some(r=>Math.abs(r.value-aspect)<.004)?[{value:String(aspect),label:t('自定义')}]:[]),
    ...frameRatios.map(r=>({value:String(r.value),label:r.label}))
   ]}/></div>
   <div className="cast-native-framing-field"><span>{t('录制分辨率')}</span><Select label={t('录制分辨率')} disabled={busy||!draft} value={draft?`${draft.width}x${draft.height}`:''} onChange={value=>{const [width,height]=value.split('x').map(Number);change({width,height});}} options={[
    ...(draft&&!framePresets(aspect).some(s=>s.width===draft.width&&s.height===draft.height)?[{value:`${draft.width}x${draft.height}`,label:`${draft.width} × ${draft.height}`}]:[]),
    ...framePresets(aspect).filter(s=>s.width<=4096&&s.height<=4096&&s.width*s.height<=8847360).map(s=>({value:`${s.width}x${s.height}`,label:`${s.width} × ${s.height}`}))
   ]}/></div>
   {error&&<p role="alert" className="error">{error}</p>}
   <button className="cast-framing-reset" title={t('保留比例和分辨率，恢复居中有效画幅')} disabled={busy||!draft||!limits||!ready} onClick={resetSize}>{t('重置大小')}</button>
   <footer><button disabled={busy||!geometry} onClick={()=>void finish(false)}>{t('取消')}</button><button className="primary" disabled={busy||!draft||!ready} onClick={()=>void finish(true)}>{t('应用取景')}</button></footer>
  </section>
 </main>;
}
