import React from 'react';
export type SwitchProps={checked:boolean;label:string;disabled?:boolean;onChange:(checked:boolean)=>void;className?:string};
export function Switch({checked,label,disabled=false,onChange,className=''}:SwitchProps){
 return <button type="button" className={`ui-switch ${checked?'on':''} ${className}`} role="switch" aria-checked={checked} aria-label={label} disabled={disabled} onClick={()=>onChange(!checked)}><span aria-hidden="true"/></button>;
}
export function SwitchRow({description,...props}:SwitchProps&{description?:string}){
 return <div className="switch-row"><div><b>{props.label}</b>{description&&<small>{description}</small>}</div><Switch {...props}/></div>;
}
