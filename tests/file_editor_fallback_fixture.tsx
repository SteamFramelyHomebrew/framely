import React,{useState} from 'react';
import {createRoot} from 'react-dom/client';
import {FileCodeEditor} from '../ui/src/file-code-editor';
import {configureLanguage} from '../ui/src/i18n';
import {installKeyboard} from '../sdk/src/keyboard';
import '../ui/src/style.css';
import '../ui/src/file-manager.css';
configureLanguage('en-US',[]);installKeyboard(async()=>true);
let attempts=0;
const append=document.head.append.bind(document.head);
document.head.append=(...nodes:(Node|string)[])=>{const script=nodes.find(n=>n instanceof HTMLScriptElement) as HTMLScriptElement|undefined;if(script?.src.endsWith('/assets/code-editor.js')&&attempts++===0){setTimeout(()=>script.dispatchEvent(new Event('error')),20);return;}append(...nodes);};
function Fixture(){const [value,setValue]=useState('original'),[readOnly,setReadOnly]=useState(false);return <div className="app"><div className="file-manager"><h1>Editor fallback test</h1><FileCodeEditor path="test.ts" value={value} onChange={setValue} readOnly={readOnly} onSave={()=>{}}/><output id="fallback-state">{value}</output><button onClick={()=>setReadOnly(true)}>Read-only</button></div></div>;}
createRoot(document.getElementById('root')!).render(<Fixture/>);
const wait=(ms=40)=>new Promise(r=>setTimeout(r,ms));
const until=async(f:()=>unknown)=>{for(let n=0;n<150;n++){if(f())return;await wait();}throw Error('Timeout '+document.body.innerText);};
(window as any).runInstallReviewChecks=async()=>{try{
 await until(()=>document.querySelector('textarea'));
 const fallback=document.querySelector('textarea')!;fallback.focus();(window as any).__framelyCommitKeyboard('汉😃\nline');await until(()=>document.querySelector('#fallback-state')?.textContent==='汉😃\nline');
 fallback.setSelectionRange(fallback.value.length,fallback.value.length);
 (window as any).__framelyEditKeyboard('\b');await until(()=>document.querySelector('#fallback-state')?.textContent==='汉😃\nlin');
 [...document.querySelectorAll('button')].find(b=>b.textContent==='Retry')!.click();await until(()=>document.querySelector('.cm-content[contenteditable=true]'));
 if(attempts!==2||document.querySelector('textarea')||!document.querySelector('.cm-content')!.textContent?.includes('lin'))throw Error('Retry lost fallback content');
 const content=document.querySelector<HTMLElement>('.cm-content')!;content.focus();await wait();(window as any).__framelyEditKeyboard('中文');await wait();
 if(!document.querySelector('#fallback-state')?.textContent?.includes('中文'))throw Error('Native editor input failed after retry');
 [...document.querySelectorAll('button')].find(b=>b.textContent==='Read-only')!.click();await until(()=>content.contentEditable==='false');const previous=document.querySelector('#fallback-state')?.textContent;
 (window as any).__framelyEditKeyboard('not allowed');await wait();if(document.querySelector('#fallback-state')?.textContent!==previous)throw Error('Read-only editor accepted native input');
 console.log('FRAMELY_BRIDGE_PASS');
 }catch(error){console.error('FRAMELY_BRIDGE_FAIL '+error+' '+(error as Error).stack);}
};
console.log('FRAMELY_VIEW_READY');
