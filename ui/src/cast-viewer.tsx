import React,{useEffect,useRef,useState} from 'react';
import {IconArrowLeft,IconPlayerPause,IconPlayerPlay,IconVolume,IconVolumeOff,IconMaximize,IconMinimize,IconRefresh,IconAspectRatio} from '@tabler/icons-react';
import {t} from './i18n';

export function CastVideo({active,onError,controls=true,aspectRatio,videoRef,onState,showStatus=true}:{active:boolean;onError?:(error:string)=>void;controls?:boolean;aspectRatio?:number;videoRef?:React.RefObject<HTMLVideoElement|null>;onState?:(state:'connecting'|'playing'|'failed')=>void;showStatus?:boolean}){
 const localVideo=useRef<HTMLVideoElement>(null),video=videoRef??localVideo,failure=useRef(onError);failure.current=onError;
 const [state,setState]=useState<'connecting'|'playing'|'failed'>('connecting');
 useEffect(()=>{onState?.(state);},[state,onState]);
 const [ratio,setRatio]=useState(16/9),[retry,setRetry]=useState(0);
 useEffect(()=>{
  if(!active)return;
  const peer=new RTCPeerConnection(),abort=new AbortController();let resource:string|undefined,closed=false;
  setState('connecting');
  peer.addTransceiver('video',{direction:'recvonly'});peer.addTransceiver('audio',{direction:'recvonly'});
  peer.ontrack=event=>{if(video.current){const stream=video.current.srcObject as MediaStream|null??new MediaStream();stream.addTrack(event.track);video.current.srcObject=stream;void video.current.play().catch(()=>{});}};
  peer.onconnectionstatechange=()=>{if(peer.connectionState==='failed'){setState('failed');failure.current?.(t('观看连接失败，请重新连接。'));}};
  void(async()=>{
   try{
    await peer.setLocalDescription(await peer.createOffer());
    if(peer.iceGatheringState!=='complete')await new Promise<void>((resolve,reject)=>{
     const check=()=>{if(peer.iceGatheringState==='complete'){cleanup();resolve();}},cancel=()=>{cleanup();reject(new DOMException('Aborted','AbortError'));};
     const timeout=setTimeout(()=>{cleanup();resolve();},4000);
     const cleanup=()=>{clearTimeout(timeout);peer.removeEventListener('icegatheringstatechange',check);abort.signal.removeEventListener('abort',cancel);};
     peer.addEventListener('icegatheringstatechange',check);abort.signal.addEventListener('abort',cancel,{once:true});check();
    });
    const response=await fetch('/cast/whep',{method:'POST',headers:{'Content-Type':'application/sdp'},body:peer.localDescription?.sdp,signal:abort.signal});
    if(!response.ok)throw Error(await response.text()||t('串流尚未就绪，请稍后重试。'));
    resource=response.headers.get('Location')??undefined;
    if(closed){if(resource)void fetch(resource,{method:'DELETE'});return;}
    await peer.setRemoteDescription({type:'answer',sdp:await response.text()});
   }catch(error){if(!abort.signal.aborted){setState('failed');failure.current?.(String(error));}}
  })();
  return()=>{closed=true;abort.abort();peer.close();if(resource)void fetch(resource,{method:'DELETE'}).catch(()=>{});if(video.current)video.current.srcObject=null;};
 },[active,retry]);
 return <div className="cast-video-stage" style={{aspectRatio:aspectRatio??ratio}}><video ref={video} autoPlay playsInline muted controls={controls} onLoadedMetadata={()=>{if(video.current?.videoHeight)setRatio(video.current.videoWidth/video.current.videoHeight);}} onPlaying={()=>setState('playing')}/>{showStatus&&active&&state==='connecting'&&<span role="status">{t('正在连接画面…')}</span>}{showStatus&&active&&state==='failed'&&<div className="cast-video-failure"><p>{t('观看连接失败，请重新连接。')}</p><button onClick={()=>setRetry(v=>v+1)}>{t('重新连接')}</button></div>}{!active&&<span>{t('开始串流后显示头显画面。')}</span>}</div>;
}
export function CastViewer(){
 const video=useRef<HTMLVideoElement>(null),root=useRef<HTMLElement>(null),timer=useRef<ReturnType<typeof setTimeout>|null>(null);
 const [key,setKey]=useState(0),[error,setError]=useState(''),[state,setState]=useState<'connecting'|'playing'|'failed'>('connecting'),[visible,setVisible]=useState(true),[paused,setPaused]=useState(false),[muted,setMuted]=useState(true),[volume,setVolume]=useState(1),[fullscreen,setFullscreen]=useState(false),[fill,setFill]=useState(true);
 function reveal(){setVisible(true);if(timer.current)clearTimeout(timer.current);if(state==='playing'&&!paused)timer.current=setTimeout(()=>{if(!root.current?.querySelector(':focus-visible'))setVisible(false);},3000);}
 useEffect(()=>{reveal();return()=>{if(timer.current)clearTimeout(timer.current);};},[state,paused,key]);
 useEffect(()=>{const changed=()=>setFullscreen(!!document.fullscreenElement);document.addEventListener('fullscreenchange',changed);return()=>document.removeEventListener('fullscreenchange',changed);},[]);
 useEffect(()=>{const el=video.current;if(!el)return;const play=()=>setPaused(false),pause=()=>setPaused(true);el.addEventListener('play',play);el.addEventListener('pause',pause);return()=>{el.removeEventListener('play',play);el.removeEventListener('pause',pause);};},[key]);
 useEffect(()=>{if(video.current){video.current.muted=muted;video.current.volume=volume;}},[muted,volume,key,state]);
 function reconnect(){setError('');setPaused(false);setKey(v=>v+1);reveal();}
 function togglePlay(){const el=video.current;if(!el)return;if(el.paused)void el.play().catch(()=>setError(t('无法播放，请重新连接。')));else el.pause();reveal();}
 async function toggleFullscreen(){try{if(document.fullscreenElement)await document.exitFullscreen();else if(root.current?.requestFullscreen)await root.current.requestFullscreen();else (video.current as HTMLVideoElement&{webkitEnterFullscreen?:()=>void})?.webkitEnterFullscreen?.();}catch{setError(t('此浏览器暂不支持全屏。'));}reveal();}
 return <main ref={root} tabIndex={-1} className={`cast-watch ${visible||state!=='playing'||paused?'controls-visible':''} ${fill?'video-fill':''}`} onPointerMove={e=>{if(e.pointerType==='mouse')reveal();}} onClick={e=>{if(!(e.target as HTMLElement).closest('button,a,input')){reveal();}}} onFocusCapture={reveal} onBlurCapture={reveal} onKeyDown={e=>{if((e.target as HTMLElement).closest('button,a,input'))return;if(e.code==='Space'){e.preventDefault();togglePlay();}else if(e.key==='Escape')reveal();}}>
  <CastVideo key={key} active controls={false} videoRef={video} onState={setState} onError={setError} showStatus={false}/>
  <header className="cast-watch-top cast-watch-overlay"><a href="/manager#casting" className="cast-watch-icon" aria-label={t('返回投屏设置')}><IconArrowLeft size={22}/></a><div><strong>Framely</strong><span>{t('头显画面')}</span></div><span className="cast-watch-connection" role="status">{state==='playing'?t('实时画面'):state==='connecting'?t('正在连接'):t('连接已断开')}</span></header>
  {state!=='playing'&&<div className="cast-watch-message" role={state==='failed'?'alert':'status'}>{state==='connecting'?<><span className="cast-spinner"/><h1>{t('正在连接头显')}</h1><p>{t('画面即将开始，请稍候。')}</p></>:<><h1>{t('暂时无法观看')}</h1><p>{error||t('确认头显已开始串流，然后重新连接。')}</p><button className="primary" onClick={reconnect}><IconRefresh size={20}/>{t('重新连接')}</button></>}</div>}
  {error&&state==='playing'&&<p className="cast-watch-toast" role="alert">{error}</p>}
  <footer className="cast-watch-bottom cast-watch-overlay"><div className="cast-watch-controls"><button className="cast-watch-icon" aria-label={t(paused?'播放':'暂停')} disabled={state!=='playing'} onClick={togglePlay}>{paused?<IconPlayerPlay size={24}/>:<IconPlayerPause size={24}/>}</button><div className="cast-watch-sound"><button className="cast-watch-icon" aria-label={t(muted?'开启声音':'静音')} aria-pressed={!muted} onClick={()=>{setMuted(v=>!v);reveal();}}>{muted||volume===0?<IconVolumeOff size={23}/>:<IconVolume size={23}/>}</button><input aria-label={t('音量')} type="range" min={0} max={1} step={.05} value={muted?0:volume} onChange={e=>{setVolume(+e.target.value);setMuted(false);reveal();}}/></div><span className="cast-watch-live">{t('实时')}</span><div className="cast-watch-tools"><button className="cast-watch-icon" aria-label={t('重新连接')} onClick={reconnect}><IconRefresh size={22}/></button><button className="cast-watch-icon" aria-label={t(fill?'完整画面':'填满屏幕')} aria-pressed={fill} onClick={()=>{setFill(v=>!v);reveal();}}><IconAspectRatio size={23}/></button><button className="cast-watch-icon" aria-label={t(fullscreen?'退出全屏':'全屏')} onClick={()=>void toggleFullscreen()}>{fullscreen?<IconMinimize size={22}/>:<IconMaximize size={22}/>}</button></div></div></footer>
 </main>;
}
