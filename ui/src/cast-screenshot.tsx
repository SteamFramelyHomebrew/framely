import React,{useEffect,useMemo,useRef,useState} from 'react';
import {IconArrowsMove,IconCamera} from '@tabler/icons-react';
import {api} from './api';
import {t} from './i18n';
import {Select} from './localized-select';
import {FramingEditor} from './cast-framing-editor';
import {frameFromSettings,frameRatios,type ReferenceView} from './cast-framing';
import {normalizeRegion} from './cast-region';
import {FramingLimits,framingMenuScale,type VisibilityGrid} from './cast-framing-limits';
import type {CastSettings} from './casting';
import './cast-spatial-framing.css';
import './cast-screenshot.css';

type Editor={id:string;settings:CastSettings;geometry:null|{referenceTan:number;view:ReferenceView&{visibility:VisibilityGrid;eye:0|1}};spatial:boolean;preview:string|null;previewAspect:number|null};
export function ScreenshotFraming(){
 const [editor,setEditor]=useState<Editor|null>(null),[draft,setDraft]=useState<CastSettings|null>(null),[sourceAspect,setSourceAspect]=useState(1),[error,setError]=useState(''),[busy,setBusy]=useState(false),[closed,setClosed]=useState(false);
 const root=useRef<HTMLElement>(null),menu=useRef<HTMLElement>(null),moveHandle=useRef<HTMLButtonElement>(null),finishing=useRef(false),[minimum,setMinimum]=useState({width:0,height:0,referenceTan:0});
 useEffect(()=>{let alive=true;void api<Editor>('cast.screenshot.get').then(value=>{if(!alive)return;setEditor(value);if(value.previewAspect)setSourceAspect(value.previewAspect);setDraft(value.spatial&&value.geometry?{...value.settings,horizontalFov:2*Math.atan(value.geometry.view.tanHalfHorizontal)*180/Math.PI,centerX:Math.atan(value.geometry.view.centerTanX)*180/Math.PI,centerY:Math.atan(value.geometry.view.centerTanY)*180/Math.PI}:{...value.settings,width:1920,height:Math.round(1920/(value.previewAspect??1)),horizontalFov:null,centerX:0,centerY:0});}).catch(e=>{if(alive)setError(String(e.message??e));});return()=>{alive=false;};},[]);
 useEffect(()=>{const measure=()=>{const area=root.current?.getBoundingClientRect(),panel=menu.current?.getBoundingClientRect();if(area?.width&&area.height&&panel)setMinimum({width:(panel.width+32)/area.width,height:(panel.height+32)/area.height,referenceTan:editor?.geometry?.referenceTan??1});};measure();const observer=typeof ResizeObserver==='undefined'?null:new ResizeObserver(measure);if(menu.current)observer?.observe(menu.current);if(root.current)observer?.observe(root.current);window.addEventListener('resize',measure);return()=>{observer?.disconnect();window.removeEventListener('resize',measure);};},[editor?.id]);
 const reference:ReferenceView=editor?.spatial?{tanHalfHorizontal:editor.geometry?.referenceTan??1,centerTanX:0,centerTanY:0,aspect:1}:{tanHalfHorizontal:1,centerTanX:0,centerTanY:0,aspect:sourceAspect};
 const limits=useMemo(()=>editor?.spatial&&editor.geometry?new FramingLimits(editor.geometry.view.visibility,editor.geometry.referenceTan):null,[editor]);
 const limit=useMemo(()=>limits?.fit.bind(limits),[limits]);
 const ready=!!editor&&!!draft&&(!editor.spatial||minimum.referenceTan===editor.geometry?.referenceTan);
 function change(patch:Partial<CastSettings>){if(!draft||!editor||busy||closed)return;try{setDraft(editor.spatial?normalizeRegion({...draft,...patch},reference,minimum,limit,draft):{...draft,...patch});setError('');}catch(e){setError(t((e as Error).message));}}
 function close(){setClosed(true);setEditor(null);setDraft(null);}
 async function finish(capture:boolean){if(!editor||!draft||finishing.current)return;finishing.current=true;setBusy(true);setError('');try{
  let params:Record<string,unknown>={id:editor.id};
  if(capture){if(editor.spatial){const valid=normalizeRegion(draft,reference,minimum,limit,draft);params={...params,aspect:valid.width/valid.height,horizontalFov:valid.horizontalFov,centerX:valid.centerX,centerY:valid.centerY};}
   else{const frame=frameFromSettings(draft,draft.width/draft.height,reference);params={...params,crop:frame};}}
  await api(capture?'cast.screenshot.capture':'cast.screenshot.cancel',params);close();
 }catch(e){setError(String((e as Error).message??e));try{await api('cast.screenshot.get');}catch{close();}}finally{setBusy(false);finishing.current=false;}}
 useEffect(()=>{const back=()=>void finish(false);window.addEventListener('framely.back',back);return()=>window.removeEventListener('framely.back',back);},[editor,draft]);
 if(closed)return null;
 const aspect=draft?draft.width/draft.height:1;
 return <main ref={root} className={editor?.spatial?'cast-native-framing cast-screenshot-spatial':'cast-screenshot-flat'} aria-label={t('截图取景')} inert={busy}>
  {ready&&draft&&editor?.spatial&&<FramingEditor spatial moveHandle={moveHandle} settings={draft} view={reference} minimumSize={minimum} limit={limit} onChange={change} onError={setError}/>}
  {ready&&draft&&!editor?.spatial&&<div className="cast-screenshot-flat-stage"><img src={editor?.preview??''} alt={t('截图画面')} onLoad={event=>{const img=event.currentTarget;if(img.naturalWidth&&img.naturalHeight)setSourceAspect(img.naturalWidth/img.naturalHeight);}}/><FramingEditor key={sourceAspect} settings={draft} view={reference} previewUrl={editor?.preview??undefined} onChange={change} onError={setError}/></div>}
  <section ref={menu} className={editor?.spatial?'cast-native-framing-controls':'cast-screenshot-flat-controls'} style={editor?.spatial?{'--framing-menu-scale':framingMenuScale(reference.tanHalfHorizontal)} as React.CSSProperties:undefined}>
   <h1>{editor?.spatial?<button ref={moveHandle} className="cast-framing-move" disabled={!ready||busy}>{t('截图取景')}<IconArrowsMove size={22}/></button>:t('截图取景')}</h1>
   <p>{t('拖动取景框移动画面，拖动四角调整范围。截图使用原始可用像素。')}</p>
   {draft&&<div className="cast-native-framing-field"><span>{t('画面比例')}</span><Select label={t('画面比例')} value={String(frameRatios.find(r=>Math.abs(r.value-aspect)<.004)?.value??aspect)} disabled={busy} onChange={value=>{const ratio=Number(value),edge=Math.max(draft.width,draft.height);change(ratio>=1?{width:edge,height:Math.round(edge/ratio)}:{width:Math.round(edge*ratio),height:edge});}} options={[...(!frameRatios.some(r=>Math.abs(r.value-aspect)<.004)?[{value:String(aspect),label:t('当前比例')}]:[]),...frameRatios.map(r=>({value:String(r.value),label:r.label}))]}/></div>}
   {error&&<p role="alert" className="error">{error}</p>}
   <footer><button disabled={busy||!editor} onClick={()=>void finish(false)}>{t('取消')}</button><button className="primary" disabled={busy||!ready} onClick={()=>void finish(true)}><IconCamera size={19}/>{t('截图')}</button></footer>
  </section>
 </main>;
}
