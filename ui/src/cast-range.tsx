import React,{useLayoutEffect,useRef} from 'react';
export function updateRangeFill(input:HTMLInputElement){const min=Number(input.min)||0,max=Number(input.max)||100;input.style.setProperty('--range-fill',`${max>min?Math.max(0,Math.min(100,(Number(input.value)-min)/(max-min)*100)):0}%`);}
export function CastRange({ref:outerRef,className='',onInput,...props}:React.InputHTMLAttributes<HTMLInputElement>&{ref?:React.RefObject<HTMLInputElement|null>}){
 const local=useRef<HTMLInputElement>(null),ref=outerRef??local;
 useLayoutEffect(()=>{if(ref.current)updateRangeFill(ref.current);},[props.value,props.defaultValue,props.min,props.max]);
 return <input {...props} ref={ref} type="range" className={`cast-range ${className}`} onInput={e=>{updateRangeFill(e.currentTarget);onInput?.(e);}}/>;
}
