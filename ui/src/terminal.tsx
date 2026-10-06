import React,{useEffect,useRef,useState} from 'react';
import {Terminal} from '@xterm/xterm';
import {IconTerminal2,IconPlus,IconX,IconRefresh,IconPencil,IconLock,IconPlugConnected,IconPlugConnectedX,IconInfoCircle} from '@tabler/icons-react';
import {FitAddon} from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import {managerApi} from './manager-api';
import {t} from './i18n';
import './manager-tools.css';
import './terminal.css';
type Session={id:string;name:string;exited:boolean;controlled:boolean};
export function TerminalManager(){
 const[sessions,setSessions]=useState<Session[]>([]),[selected,setSelected]=useState(()=>sessionStorage.getItem('framely-terminal-session')??''),[error,setError]=useState(''),[connected,setConnected]=useState(false),[writable,setWritable]=useState(false),[exited,setExited]=useState(false),[truncated,setTruncated]=useState(false),[close,setClose]=useState<Session|null>(null),[name,setName]=useState(''),[renaming,setRenaming]=useState(false),[busy,setBusy]=useState(false),[loading,setLoading]=useState(true),[dimensions,setDimensions]=useState({cols:80,rows:24});
 const localPanel=['localhost','127.0.0.1','[::1]'].includes(location.hostname),reachable=()=>navigator.onLine!==false||localPanel;
 const host=useRef<HTMLDivElement>(null),socket=useRef<WebSocket|null>(null),term=useRef<Terminal|null>(null),fit=useRef<FitAddon|null>(null),reconnect=useRef(()=>{});
 useEffect(()=>{if(!renaming&&!close)return;const back=(event:Event)=>{if(event instanceof KeyboardEvent&&event.key!=='Escape')return;if(busy)return;setRenaming(false);setClose(null);};window.addEventListener('keydown',back);window.addEventListener('framely.back',back);return()=>{window.removeEventListener('keydown',back);window.removeEventListener('framely.back',back);};},[renaming,close,busy]);
 async function refresh(){try{const next=await managerApi<Session[]>('terminal',{operation:'list'});setSessions(next);setSelected(previous=>next.some(s=>s.id===previous)?previous:'');}catch(e){setError(String(e));}finally{setLoading(false);}}
 useEffect(()=>{if(!name&&selected){const active=sessions.find(s=>s.id===selected);if(active)setName(active.name);}},[sessions,selected]);
 useEffect(()=>{sessionStorage.setItem('framely-terminal-session',selected);},[selected]);
 useEffect(()=>{void refresh();const timer=setInterval(()=>void refresh(),3000);return()=>clearInterval(timer);},[]);
 async function create(){if(busy)return;setBusy(true);try{const s=await managerApi<Session>('terminal',{operation:'create',name:t('终端')});setSelected(s.id);setName(s.name);await refresh();term.current?.focus();}catch(e){setError(String(e));}finally{setBusy(false);}}
 useEffect(()=>{if(!host.current||!selected)return;const terminal=new Terminal({fontSize:16,scrollback:10000,allowTransparency:true,cursorBlink:!matchMedia('(prefers-reduced-motion: reduce)').matches,disableStdin:true,theme:{background:'#00000000',foreground:'#e8e8ed',selectionBackground:'#555555'}}),fitter=new FitAddon();terminal.loadAddon(fitter);terminal.open(host.current);term.current=terminal;fit.current=fitter;terminal.parser.registerOscHandler(52,()=>true);
  terminal.attachCustomKeyEventHandler(event=>{
   if(event.type!=='keydown'||!event.ctrlKey||!event.shiftKey||event.altKey||event.metaKey)return true;
   if(event.code==='KeyV')return false; // Native paste supplies the clipboard to xterm; do not emit Ctrl-V.
   if(event.code!=='KeyC')return true;
   event.preventDefault();const text=terminal.getSelection();if(!text)return false;
   const fallback=()=>{const field=document.createElement('textarea');field.value=text;field.setAttribute('aria-hidden','true');field.style.cssText='position:fixed;left:-10000px;top:0';document.body.appendChild(field);field.select();try{if(!document.execCommand('copy'))setError(t('无法复制，请使用浏览器的复制功能。'));}catch{setError(t('无法复制，请使用浏览器的复制功能。'));}finally{field.remove();terminal.focus();}};
   if(navigator.clipboard?.writeText)void navigator.clipboard.writeText(text).catch(fallback);else fallback();
   return false;
  });
  let alive=true,lastSeen=performance.now(),timer:ReturnType<typeof setInterval>|undefined,retry:ReturnType<typeof setTimeout>|undefined;
  const send=(v:object)=>{if(socket.current?.readyState===WebSocket.OPEN)socket.current.send(JSON.stringify(v));};
  const encode=(s:string)=>{let bytes='';for(const v of new TextEncoder().encode(s))bytes+=String.fromCharCode(v);return btoa(bytes);};
  const input=terminal.onData(data=>{if(reachable()&&performance.now()-lastSeen<3000)send({input:encode(data)});});
  const resize=()=>{try{fitter.fit();setDimensions({cols:terminal.cols,rows:terminal.rows});send({cols:terminal.cols,rows:terminal.rows});}catch{}};
  const observer=new ResizeObserver(resize);observer.observe(host.current);
  function connect(){if(!alive)return;if(timer)clearInterval(timer);if(retry)clearTimeout(retry);socket.current?.close();terminal.options.disableStdin=true;setConnected(false);setWritable(false);if(!reachable())return;lastSeen=performance.now();terminal.reset();setTruncated(false);setExited(false);const ws=new WebSocket(`${location.protocol==='https:'?'wss:':'ws:'}//${location.host}/manager-api/terminal/ws?id=${encodeURIComponent(selected)}`);socket.current=ws;
   ws.onopen=()=>{if(!alive||socket.current!==ws)return;setConnected(true);setError('');resize();timer=setInterval(()=>{if(performance.now()-lastSeen>=3000){terminal.options.disableStdin=true;setConnected(false);setWritable(false);ws.close();return;}if(ws.readyState===WebSocket.OPEN)ws.send('{}');},40);};
   ws.onmessage=e=>{if(!alive||socket.current!==ws)return;lastSeen=performance.now();try{const v=JSON.parse(e.data);setWritable(v.writable);terminal.options.disableStdin=!v.writable||v.exited;setExited(v.exited);if(v.truncated)setTruncated(true);if(v.data){const raw=atob(v.data);terminal.write(Uint8Array.from(raw,c=>c.charCodeAt(0)));}}catch(err){setError(String(err));}};
   ws.onclose=()=>{if(!alive||socket.current!==ws)return;if(timer)clearInterval(timer);setConnected(false);setWritable(false);terminal.options.disableStdin=true;retry=setTimeout(connect,2000);};ws.onerror=()=>setError(t('终端连接中断，正在重连。'));
  }
  const offline=()=>{if(localPanel)return;terminal.options.disableStdin=true;setConnected(false);setWritable(false);socket.current?.close();};window.addEventListener('offline',offline);window.addEventListener('online',connect);reconnect.current=connect;connect();return()=>{window.removeEventListener('offline',offline);window.removeEventListener('online',connect);alive=false;if(timer)clearInterval(timer);if(retry)clearTimeout(retry);observer.disconnect();input.dispose();socket.current?.close();socket.current=null;terminal.dispose();term.current=null;};
 },[selected]);
 const current=sessions.find(session=>session.id===selected);
 async function rename(){if(!name.trim()||busy)return;setBusy(true);try{await managerApi('terminal',{operation:'rename',id:selected,name:name.trim()});await refresh();setRenaming(false);}catch(e){setError(String(e));}finally{setBusy(false);}}
 async function closeSession(){if(!close||busy)return;setBusy(true);try{await managerApi('terminal',{operation:'close',id:close.id,approve:true});if(selected===close.id)setSelected('');setClose(null);await refresh();}catch(e){setError(String(e));}finally{setBusy(false);}}
 return <section className="manager-tool terminal-manager">
 <div className="terminal-tabbar"><div className="terminal-sessions" role="tablist" aria-label={t('终端会话')}>{sessions.map(session=><div className={`terminal-tab ${selected===session.id?'active':''}`} key={session.id}><button role="tab" aria-selected={selected===session.id} onClick={()=>{setSelected(session.id);setName(session.name);}}><IconTerminal2 size={17}/><span>{session.name}</span>{session.exited&&<small>{t('已结束')}</small>}</button><button className="terminal-tab-close" aria-label={t('关闭会话 {0}',{0:session.name})} onClick={()=>{setError('');setClose(session);}}><IconX size={15}/></button></div>)}</div><button className="terminal-new" disabled={busy} onClick={()=>void create()}><IconPlus size={18}/><span>{t('新建终端')}</span></button></div>
 {error&&<div className="terminal-notice error" role="alert"><IconInfoCircle size={18}/><span>{error}</span><button aria-label={t('关闭')} onClick={()=>setError('')}><IconX size={16}/></button></div>}
 {selected?<><div className="terminal-toolbar"><span className="terminal-connection" role="status">{exited?<IconTerminal2 size={16}/>:connected?(writable?<IconPlugConnected size={16}/>:<IconLock size={16}/>):<IconPlugConnectedX size={16}/>}<span>{t(exited?'已结束':connected?(writable?'可输入':'只读'):'正在连接')}</span></span><div className="terminal-controls"><button aria-label={t('重命名会话')} disabled={busy} onClick={()=>{setName(current?.name??name);setError('');setRenaming(true);}}><IconPencil size={17}/><span>{t('重命名')}</span></button><button onClick={()=>reconnect.current()}><IconRefresh size={17}/><span>{t('重新连接')}</span></button>{!writable&&connected&&!exited&&<button className="terminal-takeover" onClick={()=>socket.current?.send('{"claim":true}')}><IconLock size={17}/>{t('接管控制')}</button>}</div></div>
 {truncated&&<div className="terminal-notice"><IconInfoCircle size={18}/><span>{t('较早的终端输出已超出缓存，仅恢复保留的内容。')}</span></div>}<div className="terminal-canvas" ref={host}/><footer className="terminal-status"><span>{t('Steam 会话用户')}<small>{t('可使用 sudo 提权')}</small></span><span className="terminal-shortcuts">{t('Ctrl+Shift+C 复制 · Ctrl+Shift+V 粘贴 · Ctrl+C 中断程序')}</span><span>{dimensions.cols} × {dimensions.rows}</span></footer></>:<div className="terminal-empty"><IconTerminal2 size={42}/><h2>{t(loading?'正在读取':'选择或新建终端')}</h2><p>{t('离开此页面不会结束终端会话。')}</p></div>}
 {renaming&&<div className="modal terminal-dialog" role="dialog" aria-modal="true" aria-label={t('重命名会话')}><h2>{t('重命名会话')}</h2><label>{t('会话名称')}<input autoFocus value={name} onChange={event=>setName(event.target.value)} onKeyDown={event=>{if(event.key==='Enter')void rename();}}/></label>{error&&<p className="error">{error}</p>}<footer><button disabled={busy} onClick={()=>setRenaming(false)}>{t('取消')}</button><button className="primary" disabled={busy||!name.trim()} onClick={()=>void rename()}>{t('保存')}</button></footer></div>}
 {close&&<div className="modal terminal-dialog" role="dialog" aria-modal="true" aria-label={t('关闭终端')}><h2>{t('关闭终端')}</h2><b>{close.name}</b><p>{t('关闭会话将结束其中运行的程序。')}</p>{error&&<p className="error">{error}</p>}<footer><button disabled={busy} onClick={()=>setClose(null)}>{t('取消')}</button><button className="danger" disabled={busy} onClick={()=>void closeSession()}>{t('确认关闭')}</button></footer></div>}
 </section>;
}
