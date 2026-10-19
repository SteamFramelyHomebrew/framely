import {useEffect,useState} from 'react';
import {IconApps,IconCamera,IconDeviceDesktop,IconPlayerRecord,IconRefresh} from '@tabler/icons-react';
import './system-radial-menu.css';

type State={progress:number;open:boolean;direction:string;recording:boolean};
const initial:State={progress:0,open:false,direction:'center',recording:false};
const slots=[
 {direction:'up',label:'启动台',icon:IconApps},
 {direction:'upRight',label:'',icon:null},
 {direction:'right',label:'截图',icon:IconCamera},
 {direction:'downRight',label:'',icon:null},
 {direction:'down',label:'管理面板',icon:IconDeviceDesktop},
 {direction:'downLeft',label:'',icon:null},
 {direction:'left',label:'录屏',icon:IconPlayerRecord},
 {direction:'upLeft',label:'',icon:null},
];

export function SystemRadialMenu({progressOnly=false}:{progressOnly?:boolean}){
 const[state,setState]=useState<State>(initial);
 useEffect(()=>{
  const update=(event:Event)=>setState((event as CustomEvent<State>).detail);
  window.addEventListener('framely.systemMenu',update);
  return()=>window.removeEventListener('framely.systemMenu',update);
 },[]);
 return <main className={'system-radial '+(progressOnly?'is-charging':'is-open')} aria-label="Framely 快捷菜单">
  {progressOnly?<div className="system-radial-charge">
   <svg className="system-radial-charge-ring" viewBox="0 0 86 86" aria-hidden="true">
    <circle className="system-radial-charge-track" cx="43" cy="43" r="31"/>
    <circle className="system-radial-charge-progress" cx="43" cy="43" r="31" pathLength="100" strokeDasharray={`${Math.max(0,Math.min(1,state.progress))*100} 100`}/>
   </svg>
  </div>:<div className="system-radial-face">
   {slots.map((slot,index)=>{
    const Icon=slot.icon;
    if(!Icon)return null;
    return <div key={slot.direction} className={'system-radial-slot'+(state.direction===slot.direction?' is-selected':'')} style={{'--angle':`${index*45}deg`} as React.CSSProperties}>
     <Icon size={29} stroke={1.7}/><span>{slot.direction==='left'&&state.recording?'结束录屏':slot.direction==='left'?'开始录屏':slot.label}</span>
    </div>;
   })}
   <div className={'system-radial-center'+(state.direction==='center'?' is-selected':'')}>
    <IconRefresh size={24} stroke={1.6}/><span>重置视角</span>
   </div>
  </div>}
 </main>;
}
