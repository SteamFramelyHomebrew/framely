import React,{useState} from 'react';
import {registerPlugin} from '../sdk/src/index';
function Page(){const[value,setValue]=useState(''),[multiline,setMultiline]=useState('');return <><input id="keyboard-input" value={value} onChange={e=>setValue(e.target.value)}/><output id="keyboard-state">{value}</output><textarea id="keyboard-multiline" value={multiline} onChange={e=>setMultiline(e.target.value)}/><output id="keyboard-multiline-state">{multiline}</output></>}
registerPlugin({QuickPage:Page});
(async()=>{try{
 const until=async(find:()=>boolean)=>{for(let i=0;i<100;i++){if(find())return;await new Promise(r=>setTimeout(r,20));}throw Error('keyboard state timeout');};
 await until(()=>!!document.getElementById('keyboard-input'));
 const input=document.getElementById('keyboard-input') as HTMLInputElement;input.focus();input.dispatchEvent(new FocusEvent('focusin',{bubbles:true}));
 const value='中文输入 😃 "quoted"';(window as any).__framelyCommitKeyboard(value);
 await until(()=>document.getElementById('keyboard-state')?.textContent===value);
 for(const edit of ['字','','替换','']){
  (window as any).__framelyCommitKeyboard(edit);
  await until(()=>document.getElementById('keyboard-state')?.textContent===edit);
 }
 const native=async(command:string,value:string,target='keyboard-state')=>{
  console.log('FRAMELY_NATIVE_KEYBOARD_'+command);
  await until(()=>document.getElementById(target)?.textContent===value);
 };
 await native('TEXT','汉😃');await native('BACKSPACE','汉');await native('BACKSPACE','');
 await native('X','x');await native('LEFT','x');await native('Y','yx');
 await native('BACKSPACE','x');await native('RIGHT','x');await native('BACKSPACE','');
 const textarea=document.getElementById('keyboard-multiline') as HTMLTextAreaElement;
 textarea.focus();textarea.dispatchEvent(new FocusEvent('focusin',{bubbles:true}));
 await native('X','x','keyboard-multiline-state');await native('ENTER','x\n','keyboard-multiline-state');
 await native('Y','x\ny','keyboard-multiline-state');
 await native('BACKSPACE','x\n','keyboard-multiline-state');await native('BACKSPACE','x','keyboard-multiline-state');await native('BACKSPACE','','keyboard-multiline-state');
 console.log('FRAMELY_KEYBOARD_REACT_PASS');
 (window as any).__framelyKeyboardFixtureDone=true;
}catch(e){console.error('FRAMELY_BRIDGE_FAIL '+e);}})();
