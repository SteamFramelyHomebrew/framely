import React from 'react';import {createRoot} from 'react-dom/client';
import {FileManager} from '../ui/src/file-manager';import {configureLanguage} from '../ui/src/i18n';import {Brand} from '../ui/src/icons';import '../ui/src/style.css';
const wait=(ms=30)=>new Promise(r=>setTimeout(r,ms));const until=async(f:()=>unknown)=>{for(let i=0;i<200;i++){if(f())return;await wait();}throw Error('Timed out '+document.body.innerText.slice(-600));};
const home='/home/steamos',dest=home+'/Downloads',calls:any[]=[],tickets=new Map<string,any>();let seq=0;
let prefs:any={view:'list',folders:{},uploadConcurrency:3};
window.fetch=async(url,init)=>{
 const u=String(url);if(u==='/api')return new Response(JSON.stringify({result:true}));if(u.startsWith('/manager-api/file-upload/')){await wait(450);if(init?.signal?.aborted)throw new DOMException('cancel','AbortError');const parts=u.split('/'),offset=Number(parts.at(-1)),size=(init?.body as Blob).size;return new Response(JSON.stringify({result:{received:offset+size}}));}
 if(u!=='/manager-api/files')throw Error('Unexpected '+u);const p=JSON.parse(String(init?.body));calls.push(p);let result:any;
 if(p.operation==='preferences')result=[];
 else if(p.operation==='view.preferences'){if(p.uploadConcurrency)prefs.uploadConcurrency=p.uploadConcurrency;result=prefs;}
 else if(p.operation==='list'){const path=p.path||home;result={path,parent:'/',entries:p.hidden?[]:[{name:'Downloads',path:dest,directory:true,size:0},{name:'notes.txt',path:home+'/notes.txt',directory:false,size:123}],total:p.hidden?0:2,roots:[{name:'Home',path:home},{name:'Downloads',path:dest}]};}
 else if(p.operation==='upload.preflight')result={directory:p.directory,rootIdentity:{dev:1,ino:2},items:p.items.map((e:any)=>({name:e.name,exists:e.name.endsWith('existing.txt')}))};
 else if(p.operation==='upload.directory')result={status:'done',path:p.directory+'/'+p.name};
 else if(p.operation==='upload.start'){if(p.conflict==='skip'&&p.name.endsWith('existing.txt'))result={status:'skipped',path:p.directory+'/'+p.name};else{const id=String(++seq);tickets.set(id,p);result={id,chunkSize:1024*1024};}}
 else if(p.operation==='upload.finish'){const item=tickets.get(p.id);result={status:'done',path:item.directory+'/'+item.name};tickets.delete(p.id);}
 else if(p.operation==='upload.cancel'){tickets.delete(p.id);result=true;}
 else throw Error('Unexpected operation '+p.operation);
 return new Response(JSON.stringify({result}));
};
const root=createRoot(document.getElementById('root')!);const render=(lang:string)=>{configureLanguage(lang,[]);root.render(<div className="app manager"><header className="shell-header"><Brand wordmark/><span>{lang==='zh-CN'?'文件':'Files'}</span></header><div className="workspace tools-workspace"><main className="content file-content"><div className="file-workspace-frame"><FileManager key={lang}/></div></main></div></div>);};
function drop(target:Element,folder=false){const data=new DataTransfer();data.items.add(new File(['fixture'],'existing.txt'));if(folder){const entry={name:'Project',isDirectory:true,createReader:()=>{let read=false;return {readEntries:(done:any)=>{if(read)return done([]);read=true;done([{name:'empty',isDirectory:true,createReader:()=>({readEntries:(done:any)=>done([])})},{name:'existing.txt',isFile:true,file:(done:any)=>done(new File(['fixture'],'existing.txt'))},{name:'assets.bin',isFile:true,file:(done:any)=>done(new File([new Uint8Array(8*1024*1024)],'assets.bin'))}]);}};}};Object.defineProperty(data,'items',{value:[{kind:'file',webkitGetAsEntry:()=>entry}]});}target.dispatchEvent(new DragEvent('dragover',{dataTransfer:data,bubbles:true,cancelable:true}));target.dispatchEvent(new DragEvent('drop',{dataTransfer:data,bubbles:true,cancelable:true}));}
const modal=()=>document.querySelector('.modal');const click=(label:string,parent:ParentNode=document)=>{const button=[...parent.querySelectorAll<HTMLButtonElement>('button')].find(b=>b.textContent?.trim()===label);if(!button)throw Error('No button '+label);button.click();};
(window as any).runInstallReviewChecks=async()=>{try{
 for(const lang of ['en-US','zh-CN']){
  render(lang);await until(()=>document.querySelector('.file-list-scroll')?.getAttribute('aria-busy')==='false');
  // Files do not fall through to the blank area's destination.
  drop(document.querySelector('[data-file-drop-block]')!);await wait(100);if(modal())throw Error('File accepted drop');
  for(const selector of ['.file-sidebar [data-file-drop-directory]','.file-tree-entry [data-file-drop-directory]','.file-breadcrumbs [data-file-drop-directory]','.file-list-scroll']){
   const target=document.querySelector<HTMLElement>(selector)!;drop(target);await until(modal);if(modal()!.querySelector('code')?.textContent!==target.dataset.fileDropDirectory)throw Error('Drop target mismatch');click(lang==='en-US'?'Cancel':'取消',modal()!);await until(()=>!modal());
  }
  const folder=document.querySelector<HTMLElement>('.file-table [data-file-drop-directory]')!;drop(folder,true);await until(modal);
  if(modal()!.querySelector('code')?.textContent!==dest)throw Error('Folder target mismatch');
  if(calls.some(p=>p.operation==='upload.start')&&lang==='en-US')throw Error('Upload before confirmation');
  await wait(1400);console.log('FRAMELY_PREVIEW_INSTALL_UPLOAD_CONFIRM_'+(lang==='en-US'?'EN':'ZH'));await wait(200);
  click(lang==='en-US'?'Confirm':'确认',modal()!);await until(()=>document.querySelector('.file-upload-task.uploading'));
  if(document.querySelector<HTMLButtonElement>('.file-command-bar button')?.disabled)throw Error('Uploads lock workspace');
  await wait(1000);console.log('FRAMELY_PREVIEW_INSTALL_UPLOAD_QUEUE_'+(lang==='en-US'?'EN':'ZH'));await wait(250);
  await until(()=>document.querySelectorAll('.file-upload-task.done').length===4);
  const uploads=calls.filter(p=>p.operation==='upload.start');if(uploads.some(p=>p.directory!==dest))throw Error('Upload destination changed');
 }
 console.log('FRAMELY_BRIDGE_PASS');
 }catch(e){console.error('FRAMELY_BRIDGE_FAIL '+String(e)+' '+(e as Error).stack);}};
console.log('FRAMELY_VIEW_READY');
