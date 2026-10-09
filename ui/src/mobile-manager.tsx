import React,{useEffect,useRef,useState} from 'react';
import {IconChevronDown,IconCheck,IconX} from '@tabler/icons-react';
import {t} from './i18n';
import type {ManagerSection} from './manager-switcher';
export function useMobileManager(){
 const [mobile,setMobile]=useState(()=>matchMedia('(max-width: 767px)').matches);
 useEffect(()=>{const query=matchMedia('(max-width: 767px)'),update=()=>setMobile(query.matches);query.addEventListener('change',update);return()=>query.removeEventListener('change',update);},[]);return mobile;
}
export function MobileSheet({title,onClose,children}:{title:string;onClose:()=>void;children:React.ReactNode}){
 const ref=useRef<HTMLDivElement>(null),closeRef=useRef(onClose);closeRef.current=onClose;
 useEffect(()=>{const previous=document.activeElement as HTMLElement|null;const items=()=>Array.from(ref.current?.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled),textarea,[tabindex="0"]')??[]).filter(el=>el.getClientRects().length);items()[0]?.focus();
 const key=(e:KeyboardEvent)=>{if(e.key==='Escape'){if((e.target as HTMLElement)?.closest('.framely-select')?.querySelector('[aria-expanded="true"]'))return;e.preventDefault();e.stopPropagation();closeRef.current();}if(e.key==='Tab'){const list=items(),first=list[0],last=list.at(-1);if(e.shiftKey&&document.activeElement===first){e.preventDefault();last?.focus();}else if(!e.shiftKey&&document.activeElement===last){e.preventDefault();first?.focus();}}};document.addEventListener('keydown',key,true);return()=>{document.removeEventListener('keydown',key,true);if(previous?.isConnected&&!document.querySelector('.modal[aria-modal="true"]'))previous.focus();};},[]);
 return <div className="mobile-sheet-backdrop" onClick={e=>{if(e.target===e.currentTarget)onClose();}}><div className="mobile-sheet" ref={ref} role="dialog" aria-modal="true" aria-label={title}><header><h2>{title}</h2><button aria-label={t('关闭')} onClick={onClose}><IconX size={22}/></button></header><div className="mobile-sheet-body">{children}</div></div></div>;
}
export const managerPages=(section:ManagerSection):[string,string,string][]=>section==='settings'?[['settings',t('通用'),'settings'],['launcher-settings',t('启动台'),'launcher'],['updates',t('更新'),'refresh'],['about',t('关于'),'info']]:section==='apk'?[['apk',t('应用'),'download'],['apk-containers',t('容器'),'plugins'],['apk-cleanup',t('数据清理'),'settings'],['apk-settings',t('设置'),'settings']]:section==='casting'?[['casting',t('网页观看'),'connection'],['casting-picture',t('画面与声音'),'settings'],['casting-receive',t('接收投屏'),'download'],['casting-devices',t('投到设备'),'connection']]:section==='plugins'?[['installed',t('已安装'),'plugins'],['catalog',t('插件库'),'download'],['sources',t('插件源导航'),'link'],['notification-settings',t('通知设置导航'),'bell']]:[];
export function MobilePagePicker({section,tab,onChange}:{section:ManagerSection;tab:string;onChange:(tab:string)=>void}){
 const [open,setOpen]=useState(false),pages=managerPages(section),label=pages.find(p=>p[0]===tab)?.[1]??t(section==='terminal'?'终端':section==='casting'?'投屏':'文件');
 useEffect(()=>setOpen(false),[section,tab]);
 return <div className="mobile-page-picker"><button disabled={!pages.length} aria-expanded={open} onClick={()=>setOpen(v=>!v)}>{label}{pages.length>0&&<IconChevronDown size={18}/>}</button>{open&&<MobileSheet title={t('选择页面')} onClose={()=>setOpen(false)}><div className="mobile-page-options">{pages.map(([key,label])=><button key={key} aria-current={tab===key?'page':undefined} onClick={()=>{onChange(key);setOpen(false);}}><span>{label}</span>{tab===key&&<IconCheck size={20}/>}</button>)}</div></MobileSheet>}</div>;
}
export function useMobileViewport(enabled:boolean){
 useEffect(()=>{if(!enabled)return;const viewport=window.visualViewport;const update=()=>{const keyboard=window.innerHeight-(viewport?.height??window.innerHeight)>140;document.documentElement.style.setProperty('--manager-mobile-height',`${viewport?.height??window.innerHeight}px`);document.documentElement.classList.toggle('manager-keyboard-open',keyboard);};update();viewport?.addEventListener('resize',update);window.addEventListener('resize',update);return()=>{viewport?.removeEventListener('resize',update);window.removeEventListener('resize',update);document.documentElement.style.removeProperty('--manager-mobile-height');document.documentElement.classList.remove('manager-keyboard-open');};},[enabled]);
}
