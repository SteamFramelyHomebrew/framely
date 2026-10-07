import React,{useEffect,useRef} from 'react';
import {IconArrowRight,IconDeviceDesktop,IconDeviceDesktopShare,IconX} from '@tabler/icons-react';
import {PluginImage} from './store';
import {t} from './i18n';
export type RemoteLaunchTarget={client:string;name:string};
export function SteamLaunchChoice({name,icon,local,targets,onChoose,onClose}:{name:string;icon?:string|null;local:boolean;targets:RemoteLaunchTarget[];onChoose:(client?:string)=>void;onClose:()=>void}){
 const dialog=useRef<HTMLDivElement>(null);
 useEffect(()=>{const previous=document.activeElement as HTMLElement|null;dialog.current?.querySelector<HTMLButtonElement>('.launch-choice-target')?.focus();return()=>previous?.focus();},[]);
 return <div className="modal-backdrop" onPointerDown={e=>e.stopPropagation()} onClick={e=>e.stopPropagation()}><div ref={dialog} className="modal launch-choice" role="dialog" aria-modal="true" aria-labelledby="launch-choice-title" onKeyDown={e=>{if(e.key!=='Tab')return;const buttons=Array.from(dialog.current?.querySelectorAll<HTMLButtonElement>('button')??[]);const first=buttons[0],last=buttons.at(-1);if(e.shiftKey&&document.activeElement===first){e.preventDefault();last?.focus();}else if(!e.shiftKey&&document.activeElement===last){e.preventDefault();first?.focus();}}}>
  <header className="launch-choice-heading"><PluginImage kind="steam" src={icon} name={name} size={56}/><div><h2 id="launch-choice-title">{name}</h2><p>{t('选择启动方式')}</p></div><button className="icon-button" aria-label={t('取消')} onClick={onClose}><IconX size={22}/></button></header>
  <div className="launch-choice-targets">{local&&<button className="launch-choice-target" onClick={()=>onChoose()}><span className="launch-choice-symbol"><IconDeviceDesktop size={26} stroke={1.6}/></span><span className="launch-choice-copy"><b>{t('本机启动')}</b><small>{t('在此 Frame 上运行')}</small></span><IconArrowRight className="launch-choice-arrow" size={22} stroke={1.6}/></button>}{targets.map(target=><button key={target.client} className="launch-choice-target" onClick={()=>onChoose(target.client)}><span className="launch-choice-symbol"><IconDeviceDesktopShare size={26} stroke={1.6}/></span><span className="launch-choice-copy"><b>{target.name||t('远程设备 {0}',{0:target.client})}</b><small>{t('远程畅玩')}</small></span><IconArrowRight className="launch-choice-arrow" size={22} stroke={1.6}/></button>)}</div>
 </div></div>;
}
