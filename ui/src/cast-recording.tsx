import React from 'react';
import {IconCircleFilled,IconPlayerStop} from '@tabler/icons-react';
import {t} from './i18n';
export type RecordingStatus={active:boolean;elapsedSeconds?:number;path?:string|null;error?:string|null};
export function RecordingButton({recording,disabled,onClick}:{recording?:RecordingStatus;disabled:boolean;onClick:()=>void}){
 const seconds=recording?.elapsedSeconds??0;
 const elapsed=`${Math.floor(seconds/60).toString().padStart(2,'0')}:${(seconds%60).toString().padStart(2,'0')}`;
 return <button className={`cast-record-button${recording?.active?' recording':''}`} disabled={disabled} onClick={onClick} title={recording?.path??'/home/steamos/Videos/framely'}>{recording?.active?<IconPlayerStop size={21}/>:<IconCircleFilled size={18}/>}<span>{t(recording?.active?'停止录制':'开始录制')}{recording?.active&&` · ${elapsed}`}</span></button>;
}
