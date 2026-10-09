import React,{useEffect,useState} from 'react';
import {IconCast,IconPlayerPlay,IconPlayerStop,IconArrowUpRight,IconDeviceVisionPro} from '@tabler/icons-react';
import {api} from './api';
import {t} from './i18n';
import {Switch} from './switch';
import type {CastSettings} from './casting';

type Runtime={running:boolean;error?:string;receiverError?:string;watchAddresses?:string[]};
export function CastQuick({config,refresh}:{config:CastSettings;refresh:()=>Promise<void>}){
 const [runtime,setRuntime]=useState<Runtime|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('');
 useEffect(()=>{let alive=true;const poll=()=>void api<Runtime>('cast.status').then(v=>{if(alive)setRuntime(v);}).catch(e=>{if(alive)setError(e.message);});poll();const timer=setInterval(poll,1000);return()=>{alive=false;clearInterval(timer);};},[]);
 async function run(task:()=>Promise<unknown>){if(busy)return;setBusy(true);setError('');try{await task();setRuntime(await api('cast.status'));await refresh();}catch(e){setError(e instanceof Error?e.message:String(e));}finally{setBusy(false);}}
 const running=!!runtime?.running;
 return <section className="cast-quick">
  <header><IconCast size={25} stroke={1.8}/><h1>{t('串流')}</h1><button aria-label={t('投屏设置')} onClick={()=>void run(()=>api('host.manager.open',{page:'casting'}))}><IconArrowUpRight size={22}/></button></header>
  <div className="cast-quick-stream"><IconDeviceVisionPro size={40} stroke={1.5}/><h2>{running?t('正在串流'):t('头显画面')}</h2><p>{runtime?`${config.width} × ${config.height} / ${config.fps} FPS`:t('正在连接')}</p><button className={running?'':'primary'} disabled={busy||!runtime} onClick={()=>void run(()=>api(running?'cast.stop':'cast.start'))}>{running?<IconPlayerStop size={21}/>:<IconPlayerPlay size={21}/>}<span>{running?t('停止串流'):t('开始串流')}</span></button></div>
  {running&&runtime?.watchAddresses?.[0]&&<div className="cast-quick-address"><span>{t('网页观看')}</span><code>{runtime.watchAddresses[0]}</code></div>}
  <div className="cast-quick-receive"><h2>{t('接收投屏')}</h2>{(['airplay','dlna'] as const).map(protocol=><label key={protocol}><span>{protocol==='airplay'?'AirPlay':'DLNA'}</span><Switch label={protocol==='airplay'?'AirPlay':'DLNA'} checked={config[protocol]} disabled={busy} onChange={value=>void run(()=>api('cast.settings.save',{...config,[protocol]:value}))}/></label>)}</div>
  {(error||runtime?.error||runtime?.receiverError)&&<p className="error" role="alert">{error||runtime?.error||runtime?.receiverError}</p>}
 </section>;
}
