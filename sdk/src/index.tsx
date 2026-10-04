export * from './visibility';
import {createVisibilityApi, type UiVisibility} from './visibility';
export * from './lifecycle';
import {installScrollbars} from './scrollbars';
import React from 'react';
import {createRoot} from 'react-dom/client';
export interface NotificationAction {id:string;label:string;icon:string}
export interface Notification {id:string;title:string;body:string;image?:string;actions?:NotificationAction[];durationMs?:number}
type Bridge={request:(op:string,p?:unknown)=>Promise<any>;subscribe:(f:(e:unknown)=>void)=>()=>void};
function bridge():Bridge{const b=(window as any).__framelyBridge;if(!b)throw new Error('This page requires the Framely plugin host');return b;}
export const framely={
 ui:{...createVisibilityApi(bridge),close:():Promise<unknown>=>bridge().request('ui.close')},
 language:{get:():Promise<{preference:string;language:string}>=>bridge().request('language.get')},
 call:<T=unknown>(method:string,params:unknown={}):Promise<T>=>bridge().request('call',{method,params}),
 windows:{open:(window:string)=>bridge().request('window.open',{window}),close:(window:string)=>bridge().request('window.close',{window})},
 dependencies:():Promise<DependencyStatus[]>=>bridge().request('dependencies'),
 notifications:{send:(notification:Notification)=>bridge().request('notification.send',{notification}),remove:(id:string)=>bridge().request('notification.remove',{id})},
 onEvent:(callback:(event:unknown)=>void)=>bridge().subscribe(callback),
};
export function registerPlugin(pages:{QuickPage:React.ComponentType;WindowPage?:React.ComponentType;windows?:Record<string,React.ComponentType>}){installScrollbars();const key=location.pathname.split('/').pop();const Page=key==='quick'?pages.QuickPage:pages.windows?.[key??'']??pages.WindowPage??pages.QuickPage;const style=document.createElement('style');style.textContent='*{box-sizing:border-box}body{margin:0;padding:20px;background:#101820;color:#edf2fa;font:20px system-ui}button,input,textarea{font:inherit;color:inherit;border:1px solid #52627a;border-radius:6px;padding:12px;background:#24333e;margin:6px 6px 6px 0;min-height:48px}button{cursor:pointer}button:disabled{opacity:.5}input,textarea{width:100%}p{line-height:1.6}small{color:#acb9cf}';document.head.append(style);createRoot(document.getElementById('root')!).render(<Boundary><Page/></Boundary>);console.log('FRAMELY_PLUGIN_READY');}
class Boundary extends React.Component<React.PropsWithChildren,{error:string|null}>{state={error:null as string|null};static getDerivedStateFromError(e:Error){return {error:e.message}}render(){return this.state.error?<p role="alert">插件页面错误：{this.state.error}</p>:this.props.children;}}
export function Button(props:React.ButtonHTMLAttributes<HTMLButtonElement>){return <button {...props}/>;}
export function Section({title,children}:{title:string;children:React.ReactNode}){return <section style={{marginBottom:24}}><h2 style={{fontSize:24}}>{title}</h2>{children}</section>;}

export function Toggle({label,description,checked,onChange,disabled=false}:{label:string;description?:string;checked:boolean;onChange:(value:boolean)=>void;disabled?:boolean}){return <label style={{display:'flex',alignItems:'center',justifyContent:'space-between',gap:16,padding:'16px 0'}}><span>{label}{description&&<small style={{display:'block'}}>{description}</small>}</span><input type="checkbox" role="switch" checked={checked} disabled={disabled} onChange={e=>onChange(e.target.checked)} style={{width:28,height:28,flex:'none'}}/></label>}
export function Slider({label,value,onChange,min=0,max=100,step=1,disabled=false}:{label:string;value:number;onChange:(value:number)=>void;min?:number;max?:number;step?:number;disabled?:boolean}){return <label style={{display:'block',padding:'14px 0'}}>{label} <output>{value}</output><input type="range" min={min} max={max} step={step} value={value} disabled={disabled} onChange={e=>onChange(+e.target.value)}/></label>}
export function TextField({label,value,onChange,multiline=false,password=false,placeholder,disabled=false}:{label:string;value:string;onChange:(value:string)=>void;multiline?:boolean;password?:boolean;placeholder?:string;disabled?:boolean}){return <label style={{display:'block',padding:'14px 0'}}>{label}{multiline?<textarea value={value} onChange={e=>onChange(e.target.value)} placeholder={placeholder} disabled={disabled}/>:<input type={password?'password':'text'} value={value} onChange={e=>onChange(e.target.value)} placeholder={placeholder} disabled={disabled}/>}</label>}
type SelectProps={emptyLabel?:string;clearLabel?:string;label:string;options:{value:string;label:string}[];disabled?:boolean}&({multiple?:false;value:string;onChange:(value:string)=>void}|{multiple:true;value:string[];onChange:(value:string[])=>void});
export function Select(props:SelectProps){
 const{label,value,options,disabled=false}=props;
 const chosen=(option:string)=>Array.isArray(value)?value.includes(option):value===option;
 const choose=(option:string)=>{if(props.multiple){props.onChange(props.value.includes(option)?props.value.filter(v=>v!==option):[...props.value,option]);}else{props.onChange(option);setOpen(false);}};
 const[open,setOpen]=React.useState(false);
 const[placement,setPlacement]=React.useState<React.CSSProperties>({left:0,maxWidth:'calc(100vw - 24px)',minWidth:'100%'});
 const ref=React.useRef<HTMLDivElement>(null),menu=React.useRef<HTMLDivElement>(null);
 React.useLayoutEffect(()=>{
  if(!open)return;
  const place=()=>{
   if(!ref.current||!menu.current)return;
   const anchor=ref.current.getBoundingClientRect();let left=12,right=window.innerWidth-12;
   for(let parent=ref.current.parentElement;parent;parent=parent.parentElement){
    if(getComputedStyle(parent).overflowX!=='visible'){
     const bounds=parent.getBoundingClientRect();left=Math.max(left,bounds.left+4);right=Math.min(right,bounds.right-4);
    }
   }
   const available=Math.max(1,right-left),width=Math.min(menu.current.getBoundingClientRect().width,available);
   setPlacement({left:Math.max(left,Math.min(anchor.left,right-width))-anchor.left,maxWidth:available,minWidth:Math.min(anchor.width,available)});
  };
  place();window.addEventListener('resize',place);return()=>window.removeEventListener('resize',place);
 },[open,options]);
 React.useEffect(()=>{if(!open)return;const close=(e:PointerEvent)=>{if(!ref.current?.contains(e.target as Node))setOpen(false);};document.addEventListener('pointerdown',close);return()=>document.removeEventListener('pointerdown',close);},[open]);
 return <div ref={ref} className="framely-select" style={{position:'relative'}} onKeyDown={e=>{if(e.key==='Escape')setOpen(false);}}><button type="button" aria-label={label} aria-haspopup="listbox" aria-expanded={open} disabled={disabled} onClick={()=>{setPlacement({left:0,maxWidth:'calc(100vw - 24px)',minWidth:'100%'});setOpen(!open);}} style={{width:'100%'}}>{props.multiple?(props.value.length?`${label}（${props.value.length}）`:(props.emptyLabel??`全部${label}`)):(options.find(o=>o.value===value)?.label??value)} ▾</button>{open&&<div ref={menu} role="listbox" aria-label={label} aria-multiselectable={props.multiple||undefined} style={{position:'absolute',top:'100%',width:'max-content',...placement,zIndex:10,background:'#18232d',border:'1px solid #536471',padding:4,maxHeight:300,overflowY:'auto'}}>{props.multiple&&props.value.length>0&&<button type="button" onClick={()=>props.onChange([])} style={{display:'block',width:'100%',textAlign:'left'}}>{props.clearLabel??'清除选择'}</button>}{options.map(o=><button type="button" key={o.value} role="option" aria-selected={chosen(o.value)} onClick={()=>choose(o.value)} style={{display:'block',width:'100%',textAlign:'left',overflowWrap:'anywhere',background:chosen(o.value)?'#29495d':undefined}}>{props.multiple&&<span aria-hidden="true" style={{display:'inline-block',width:24}}>{chosen(o.value)?'✓':''}</span>}{o.label}</button>)}</div>}</div>;
}

export function Notice({children,error=false}:{children:React.ReactNode;error?:boolean}){return <div role={error?'alert':'status'} style={{padding:16,borderRadius:8,background:error?'#442b33':'#1e354b',color:error?'#ffc1c6':'#b6dfff'}}>{children}</div>}
export function Tabs({value,onChange,tabs}:{value:string;onChange:(value:string)=>void;tabs:{id:string;label:string}[]}){return <div role="tablist" style={{display:'flex',gap:8}}>{tabs.map(t=><button key={t.id} role="tab" aria-selected={value===t.id} onClick={()=>onChange(t.id)} style={{flex:1,background:value===t.id?'#245675':undefined}}>{t.label}</button>)}</div>}
export function usePluginEvent(callback:(event:unknown)=>void){const ref=React.useRef(callback);ref.current=callback;React.useEffect(()=>framely.onEvent(event=>ref.current(event)),[]);}
export function useBackend<T=unknown>(method:string,params:unknown={}){const[data,setData]=React.useState<T|null>(null),[error,setError]=React.useState<string|null>(null),[loading,setLoading]=React.useState(false);const active=React.useRef(true);React.useEffect(()=>{active.current=true;return()=>{active.current=false;}},[]);const call=React.useCallback(async()=>{setLoading(true);setError(null);try{const result=await framely.call<T>(method,params);if(active.current)setData(result);return result;}catch(e){if(active.current)setError(String(e));throw e;}finally{if(active.current)setLoading(false);}},[method,JSON.stringify(params)]);return{data,error,loading,call};}

export interface DependencyStatus {id:string;required:boolean;constraint:string|{version:string;source:string};version:string|null;enabled:boolean;matches:boolean;available:boolean;state?:{phase:string}|null}
export function useDependencies(){const [items,setItems]=React.useState<DependencyStatus[]>([]),[error,setError]=React.useState<string|null>(null);React.useEffect(()=>{let live=true;const load=()=>{framely.dependencies().then(v=>{if(live){setItems(v);setError(null);}}).catch(e=>{if(live)setError(String(e));});};load();const off=framely.onEvent(e=>{if((e as any)?.type==='dependencies.changed')load();});return()=>{live=false;off();};},[]);return {items,error};}

export function useVisibility(){const[state,setState]=React.useState<UiVisibility>({known:false,captureObscured:false,pageVisible:false,sequence:0});const[error,setError]=React.useState<unknown>(null);React.useEffect(()=>framely.ui.onVisibilityChanged(value=>{setState(value);setError(null);},setError),[]);return{...state,error};}
