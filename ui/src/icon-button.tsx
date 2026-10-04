import React,{useId} from 'react';
import {Icon,IconName} from './icons';

export function TooltipIconButton({label,icon,className='',...props}:React.ButtonHTMLAttributes<HTMLButtonElement>&{label:string;icon:IconName}){
 const id=useId();
 return <span className="tooltip-button"><button type="button" {...props} className={`source-icon-button ${className}`} aria-label={label} aria-describedby={id}><Icon name={icon}/></button><span id={id} role="tooltip" className="button-tooltip">{label}</span></span>;
}
