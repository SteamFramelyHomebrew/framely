import React,{useEffect,useRef,useState} from 'react';
import {IconApps,IconStar,IconPuzzle,IconBrandSteam,IconBrandAndroid,IconDeviceDesktop,IconPackageImport,IconSettings,IconSearch,IconFocus2,IconCheck,IconAlertCircle} from '@tabler/icons-react';
import {api} from './api';import {t} from './i18n';
import {fitCalibration,fixation,calibrationBlocks,calibrationValidation,type CalibrationPair,type GazeCalibration} from './gaze-calibration';
import type {GazeSample} from './launcher-gaze';
const categoryIcons=[IconStar,IconApps,IconPuzzle,IconBrandSteam,IconBrandAndroid,IconDeviceDesktop],actionIcons=[IconPackageImport,IconApps,IconFocus2,IconSettings];
export function LauncherCalibration({cells,onClose}:{cells:{row:number;column:number}[];onClose:()=>void}){
 const [phase,setPhase]=useState<'intro'|'collect'|'verify'|'result'>('intro'),[step,setStep]=useState(0),[progress,setProgress]=useState(0),[tracking,setTracking]=useState(false),[problem,setProblem]=useState(''),[result,setResult]=useState<GazeCalibration|null>(null),[saving,setSaving]=useState(false),[saved,setSaved]=useState(false);
 const root=useRef<HTMLDivElement>(null),pairs=useRef<CalibrationPair[]>([]),validation=useRef<CalibrationPair[]>([]),model=useRef<GazeCalibration|null>(null),samples=useRef<CalibrationPair[]>([]),entered=useRef(0),last=useRef(0),generation=useRef(0),geometry=useRef({width:0,height:0}),[targetErrors,setTargetErrors]=useState<{target:number;error:number;p95:number;baseline:number}[]>([]); 
 const summaries=useRef<ReturnType<typeof calibrationValidation>[]>([]);
 const targets=phase==='verify'?[2,6,10,15,21]:Array.from({length:24},(_,n)=>n),active=phase==='collect'||phase==='verify'?targets[step]:-1;
 function start(){generation.current++;pairs.current=[];validation.current=[];summaries.current=[];model.current=null;geometry.current={width:window.innerWidth,height:window.innerHeight};setSaved(false);setResult(null);setTargetErrors([]);setProblem('');setStep(0);setPhase('collect');}
 useEffect(()=>{if(active<0||problem)return;samples.current=[];entered.current=performance.now();last.current=0;setProgress(0);const nonce=generation.current;let live=true;
  const targetId=`${nonce}:${phase}:${step}:${performance.now()}`;
  const node=root.current?.querySelector<HTMLElement>(`[data-calibration-target="${active}"]`);if(!node)return;const bounds=node.getBoundingClientRect();
  void api('host.launcher.calibration.target',{id:targetId,x:(bounds.left+bounds.width/2)/window.innerWidth,y:(bounds.top+bounds.height/2)/window.innerHeight,preview:phase==='verify'?model.current:null}).catch(e=>{if(live)setProblem(String(e));});
  const sample=(e:Event)=>{const d=(e as CustomEvent<GazeSample>).detail,now=performance.now();if(d?.targetId!==targetId)return;
   if(d.targetUnavailable){setProblem(t('无法定位曲面目标，请重新打开校准。'));return;}
   if(!d.valid||d.phase==='cancel'){samples.current=[];entered.current=now;last.current=0;setProgress(0);setTracking(false);return;}
   if(!d.rawAngles||!d.targetAngles||!d.headPose||d.headPose.length!==12||![...d.rawAngles,...d.targetAngles,...d.headPose].every(Number.isFinite))return;
   last.current=now;setTracking(true);if(now-entered.current<700)return;
   // Each raw direction is paired with its target using that same frame's
   // eye origin and head pose, so head movement is not fixation jitter.
   samples.current.push({raw:{x:d.rawAngles[0],y:d.rawAngles[1]},target:{x:d.targetAngles[0],y:d.targetAngles[1]}});

  };
  const timer=window.setInterval(()=>{const now=performance.now();if(window.innerWidth!==geometry.current.width||window.innerHeight!==geometry.current.height){setProblem(t('窗口尺寸发生变化，请重新开始校准。'));return;}if(!last.current||now-last.current>180){samples.current=[];entered.current=now;setProgress(0);setTracking(false);return;}setProgress(Math.max(0,Math.min(1,samples.current.length/30,(now-entered.current-700)/1400)));if(now-entered.current<2100||samples.current.length<30)return;
   const residuals=samples.current.map(p=>({x:p.raw.x-p.target.x,y:p.raw.y-p.target.y})),{center,spread}=fixation(residuals);
   if(spread>.04){samples.current=[];entered.current=now;setProgress(0);setProblem(t('眼追与目标的相对抖动过大，请重试当前目标；可以自然转头。'));return;}
   // Reject transient eye outliers, not different head positions. Keep
   // the remaining per-frame pairs; never median together different poses.
   const accepted=samples.current.filter((_,i)=>Math.hypot(residuals[i].x-center.x,residuals[i].y-center.y)<=Math.max(.02,spread*3));
   if(accepted.length<30){samples.current=[];entered.current=now;setProgress(0);return;}
   clearInterval(timer);if(generation.current!==nonce)return;
   const blocks=calibrationBlocks(accepted,active);if(blocks.length<5){setProblem(t('有效采样不足，请重试当前目标。'));return;}
   if(phase==='collect'){pairs.current.push(...blocks);console.log('FRAMELY_GAZE_TARGET '+JSON.stringify({target:active,blocks}));}
   else{validation.current.push(...accepted);const summary=calibrationValidation(accepted,model.current!);summaries.current.push(summary);setTargetErrors(v=>[...v,{target:active,error:summary.bias,p95:summary.p95,baseline:summary.baseline}]);console.log('FRAMELY_GAZE_VALIDATION '+JSON.stringify({target:active,...summary}));}
   if(step<targets.length-1){setStep(step+1);return;}
   if(phase==='collect'){const fitted=fitCalibration(pairs.current,window.innerWidth,window.innerHeight);if(!fitted){setProblem(t('无法计算可靠校准，请重新开始。'));return;}model.current=fitted;console.log('FRAMELY_GAZE_MODEL '+JSON.stringify(fitted));setStep(0);setPhase('verify');}
   else{const fitted=model.current!,errors=summaries.current.map(p=>p.bias),error=Math.sqrt(errors.reduce((s,e)=>s+e*e,0)/errors.length),maxError=Math.max(...errors);const output={...fitted,error,maxError};setResult(output);setPhase('result');console.log('FRAMELY_GAZE_RESULT '+JSON.stringify({model:output,targets:summaries.current}));if(error>.035||maxError>.065)setProblem(t('校准后与高亮目标的角度误差仍过大；查看各目标误差后重试，原校准保持不变。'));}

  },50);window.addEventListener('framely.launcher.gaze',sample);return()=>{live=false;clearInterval(timer);window.removeEventListener('framely.launcher.gaze',sample);};
 },[phase,step,problem]);
 useEffect(()=>()=>{generation.current++;},[]);
 async function save(){if(!result||problem||saving)return;setSaving(true);try{await api('launcher.gaze.calibration.save',result);window.dispatchEvent(new Event('framely.gaze.calibration.changed'));setSaved(true);}catch(e){setProblem(String(e));}finally{setSaving(false);}}
 const targetClass=(id:number)=>active===id?'calibration-target-active':'';
 const targetName=(id:number)=>id<13?t('应用 {0}',{0:id+1}):id<19?t(['收藏','全部','插件','Steam','Lepton','桌面程序'][id-13]):id<23?t(['安装APK','插件库','眼追校准','设置'][id-19]):t('搜索应用');
 const collecting=phase==='collect'||phase==='verify';
 const overlay=phase==='intro'||phase==='result'||!!problem;
 const title=problem?t(phase==='result'?'校准需要重试':'采集已暂停'):t(phase==='intro'?'眼追校准':phase==='result'?saved?'校准已保存。':'校准完成':phase==='verify'?'验证校准':'请注视高亮目标');
 return <div ref={root} className={`launcher-shell launcher-calibration ${overlay?'calibration-show-dialog':''}`}>
 <div className={`launcher-search ${targetClass(23)}`} data-calibration-target={23}><IconSearch size={22}/><span>{t('搜索应用')}</span></div>
 <nav className="launcher-tools launcher-filters" aria-label={t('校准分类按钮')}>{categoryIcons.map((Icon,i)=><span key={i} className={`calibration-rail-target ${targetClass(13+i)}`} data-calibration-target={13+i}><Icon size={26}/></span>)}</nav>
 <nav className="launcher-tools launcher-shortcuts" aria-label={t('校准快捷按钮')}>{actionIcons.map((Icon,i)=><span key={i} className={`calibration-rail-target ${targetClass(19+i)}`} data-calibration-target={19+i}><Icon size={26}/></span>)}</nav>
 <div className="launcher-grid"><div className="launcher-page">{cells.map((cell,i)=><div key={i} className="launch-item" style={{gridColumn:`${cell.column} / span 4`,gridRow:`${1+cell.row*2} / span 2`}}><span data-calibration-target={i} className={`launch-icon ${targetClass(i)}`}><span className="launch-disc"><IconApps size={42}/></span></span><span className="launch-name">{t('应用 {0}',{0:i+1})}</span></div>)}</div></div>
 {collecting&&!problem&&<section className="calibration-live" aria-label={t('校准进度')}>
  <span className="calibration-live-icon"><IconFocus2 size={26} stroke={1.6}/></span>
  <div className="calibration-live-copy"><strong>{tracking?t('看向 {0}',{0:targetName(active)}):t('等待眼动追踪')}</strong><span>{tracking?t('可以自然转头，无需按键。'):t('请保持佩戴，看向高亮目标。')}</span></div>
  <div className="calibration-live-progress"><span>{t(phase==='verify'?'验证':'采集')} <b>{step+1} / {targets.length}</b></span><progress aria-label={t('当前目标进度')} max={1} value={progress}/></div>
  <button className="calibration-secondary" onClick={onClose}>{t('取消')}</button>
 </section>}
 {overlay&&<div className="calibration-dialog-layer"><section className="calibration-dialog" role="dialog" aria-modal="true" aria-labelledby="calibration-title">
  <span className={`calibration-symbol ${problem?'has-error':''}`}>{problem?<IconAlertCircle size={32} stroke={1.5}/>:phase==='result'?<IconCheck size={32} stroke={1.7}/>:<IconFocus2 size={32} stroke={1.5}/>}</span>
  <h1 id="calibration-title">{title}</h1>
  {phase==='intro'&&<><p className="calibration-lead">{t('让注视更贴合启动台。')}</p><ul className="calibration-instructions"><li>{t('依次看向高亮图标或按钮，等待进度完成。')}</li><li>{t('边缘看不清时可以转头，也可以移动头部。')}</li><li>{t('无需点击目标；按 B 可随时退出。')}</li></ul><p className="calibration-footnote">{t('先采集，再验证；完成后由你确认保存。')}</p></>}
  {problem&&<p className="calibration-message" role="alert">{problem}</p>}
  {phase==='result'&&result&&<><p className="calibration-lead">{t(saved?'新的校准已应用到启动台。':problem?'之前的校准保持不变。':'验证已完成，保存后应用到启动台。')}</p><dl className="calibration-metrics"><div><dt>{t('平均目标偏差')}</dt><dd>{(result.error*180/Math.PI).toFixed(2)}<small>°</small></dd></div><div><dt>{t('最差目标偏差')}</dt><dd>{(result.maxError*180/Math.PI).toFixed(2)}<small>°</small></dd></div></dl><details className="calibration-details"><summary>{t('查看各目标误差')}</summary><table><thead><tr><th>{t('目标')}</th><th>{t('校准前')}</th><th>{t('校准后')}</th><th>{t('95%帧误差')}</th></tr></thead><tbody>{targetErrors.map(v=><tr key={v.target}><th>{targetName(v.target)}</th><td>{(v.baseline*180/Math.PI).toFixed(2)}°</td><td>{(v.error*180/Math.PI).toFixed(2)}°</td><td>{(v.p95*180/Math.PI).toFixed(2)}°</td></tr>)}</tbody></table></details></>}
  <div className="calibration-actions">{phase==='intro'?<><button className="calibration-secondary" onClick={onClose}>{t('取消')}</button><button className="calibration-primary" onClick={start}>{t('开始校准')}</button></>:phase==='result'?<><button className="calibration-secondary" onClick={saved?onClose:start} disabled={saving}>{t(saved?'完成':'重新校准')}</button>{!problem&&!saved&&<button className="calibration-primary" onClick={()=>void save()} disabled={saving}>{t(saving?'保存中…':'保存校准')}</button>}{problem&&<button className="calibration-secondary" onClick={onClose}>{t('取消')}</button>}</>:<><button className="calibration-secondary" onClick={onClose}>{t('取消')}</button><button className="calibration-secondary" onClick={start}>{t('重新校准')}</button><button className="calibration-primary" onClick={()=>setProblem('')}>{t('重试当前目标')}</button></>}</div>
 </section></div>}
 </div>;
}
