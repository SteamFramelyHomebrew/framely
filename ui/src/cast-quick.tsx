import React,{useEffect,useState} from 'react';
import {IconCast,IconPlayerPlay,IconPlayerStop,IconArrowUpRight,IconDeviceVisionPro,IconCrop} from '@tabler/icons-react';
import {api} from './api';
import {t} from './i18n';
import {Switch,SwitchRow} from './switch';
import type {CastSettings} from './casting';
import {RecordingButton,type RecordingStatus} from './cast-recording';

type Runtime={running:boolean;recording?:RecordingStatus;recordingRegion?:{editing:boolean;error?:string};preview?:boolean;error?:string;receiverError?:string;watchAddresses?:string[]};
export function CastQuick({config,refresh}:{config:CastSettings;refresh:()=>Promise<void>}){
 const [runtime,setRuntime]=useState<Runtime|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('');
 useEffect(()=>{let alive=true;const poll=()=>void api<Runtime>('cast.status').then(v=>{if(alive)setRuntime(v);}).catch(e=>{if(alive)setError(e.message);});poll();const timer=setInterval(poll,1000);return()=>{alive=false;clearInterval(timer);};},[]);
 async function run(task:()=>Promise<unknown>){if(busy)return;setBusy(true);setError('');try{await task();setRuntime(await api('cast.status'));await refresh();}catch(e){setError(e instanceof Error?e.message:String(e));}finally{setBusy(false);}}
 const running=!!runtime?.running;
 return <section className="cast-quick">
  <header><IconCast size={25} stroke={1.8}/><h1>{t('串流')}</h1><button aria-label={t('投屏设置')} onClick={()=>void run(()=>api('host.manager.open',{page:'casting'}))}><IconArrowUpRight size={22}/></button></header>
  <div className="cast-quick-stream"><IconDeviceVisionPro size={40} stroke={1.5}/><h2>{running?t('正在串流'):t('头显画面')}</h2><p>{runtime?`${config.width} × ${config.height} / ${config.fps} FPS`:t('正在连接')}</p><div className="cast-stream-actions"><button className={running?'':'primary'} disabled={busy||!runtime} onClick={()=>void run(()=>api(running?'cast.stop':'cast.start'))}>{running?<IconPlayerStop size={21}/>:<IconPlayerPlay size={21}/>}<span>{running?t('停止串流'):t('开始串流')}</span></button><RecordingButton recording={runtime?.recording} disabled={busy||!runtime||!!runtime.preview} onClick={()=>void run(()=>api(runtime?.recording?.active?'cast.record.stop':'cast.record.start'))}/></div><SwitchRow label={t('显示录制指示器')} description={t('关闭后仅隐藏右上角录制提示，不会停止录制。')} checked={config.recordingIndicatorVisible??true} disabled={busy} onChange={enabled=>void run(()=>api('cast.recordingIndicator.visible.save',{enabled}))}/><p className="cast-record-path">{t('录制保存到 /home/steamos/Videos/framely')}</p></div>
  {runtime?.recording?.error&&<p className="error" role="alert">{runtime.recording.error}</p>}
  <div className="cast-quick-framing"><div className="cast-quick-framing-row"><button disabled={busy||!runtime||!!runtime.recording?.active||!!runtime.preview||!!runtime.recordingRegion?.editing||config.source!=='screen'||config.output!=='eye'} onClick={()=>void run(()=>api('cast.framing.open'))}><IconCrop size={21}/>{t('画幅调整')}</button>
   <SwitchRow label={t('录制区域显示')} checked={config.recordingRegionVisible??false} disabled={busy||!runtime||(!(config.source==='screen'&&config.output==='eye')&&!config.recordingRegionVisible)} onChange={enabled=>void run(()=>api('cast.region.visible.save',{enabled}))}/>
   </div>{(config.source!=='screen'||config.output!=='eye')&&<p className="cast-help">{t('录制区域显示仅支持屏幕采集的单眼画面')}</p>}
   {runtime?.recordingRegion?.error&&<p className="error" role="alert">{runtime.recordingRegion.error}</p>}
  </div>
  {running&&runtime?.watchAddresses?.[0]&&<div className="cast-quick-address"><span>{t('网页观看')}</span><code>{runtime.watchAddresses[0]}</code></div>}
  <div className="cast-quick-receive"><h2>{t('接收投屏')}</h2>{(['airplay','dlna'] as const).map(protocol=><label key={protocol}><span>{protocol==='airplay'?'AirPlay':'DLNA'}</span><Switch label={protocol==='airplay'?'AirPlay':'DLNA'} checked={config[protocol]} disabled={busy} onChange={value=>void run(()=>api('cast.settings.save',{...config,[protocol]:value}))}/></label>)}</div>
  {(error||runtime?.error||runtime?.receiverError)&&<p className="error" role="alert">{error||runtime?.error||runtime?.receiverError}</p>}
 </section>;
}
