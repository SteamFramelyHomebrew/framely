import React,{useEffect,useRef,useState} from 'react';
import {Terminal} from '@xterm/xterm';
import {FitAddon} from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import {managerApi} from './manager-api';
import {t} from './i18n';
import './manager-tools.css';
type Session={id:string;name:string;exited:boolean;controlled:boolean};
export function TerminalManager(){
 const[sessions,setSessions]=useState<Session[]>([]),[selected,setSelected]=useState(()=>sessionStorage.getItem('framely-terminal-session')??''),[error,setError]=useState(''),[connected,setConnected]=useState(false),[writable,setWritable]=useState(false),[exited,setExited]=useState(false),[truncated,setTruncated]=useState(false),[close,setClose]=useState<Session|null>(null),[name,setName]=useState('');
 const localPanel=['localhost','127.0.0.1','[::1]'].includes(location.hostname),reachable=()=>navigator.onLine!==false||localPanel;
 const host=useRef<HTMLDivElement>(null),socket=useRef<WebSocket|null>(null),term=useRef<Terminal|null>(null),fit=useRef<FitAddon|null>(null),reconnect=useRef(()=>{});
 async function refresh(){try{const next=await managerApi<Session[]>('terminal',{operation:'list'});setSessions(next);setSelected(previous=>next.some(s=>s.id===previous)?previous:'');}catch(e){setError(String(e));}}
 useEffect(()=>{if(!name&&selected){const active=sessions.find(s=>s.id===selected);if(active)setName(active.name);}},[sessions,selected]);
 useEffect(()=>{sessionStorage.setItem('framely-terminal-session',selected);},[selected]);
 useEffect(()=>{void refresh();const timer=setInterval(()=>void refresh(),3000);return()=>clearInterval(timer);},[]);
 async function create(){try{const s=await managerApi<Session>('terminal',{operation:'create',name:t('终端')});setSelected(s.id);setName(s.name);await refresh();}catch(e){setError(String(e));}}
 useEffect(()=>{if(!host.current||!selected)return;const terminal=new Terminal({fontSize:16,scrollback:10000,cursorBlink:true,disableStdin:true,theme:{background:'#141414',foreground:'#e4e4e4',selectionBackground:'#555555'}}),fitter=new FitAddon();terminal.loadAddon(fitter);terminal.open(host.current);term.current=terminal;fit.current=fitter;terminal.parser.registerOscHandler(52,()=>true);
  let alive=true,lastSeen=performance.now(),timer:ReturnType<typeof setInterval>|undefined,retry:ReturnType<typeof setTimeout>|undefined;
  const send=(v:object)=>{if(socket.current?.readyState===WebSocket.OPEN)socket.current.send(JSON.stringify(v));};
  const encode=(s:string)=>{let bytes='';for(const v of new TextEncoder().encode(s))bytes+=String.fromCharCode(v);return btoa(bytes);};
  const input=terminal.onData(data=>{if(reachable()&&performance.now()-lastSeen<3000)send({input:encode(data)});});
  const resize=()=>{try{fitter.fit();send({cols:terminal.cols,rows:terminal.rows});}catch{}};
  const observer=new ResizeObserver(resize);observer.observe(host.current);
  function connect(){if(!alive)return;if(timer)clearInterval(timer);if(retry)clearTimeout(retry);socket.current?.close();terminal.options.disableStdin=true;setConnected(false);setWritable(false);if(!reachable())return;lastSeen=performance.now();terminal.reset();setTruncated(false);setExited(false);const ws=new WebSocket(`${location.protocol==='https:'?'wss:':'ws:'}//${location.host}/manager-api/terminal/ws?id=${encodeURIComponent(selected)}`);socket.current=ws;
   ws.onopen=()=>{if(!alive||socket.current!==ws)return;setConnected(true);setError('');resize();timer=setInterval(()=>{if(performance.now()-lastSeen>=3000){terminal.options.disableStdin=true;setConnected(false);setWritable(false);ws.close();return;}if(ws.readyState===WebSocket.OPEN)ws.send('{}');},40);};
   ws.onmessage=e=>{if(!alive||socket.current!==ws)return;lastSeen=performance.now();try{const v=JSON.parse(e.data);setWritable(v.writable);terminal.options.disableStdin=!v.writable||v.exited;setExited(v.exited);if(v.truncated)setTruncated(true);if(v.data){const raw=atob(v.data);terminal.write(Uint8Array.from(raw,c=>c.charCodeAt(0)));}}catch(err){setError(String(err));}};
   ws.onclose=()=>{if(!alive||socket.current!==ws)return;if(timer)clearInterval(timer);setConnected(false);setWritable(false);terminal.options.disableStdin=true;retry=setTimeout(connect,2000);};ws.onerror=()=>setError(t('终端连接中断，正在重连。'));
  }
  const offline=()=>{if(localPanel)return;terminal.options.disableStdin=true;setConnected(false);setWritable(false);socket.current?.close();};window.addEventListener('offline',offline);window.addEventListener('online',connect);reconnect.current=connect;connect();return()=>{window.removeEventListener('offline',offline);window.removeEventListener('online',connect);alive=false;if(timer)clearInterval(timer);if(retry)clearTimeout(retry);observer.disconnect();input.dispose();socket.current?.close();socket.current=null;terminal.dispose();term.current=null;};
 },[selected]);
 return <section className="manager-tool"><div className="list-heading"><div><h1>{t('终端')}</h1><p>{t('以 Steam 会话用户运行；可在终端内使用 sudo。')}</p></div><button onClick={()=>void create()}>{t('新建终端')}</button></div>{error&&<p className="error">{error}</p>}
 <div className="terminal-sessions">{sessions.map(s=><div key={s.id}><button className={selected===s.id?'active':''} onClick={()=>{setSelected(s.id);setName(s.name);}}>{s.name}{s.exited?' · '+t('已结束'):''}</button><button aria-label={t('关闭终端')} onClick={()=>setClose(s)}>×</button></div>)}</div>
 {selected?<><div className="terminal-toolbar"><span>{connected?(writable?t('可输入'):t('只读')):t('正在连接')}{exited?' · '+t('已结束'):''}</span><input aria-label={t('会话名称')} value={name} onChange={e=>setName(e.target.value)} onBlur={()=>{if(name.trim())void managerApi('terminal',{operation:'rename',id:selected,name}).then(refresh).catch(e=>setError(String(e)));}}/><button onClick={()=>reconnect.current()}>{t('重新连接')}</button>{!writable&&connected&&<button onClick={()=>socket.current?.send('{"claim":true}')}>{t('接管控制')}</button>}</div>{truncated&&<p className="banner">{t('较早的终端输出已超出缓存，仅恢复保留的内容。')}</p>}<div className="terminal-canvas" ref={host}/></>:<div className="empty-state"><h2>{t('选择或新建终端')}</h2><p>{t('离开此页面不会结束终端会话。')}</p></div>}
 {close&&<div className="modal" role="dialog" aria-modal="true"><h2>{t('关闭终端')}</h2>{error&&<p className="error">{error}</p>}<p>{t('关闭会话将结束其中运行的程序。')}</p><footer><button onClick={()=>setClose(null)}>{t('取消')}</button><button className="danger" onClick={()=>void managerApi('terminal',{operation:'close',id:close.id,approve:true}).then(()=>{if(selected===close.id)setSelected('');setClose(null);void refresh();}).catch(e=>setError(String(e)))}>{t('确认关闭')}</button></footer></div>}
 </section>;
}
