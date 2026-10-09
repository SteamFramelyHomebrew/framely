import React,{useEffect,useRef,useState} from 'react';
import {IconMusic,IconPlayerPause,IconPlayerPlay,IconPlayerStop,IconVolume,IconVolumeOff} from '@tabler/icons-react';
import {api} from './api';
import {CastRange} from './cast-range';
import {t} from './i18n';
type Session={id:string;protocol:string;width?:number;height?:number;error?:string};
export function CastPlayer(){
 const id=location.pathname.split('/').pop()!,[session,setSession]=useState<Session|null>(null),[status,setStatus]=useState({paused:false,position:0,duration:0,seekable:false}),[error,setError]=useState(''),[volume,setVolume]=useState(1),[seek,setSeek]=useState<number|null>(null),[busy,setBusy]=useState(false),[visible,setVisible]=useState(true);
 const timer=useRef<ReturnType<typeof setTimeout>|null>(null),root=useRef<HTMLElement>(null),video=!!session?.width&&!!session?.height;
 function reveal(){setVisible(true);if(timer.current)clearTimeout(timer.current);if(video&&!status.paused)timer.current=setTimeout(()=>{if(!root.current?.querySelector(':focus-visible'))setVisible(false);},3000);}
 useEffect(()=>{reveal();return()=>{if(timer.current)clearTimeout(timer.current);};},[video,status.paused]);
 useEffect(()=>{let alive=true;const poll=()=>void Promise.all([api<Session>('cast.window',{id}),api<typeof status>('cast.control',{id,action:'status'})]).then(([session,status])=>{if(alive){setSession(session);setStatus(status);}}).catch(e=>{if(alive)setError(e.message);});poll();const interval=setInterval(poll,1000);return()=>{alive=false;clearInterval(interval);};},[id]);
 async function control(action:string,params:Record<string,unknown>={}){setBusy(true);try{await api('cast.control',{id,action,...params});setError('');if(action==='stop')setSession(null);}catch(e){setError(e instanceof Error?e.message:String(e));}finally{setBusy(false);reveal();}}
 const time=(seconds:number)=>`${Math.floor(seconds/60)}:${String(Math.floor(seconds%60)).padStart(2,'0')}`;
 return <main ref={root} className={`cast-player ${video?'cast-player-video':'cast-player-audio'} ${visible||status.paused||error||!video?'controls-visible':''}`} onPointerMove={reveal} onClick={e=>{if(!(e.target as HTMLElement).closest('button,input'))reveal();}} onFocusCapture={reveal}>
  {!video&&<div className="cast-audio-art"><IconMusic size={64} stroke={1.3}/><h1>{t('音频投屏')}</h1><p>{session?.protocol??t('正在连接')}</p></div>}
  <header className="cast-player-heading"><span>Framely</span><strong>{session?.protocol}</strong></header>
  <footer className="cast-player-controls">{session?.protocol==='DLNA'&&status.seekable&&status.duration>0&&<label className="cast-player-progress"><span>{time(seek??status.position)}</span><CastRange aria-label={t('播放进度')} min={0} max={status.duration} value={seek??status.position} onChange={e=>setSeek(+e.target.value)} onPointerUp={()=>{if(seek!==null)void control('seek',{seconds:seek}).finally(()=>setSeek(null));}} onKeyUp={()=>{if(seek!==null)void control('seek',{seconds:seek}).finally(()=>setSeek(null));}}/><span>{time(status.duration)}</span></label>}<div className="cast-player-buttons"><button aria-label={t(status.paused?'播放':'暂停')} disabled={busy||!session} onClick={()=>void control('pause',{paused:!status.paused})}>{status.paused?<IconPlayerPlay size={26}/>:<IconPlayerPause size={26}/>}</button><button aria-label={t(volume===0?'开启声音':'静音')} disabled={busy||!session} onClick={()=>{const next=volume===0?1:0;setVolume(next);void control('volume',{volume:next});}}>{volume===0?<IconVolumeOff size={24}/>:<IconVolume size={24}/>}</button><CastRange aria-label={t('音量')} min={0} max={1} step={.01} value={volume} onChange={e=>setVolume(+e.target.value)} onPointerUp={()=>void control('volume',{volume})} onKeyUp={()=>void control('volume',{volume})}/><button className="cast-player-stop" aria-label={t('停止投屏')} disabled={busy||!session} onClick={()=>void control('stop')}><IconPlayerStop size={24}/><span>{t('停止投屏')}</span></button></div></footer>
  {error&&<p className="cast-player-error" role="alert">{error}</p>}
 </main>;
}
