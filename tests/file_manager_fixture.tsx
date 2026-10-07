import React from 'react';
import {createRoot} from 'react-dom/client';
import {FileManager} from '../ui/src/file-manager';
import {configureLanguage} from '../ui/src/i18n';
import {Brand} from '../ui/src/icons';
import '../ui/src/style.css';
const wait=(ms=40)=>new Promise(r=>setTimeout(r,ms));
const until=async(f:()=>unknown)=>{for(let n=0;n<150;n++){if(f())return;await wait();}throw Error('Timeout '+document.body.innerText.slice(-800));};
const home='/home/steamos',downloads=home+'/Downloads';
let preferences:any={view:'list',folders:{[home]:{sort:'size',descending:true},[downloads]:{sort:'modified',descending:false}}};
const calls:any[]=[];
const files=(directory:string)=>[
 {name:'Photos',path:directory+'/Photos',directory:true,symlink:false,size:0,mode:493,uid:1000,gid:1000},
 {name:'A very long photograph name with spaces.png',path:directory+'/picture.png',directory:false,symlink:false,size:8192,modified:1791360000,mode:420,uid:1000,gid:1000},
 {name:'notes.txt',path:directory+'/notes.txt',directory:false,symlink:false,size:64,modified:1791350000,mode:420,uid:1000,gid:1000},
 {name:'movie.webm',path:directory+'/movie.webm',directory:false,symlink:true,size:2048,modified:1791340000,mode:420,uid:1000,gid:1000}];
window.fetch=async(url,init)=>{
 if(String(url)!=='/manager-api/files')throw Error('Unexpected request '+url);
 const p=JSON.parse(String(init?.body));calls.push(p);let result:any;
 if(p.operation==='preferences')result=[];
 else if(p.operation==='view.preferences'){
  await wait(40);
  if(p.view)preferences.view=p.view;
  if(p.path)preferences.folders[p.path]={sort:p.sort,descending:p.descending};
  result=structuredClone(preferences);
 }else if(p.operation==='list'){
  let directory=p.path||home;if(directory==='/alias')directory=home;
  let entries=files(directory);
  if(p.hidden)entries=[]; // Tree requests do not consume the sort checks below.
  result={path:directory,parent:'/',entries,total:entries.length,roots:[{name:'Home',path:home},{name:'Downloads',path:downloads}]};
 }else if(p.operation==='read')result={text:'fixture text',revision:'revision'};
 else throw Error('Unexpected operation '+p.operation);
 return new Response(JSON.stringify({result}));
};
const root=createRoot(document.getElementById('root')!);
const render=(key:string)=>root.render(<div className="app manager"><header className="shell-header"><Brand wordmark/><span>Files</span></header><div className="workspace tools-workspace"><main className="content file-content"><div className="file-workspace-frame"><FileManager key={key}/></div></main></div></div>);
const button=(label:string)=>[...document.querySelectorAll<HTMLButtonElement>('.file-manager button')].find(b=>b.textContent?.trim()===label)!;
const select=async(label:string,option:string)=>{document.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!.click();await until(()=>document.querySelector('[role=listbox]'));[...document.querySelectorAll<HTMLButtonElement>('[role=option]')].find(b=>b.textContent===option)!.click();await wait();};
const ready=()=>document.querySelector('.file-list-scroll')?.getAttribute('aria-busy')==='false';
const listing=()=>calls.filter(p=>p.operation==='list'&&!p.hidden).at(-1);
const changeAddress=async(value:string)=>{const input=document.querySelector<HTMLInputElement>('.file-address input')!;Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')!.set!.call(input,value);input.dispatchEvent(new Event('input',{bubbles:true}));await wait();input.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));};
(window as any).runInstallReviewChecks=async()=>{try{
 configureLanguage('en-US',[]);render('one');await until(ready);
 if(listing().sort!=='size'||listing().descending!==true)throw Error('Home sort not restored on initial canonical path');
 await select('View','Large icons');await until(()=>document.querySelector('.file-icon-grid.icons-large'));
 if(document.querySelector('.file-table'))throw Error('Table remained in icon view');
 const checkbox=document.querySelector<HTMLInputElement>('.file-tile-check input')!;checkbox.click();await until(()=>document.querySelector('.file-icon-tile.selected'));
 await select('View','Small icons');await until(()=>document.querySelector('.icons-small .file-icon-tile.selected'));
 document.querySelector('.file-tile-open')!.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,clientX:500,clientY:400}));await until(()=>document.querySelector('.file-context-menu'));if(!button('Copy'))throw Error('Context actions missing');window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape'}));
 button('Downloads').click();await until(()=>ready()&&listing().path===downloads);
 if(listing().sort!=='modified'||listing().descending!==false||!document.querySelector('.icons-small'))throw Error('Folder sort/global view scope incorrect');
 await select('Sort','Name');await until(ready);document.querySelector<HTMLButtonElement>('[aria-label="Ascending"]')!.click();await until(()=>ready()&&listing().descending===true);
 button('Home').click();await until(()=>ready()&&listing().path===home);if(listing().sort!=='size'||listing().descending!==true)throw Error('Home sort overwritten by downloads');
 button('Downloads').click();await until(()=>ready()&&listing().path===downloads);if(listing().sort!=='name'||listing().descending!==true)throw Error('Changed sort not remembered');
 await select('View','Medium icons');await until(()=>preferences.view==='icons-medium');render('two');await wait();await until(()=>ready()&&document.querySelector('.icons-medium'));
 if(listing().sort!=='size')throw Error('Preferences failed across remount');
 await changeAddress('/alias');await until(()=>ready()&&document.querySelector<HTMLInputElement>('.file-address input')!.value===home);
 if(listing().sort!=='size')throw Error('Canonical folder preference not restored');
 const tile=document.querySelector<HTMLButtonElement>('.file-tile-open[title="notes.txt"]')!;tile.click();await until(()=>document.querySelector('.file-editor'));button('Close').click();
 await wait(400);console.log('FRAMELY_PREVIEW_INSTALL_FILE_MANAGER_ICONS_EN');await wait(300);
 configureLanguage('zh-CN',[]);render('three');await wait();await until(()=>ready()&&document.querySelector('.icons-medium'));await select('展示方式','大图标');await until(()=>document.querySelector('.icons-large'));
 await wait(400);console.log('FRAMELY_PREVIEW_INSTALL_FILE_MANAGER_ICONS_ZH');await wait(300);
 await select('展示方式','列表');await until(()=>document.querySelector('.file-table'));document.querySelector<HTMLInputElement>('[aria-label="选择本页"]')!.click();await until(()=>document.querySelectorAll('.file-table tr.selected').length===4);
 if(calls.some(p=>p.operation==='preview'))throw Error('Grid fetched media unexpectedly');
 console.log('FRAMELY_BRIDGE_PASS');
 }catch(e){console.error('FRAMELY_BRIDGE_FAIL '+String(e)+' '+(e as Error).stack);}};
console.log('FRAMELY_VIEW_READY');
