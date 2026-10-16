import React,{useEffect,useRef,useState} from 'react';
import {IconArrowBackUp,IconArrowForwardUp,IconSearch} from '@tabler/icons-react';
import type {mount,MountedCodeEditor} from './file-code-engine';
import {t,currentLanguage} from './i18n';
import {Select} from './localized-select';
declare global {interface Window {FramelyCodeEditor?:{mount:typeof mount;languageForPath?:(path:string)=>string};}}
let loading:Promise<void>|null=null;
function loadEditor(){
 if(window.FramelyCodeEditor)return Promise.resolve();
 if(!loading)loading=new Promise<void>((resolve,reject)=>{const script=document.createElement('script');script.src='/assets/code-editor.js';script.onload=()=>{if(window.FramelyCodeEditor)resolve();else{script.remove();loading=null;reject(Error('Code editor failed to initialize'));}};script.onerror=()=>{script.remove();loading=null;reject(Error('Code editor failed to load'));};document.head.append(script);});
 return loading;
}
export function FileCodeEditor({path,value,readOnly,onChange,onSave}:{path:string;value:string;readOnly:boolean;onChange:(text:string)=>void;onSave:()=>void}){
 const parent=useRef<HTMLDivElement>(null),editor=useRef<MountedCodeEditor|null>(null),position=useRef<HTMLOutputElement>(null),latest=useRef({value,readOnly,onChange,onSave});latest.current={value,readOnly,onChange,onSave};
 const [ready,setReady]=useState(false),[error,setError]=useState(''),[wrap,setWrap]=useState(false),[attempt,setAttempt]=useState(0);
 const [language,setLanguage]=useState('text');
 const languages=[{value:'text',label:t('纯文本')},{value:'javascript',label:'JavaScript'},{value:'typescript',label:'TypeScript'},{value:'json',label:'JSON'},{value:'html',label:'HTML'},{value:'css',label:'CSS'},{value:'python',label:'Python'},{value:'rust',label:'Rust'}];
 useEffect(()=>{let alive=true;setReady(false);setError('');void loadEditor().then(()=>{
  if(!alive||!parent.current)return;
  const phrases:Record<string,string>=currentLanguage().startsWith('zh')?{Find:'查找',Replace:'替换',next:'下一个',previous:'上一个',all:'全部','match case':'区分大小写','by word':'全词匹配',regexp:'正则表达式',replace:'替换','replace all':'全部替换',close:'关闭','go to line':'跳转到行',go:'跳转','Toggle replace':'展开或收起替换','Invalid regular expression':'正则表达式无效','No results':'没有匹配结果','Read-only document':'只读文档，无法替换'}:{};
  setLanguage(window.FramelyCodeEditor!.languageForPath?.(path)??'text');
  editor.current=window.FramelyCodeEditor!.mount({parent:parent.current,path,value:latest.current.value,readOnly:latest.current.readOnly,label:t('文本内容'),phrases,onChange:text=>latest.current.onChange(text),onSave:()=>latest.current.onSave(),onCursor:status=>{if(position.current)position.current.textContent=t('行 {0}，列 {1}',{0:status.line,1:status.column})+(status.selected?` / ${t('选中 {0}',{0:status.selected})}`:'');}});setReady(true);
 }).catch(e=>{if(alive)setError(String(e));});return()=>{alive=false;editor.current?.destroy();editor.current=null;};},[path,attempt]);
 useEffect(()=>{editor.current?.setValue(value);},[value,ready]);
 useEffect(()=>{editor.current?.setReadOnly(readOnly);},[readOnly,ready]);
 useEffect(()=>{editor.current?.wrap(wrap);},[wrap,ready]);
 useEffect(()=>{editor.current?.setLanguage(language);},[language,ready]);
 return <div className="file-code-editor" onKeyDownCapture={e=>{if(e.key==='Escape'&&editor.current?.closeSearch())e.stopPropagation();}} onKeyDown={e=>{if(e.key==='Escape'&&(e.target as HTMLElement).closest('.framely-select'))e.stopPropagation();}}>
  <div className="file-code-toolbar"><div className="file-code-history"><button disabled={!ready||readOnly} onClick={()=>editor.current?.undo()} aria-label={t('撤销')} title={t('撤销')}><IconArrowBackUp size={18}/></button><button disabled={!ready||readOnly} onClick={()=>editor.current?.redo()} aria-label={t('重做')} title={t('重做')}><IconArrowForwardUp size={18}/></button></div><button disabled={!ready} onClick={()=>editor.current?.find()}><IconSearch size={18}/>{t('查找替换')}</button><button disabled={!ready} aria-pressed={wrap} onClick={()=>setWrap(v=>!v)}>{t('自动换行')}</button><Select label={t('语言模式')} options={languages} value={language} disabled={!ready} onChange={setLanguage}/></div>
  {!ready&&!error&&<p role="status">{t('正在加载代码编辑器…')}</p>}
  {error&&<div className="file-code-fallback" role="alert"><div><p>{t('代码编辑器加载失败，可以使用普通文本编辑。')}</p><button onClick={()=>setAttempt(v=>v+1)}>{t('重试')}</button></div><textarea aria-label={t('文本内容')} className="file-editor" readOnly={readOnly} value={value} onChange={e=>onChange(e.target.value)}/></div>}
  <div className="file-code-surface file-editor" ref={parent} hidden={!ready}/>
  <div className="file-editor-status"><span>{readOnly?t('只读'):t('可编辑')}</span><output ref={position}>{t('行 {0}，列 {1}',{0:1,1:1})}</output><span>UTF-8</span><span>{value.includes('\r\n')?'CRLF':value.includes('\r')&&!value.includes('\n')?'CR':'LF'}</span><span className="file-editor-indent">{t('缩进：{0} 个空格',{0:2})}</span></div>
 </div>;
}
