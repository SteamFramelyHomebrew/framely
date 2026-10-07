import React,{useEffect,useId,useState} from 'react';
import {t} from './i18n';
export function GamepadTriggerSetting({value,busy,onSave}:{value:number;busy:boolean;onSave:(value:number)=>void}){
 const[draft,setDraft]=useState(value);const id=useId();
 useEffect(()=>setDraft(value),[value]);
 return <div className="apk-trigger-setting"><div><label htmlFor={id}><b>{t('L2 / R2 按键触发行程')}</b></label><small id={`${id}-description`}>{t('默认 80%。达到此行程才触发按键，模拟轴保持完整行程；保存后立即生效。')}</small></div><div className="apk-trigger-controls"><div><input id={id} type="range" min={1} max={100} step={1} value={draft} disabled={busy} aria-describedby={`${id}-description`} aria-valuetext={`${draft}%`} onChange={e=>setDraft(Number(e.target.value))}/><output htmlFor={id}>{draft}%</output></div><button disabled={busy||draft===value} onClick={()=>onSave(draft)}>{t('保存')}</button></div></div>;
}
