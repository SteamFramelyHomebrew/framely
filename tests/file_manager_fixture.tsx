import React from 'react';
import {createRoot} from 'react-dom/client';
import {FileManager} from '../ui/src/file-manager';
import {configureLanguage} from '../ui/src/i18n';
import {Brand} from '../ui/src/icons';
import '../ui/src/style.css';
const wait=(ms=40)=>new Promise(r=>setTimeout(r,ms));
const until=async(f:()=>unknown)=>{for(let n=0;n<150;n++){if(f())return;await wait();}throw Error('Timeout '+document.body.innerText.slice(-800));};
const home='/home/steamos',downloads=home+'/Downloads';
const longFolder='一个很长的文件夹名称 - Project documents and archived screenshots';
let binaryMode=false;
let savedText='fixture text',permissionMode=420,failPermissions=false;
let preferences:any={view:'list',folders:{[home]:{sort:'size',descending:true},[downloads]:{sort:'modified',descending:false}}};
const calls:any[]=[],downloadsClicked:string[]=[];const anchorClick=HTMLAnchorElement.prototype.click;HTMLAnchorElement.prototype.click=function(){downloadsClicked.push(this.getAttribute('href')??'');};
const files=(directory:string)=>[
 {name:'Photos',path:directory+'/Photos',directory:true,symlink:false,size:0,mode:493,uid:1000,gid:1000},
 {name:'A very long photograph name with spaces.png',path:directory+'/picture.png',directory:false,symlink:false,size:8192,modified:1791360000,mode:420,uid:1000,gid:1000},
 {name:'notes.txt',path:directory+'/notes.txt',directory:false,symlink:false,size:64,modified:1791350000,mode:420,uid:1000,gid:1000},
 {name:'movie.webm',path:directory+'/movie.webm',directory:false,symlink:true,size:2048,modified:1791340000,mode:420,uid:1000,gid:1000}];
window.fetch=async(url,init)=>{
 if(String(url)==='/api'){const request=JSON.parse(String(init?.body));calls.push(request);return new Response(JSON.stringify({result:true}));}
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
  let entries=files(directory);if(binaryMode)entries=[{...entries[2],name:'sample.bin',path:directory+'/sample.bin'}];
  if(p.hidden){ // Tree requests do not consume the sort checks below.
   const child=directory==='/'?{name:'home',path:'/home'}:directory==='/home'?{name:'steamos',path:home}:directory===home?{name:longFolder,path:home+'/'+longFolder}:null;
   entries=child?[{...child,directory:true,symlink:false,size:0,mode:493,uid:1000,gid:1000}]:[];
  }
  result={path:directory,parent:'/',entries,total:entries.length,roots:[{name:'Home',path:home},{name:'Downloads',path:downloads}]};
 }else if(p.operation==='preview'){const canvas=document.createElement('canvas');canvas.width=80;canvas.height=60;const ctx=canvas.getContext('2d')!;ctx.fillStyle='#8bb4ce';ctx.fillRect(0,0,80,60);result={url:p.path.endsWith('.png')?canvas.toDataURL(): 'data:video/webm;base64,AA==',token:p.path};}
 else if(p.operation==='preview.release')result=true;
 else if(p.operation==='read'){if(p.path.endsWith('notes.txt'))result={text:savedText,revision:'revision',readOnly:false};else if(p.asText)result={text:'binary\u0000preview',revision:'binary',readOnly:true};else return new Response(JSON.stringify({error:'Binary files cannot be edited as text'}));}
 else if(p.operation==='write'){savedText=p.text;result={text:savedText,revision:'saved',readOnly:false};}
 else if(p.operation==='properties'){const e=files(home).find(e=>e.path===p.path)!;result={...e,regular:!e.directory&&!e.symlink,mode:permissionMode,owner:'steamos',group:'steamos',canChmod:!e.symlink,inode:'9007199254740993',device:'2049',links:1,allocated:8192,accessed:1791360000,changed:1791360000,linkTarget:e.symlink?'notes.txt':undefined,resolvedPath:e.symlink?home+'/notes.txt':e.path};}
 else if(p.operation==='chmod'){if(failPermissions)return new Response(JSON.stringify({error:'Fixture permission denied'}));if(p.identity.inode!=='9007199254740993')throw Error('Properties lost inode precision');permissionMode=p.mode;result=true;}
 else if(p.operation==='download')result={url:'/fixture-download'};
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
 configureLanguage('en-US',[]);render('one');await until(ready);await wait(200);
 if(innerWidth<1000){
  const item=(name:string)=>[...document.querySelectorAll<HTMLButtonElement>('.file-name')].find(e=>e.textContent?.trim()===name)!;
  const context=async(name:string)=>{await until(ready);item(name).dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,clientX:300,clientY:400}));await until(()=>document.querySelector('.file-context-menu'));};
  await context('notes.txt');button('Properties').click();await until(()=>document.querySelector('.file-properties-modal')?.getAttribute('aria-busy')==='false');
  if(document.querySelectorAll('.file-permission-grid button').length!==9)throw Error('Mobile permissions missing');await wait(150);console.log('FRAMELY_PREVIEW_INSTALL_FILE_PROPERTIES_EN');await wait(100);button('Close').click();await until(()=>!document.querySelector('.file-properties-modal'));
  await context('notes.txt');button('Edit as text').click();await until(()=>document.querySelector('.cm-content[contenteditable=true]'));const content=document.querySelector<HTMLElement>('.cm-content')!;content.focus();await wait();(window as any).__framelyEditKeyboard('中文');await wait();if(!content.textContent?.includes('中文'))throw Error('Mobile native editing failed');await wait(100);console.log('FRAMELY_PREVIEW_INSTALL_CODE_EDITOR_EN');await wait(100);button('Save').click();await until(()=>!button('Close').disabled&&button('Save').disabled&&savedText.includes('中文'));button('Close').click();await until(()=>!document.querySelector('.file-editor-modal'));
  await context('A very long photograph name with spaces.png');await until(()=>!button('Edit as text').disabled);button('Edit as text').click();await until(()=>document.querySelector('.cm-content[contenteditable=false]'));if(!button('Save').disabled)throw Error('Mobile binary preview writable');button('Close').click();await until(()=>!document.querySelector('.file-editor-modal'));
  configureLanguage('zh-CN',[]);render('mobile-zh');await wait();await until(ready);await context('notes.txt');button('属性').click();await until(()=>document.querySelector('.file-properties-modal')?.getAttribute('aria-busy')==='false');await wait(150);console.log('FRAMELY_PREVIEW_INSTALL_FILE_PROPERTIES_ZH');await wait(100);
  if(document.documentElement.scrollWidth>innerWidth)throw Error('Mobile page overflow');
  HTMLAnchorElement.prototype.click=anchorClick;console.log('FRAMELY_BRIDGE_PASS');return;
 }
 if(calls.some(p=>p.operation==='preview'))throw Error('List view fetched thumbnails');
 if(listing().sort!=='size'||listing().descending!==true)throw Error('Home sort not restored on initial canonical path');
 await until(()=>[...document.querySelectorAll('.file-tree-entry button>span')].some(node=>node.textContent===longFolder));
 const tree=document.querySelector<HTMLElement>('.file-tree-scroll')!,name=[...tree.querySelectorAll<HTMLElement>('button>span')].find(node=>node.textContent===longFolder)!;
 if(getComputedStyle(name).textOverflow==='ellipsis'||name.scrollWidth>name.clientWidth+1)throw Error('Tree folder name clipped');
 if(tree.scrollWidth<=tree.clientWidth)throw Error('Tree does not expose horizontal overflow');
 tree.scrollLeft=tree.scrollWidth;if(tree.scrollLeft<=0)throw Error('Cannot scroll long tree names');tree.scrollLeft=0;
 if(name.closest('button')?.title!==home+'/'+longFolder)throw Error('Full path tooltip missing');

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
 const tile=document.querySelector<HTMLButtonElement>('.file-tile-open[title="notes.txt"]')!;
 tile.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,clientX:500,clientY:400}));await until(()=>document.querySelector('.file-context-menu'));button('Properties').click();await until(()=>document.querySelector('.file-properties-modal')?.getAttribute('aria-busy')==='false');
 await wait(150);console.log('FRAMELY_PREVIEW_INSTALL_FILE_PROPERTIES_EN');await wait(100);
 if(document.querySelectorAll('.file-permission-grid [role=checkbox]').length!==9||!document.querySelector('.file-property-grid')?.textContent?.includes('9007199254740993'))throw Error('Detailed properties missing');
 document.querySelector<HTMLButtonElement>('[aria-label="Owner: Execute / enter"]')!.click();await wait();
 if(document.querySelector<HTMLInputElement>('.file-permissions input')!.value!=='744')throw Error('Permission grid and octal mode disagree');
 const modeInput=document.querySelector<HTMLInputElement>('.file-permissions input')!;
 const setMode=async(value:string)=>{Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value')!.set!.call(modeInput,value);modeInput.dispatchEvent(new Event('input',{bubbles:true}));await wait();};
 await setMode('888');if(!button('Save permissions').disabled)throw Error('Invalid octal mode accepted');
 await setMode('640');failPermissions=true;button('Save permissions').click();await until(()=>document.body.textContent?.includes('Fixture permission denied'));if(!document.querySelector('.file-properties-modal'))throw Error('Permission failure closed panel');
 failPermissions=false;await wait();button('Save permissions').click();await until(()=>!document.querySelector('.file-properties-modal'));if(permissionMode!==416)throw Error('Permission save failed');
 tile.click();await until(()=>document.querySelector('.cm-content[contenteditable=true]'));await wait(100);
 if(!document.querySelector('.cm-lineNumbers')||document.querySelector('.cm-tooltip-autocomplete'))throw Error('Editor features incorrect');
 await wait(150);console.log('FRAMELY_PREVIEW_INSTALL_CODE_EDITOR_EN');await wait(100);
 const content=document.querySelector<HTMLElement>('.cm-content')!;content.focus();await wait();(window as any).__framelyEditKeyboard('中文');await wait();if(!content.textContent?.includes('中文'))throw Error('Native keyboard input missing');
 (window as any).__framelyEditKeyboard('\n');(window as any).__framelyEditKeyboard('\b');await wait();
 document.querySelector<HTMLButtonElement>('.file-code-toolbar [aria-label="Undo"]')!.click();await wait();document.querySelector<HTMLButtonElement>('.file-code-toolbar [aria-label="Redo"]')!.click();await wait();
 button('Close').click();await until(()=>document.querySelector('.modal[aria-label="Unsaved changes"]'));button('Cancel').click();await until(()=>!document.querySelector('.modal[aria-label="Unsaved changes"]'));if(document.querySelector('.cm-content')!==content)throw Error('Canceling close discarded editor history');
 const probe=document.createElement('div');probe.style.cssText='position:fixed;left:-10000px;width:400px;height:200px';document.body.append(probe);
 const syntax=window.FramelyCodeEditor!.mount({parent:probe,value:'const enabled: boolean = true;\nfunction demo() {\n  return enabled;\n}\n',path:'settings.ts',readOnly:false,label:'Syntax test',phrases:{},onChange:()=>{},onSave:()=>{}});await wait(150);
 if(probe.querySelectorAll('.cm-line span').length<3||!probe.querySelector('.cm-foldGutter'))throw Error('Code highlighting or folding missing');if(document.querySelector('.cm-tooltip-autocomplete'))throw Error('Completion popup enabled');syntax.destroy();probe.remove();
 button('Find and replace').click();await until(()=>document.querySelector('.cm-search'));document.querySelector<HTMLInputElement>('.cm-search input')!.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));await wait();if(!document.querySelector('.file-editor-modal'))throw Error('Search Escape closed file');
 button('Save').click();await until(()=>savedText.includes('中文'));await until(()=>!button('Close').disabled);button('Close').click();await until(()=>!document.querySelector('.file-editor-modal'));
 // Text entry is available even for images, without invoking media preview.
 await until(ready);document.querySelector<HTMLButtonElement>('.file-tile-open[title="A very long photograph name with spaces.png"]')!.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,clientX:500,clientY:400}));await until(()=>document.querySelector('.file-context-menu')&&!button('Edit as text').disabled);button('Edit as text').click();await until(()=>document.querySelector('.cm-content[contenteditable=false]'));
 if(!button('Save').disabled||!document.body.textContent?.includes('read-only to protect'))throw Error('Binary preview is writable');button('Close').click();await until(()=>!document.querySelector('.file-editor-modal'));
 document.querySelector<HTMLButtonElement>('.file-tile-open[title="movie.webm"]')!.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,clientX:500,clientY:400}));await until(()=>document.querySelector('.file-context-menu'));button('Properties').click();await until(()=>document.querySelector('.file-properties-modal')?.getAttribute('aria-busy')==='false');
 if(!button('Save permissions').disabled||document.querySelector('.file-permission-grid button:not(:disabled)'))throw Error('Symlink permissions editable');button('Close').click();await until(()=>!document.querySelector('.file-properties-modal'));
 document.querySelector<HTMLButtonElement>('.file-tile-open[title="A very long photograph name with spaces.png"]')!.click();await until(()=>document.querySelector('.file-media-overlay img'));await until(()=>document.querySelector('.file-media-overlay img')?.getAttribute('src'));document.querySelector<HTMLButtonElement>('.file-media-overlay [aria-label="Close"]')!.click();await until(()=>!document.querySelector('.file-media-overlay'));
 await wait(400);console.log('FRAMELY_PREVIEW_INSTALL_FILE_MANAGER_ICONS_EN');await wait(300);
 configureLanguage('zh-CN',[]);render('three');await wait();await until(()=>ready()&&document.querySelector('.icons-medium'));await select('展示方式','大图标');await until(()=>document.querySelector('.icons-large'));
 await wait(400);console.log('FRAMELY_PREVIEW_INSTALL_FILE_MANAGER_ICONS_ZH');await wait(300);
 await select('展示方式','列表');await until(()=>document.querySelector('.file-table'));document.querySelector<HTMLInputElement>('[aria-label="选择本页"]')!.click();await until(()=>document.querySelectorAll('.file-table tr.selected').length===4);
 if(calls.some(p=>p.operation==='preview'&&!/\.(png|webm)$/.test(p.path)))throw Error('Non-media fetched for thumbnails');
 if(!calls.some(p=>p.operation==='preview.release'))throw Error('Source preview grants not released');
 binaryMode=true;document.querySelector<HTMLButtonElement>('[aria-label="刷新"]')!.click();await until(()=>document.querySelector('.file-name')?.textContent?.includes('sample.bin'));document.querySelector<HTMLButtonElement>('.file-name')!.click();await until(()=>downloadsClicked.includes('/fixture-download'));if(document.querySelector('.modal'))throw Error('Unsupported file opened a dialog');
 HTMLAnchorElement.prototype.click=anchorClick;console.log('FRAMELY_BRIDGE_PASS');
 }catch(e){console.error('FRAMELY_BRIDGE_FAIL '+String(e)+' '+(e as Error).stack);}};
console.log('FRAMELY_VIEW_READY');
