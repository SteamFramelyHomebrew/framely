// The actual FileManager/CodeMirror workspace with generated demonstration code.
import React from 'react';
import {createRoot} from 'react-dom/client';
import {FileManager} from '../ui/src/file-manager';
import {configureLanguage} from '../ui/src/i18n';
import {Brand} from '../ui/src/icons';
import '../ui/src/style.css';
configureLanguage('zh-CN',[]);
const home='/home/steamos',code=`// 串流配置示例，仅用于界面验证\r\ninterface CaptureSettings {\r\n  width: number;\r\n  height: number;\r\n  fps: number;\r\n  codec: 'h264' | 'h265';\r\n}\r\n\r\nconst settings: CaptureSettings = {\r\n  width: 2560,\r\n  height: 1920,\r\n  fps: 90,\r\n  codec: 'h265',\r\n};\r\n\r\nfunction resolution(config: CaptureSettings): string {\r\n  const { width, height } = config;\r\n  return width + ' × ' + height;\r\n}\r\n\r\nexport { settings, resolution };\r\n`;
let text=code;
window.fetch=async(url,init)=>{
 if(String(url)==='/api')return new Response(JSON.stringify({result:true}));
 const p=JSON.parse(String(init?.body));let result:any;
 if(p.operation==='preferences')result=[];
 else if(p.operation==='view.preferences')result={view:'list',folders:{}};
 else if(p.operation==='list')result={path:p.path??home,parent:'/',entries:p.hidden?[]:[{name:'capture.ts',path:home+'/capture.ts',directory:false,symlink:false,size:code.length,mode:420,uid:1000,gid:1000}],total:p.hidden?0:1,roots:[{name:'Home',path:home}]};
 else if(p.operation==='read')result={text,revision:'example',readOnly:false};
 else if(p.operation==='write'){text=p.text;result={text,revision:'saved',readOnly:false};}
 else throw Error('Unexpected operation '+p.operation);
 return new Response(JSON.stringify({result}));
};
createRoot(document.getElementById('root')!).render(<div className="app manager"><header className="shell-header"><Brand wordmark/><span>文件</span></header><div className="workspace tools-workspace"><main className="content file-content"><div className="file-workspace-frame"><FileManager/></div></main></div></div>);
const wait=(ms=40)=>new Promise(r=>setTimeout(r,ms));
const until=async(f:()=>unknown)=>{for(let n=0;n<150;n++){if(f())return;await wait();}throw Error('Timeout '+document.body.innerText);};
(window as any).runInstallReviewChecks=async()=>{try{
 await until(()=>document.querySelector('.file-list-scroll')?.getAttribute('aria-busy')==='false');document.querySelector<HTMLButtonElement>('.file-name')!.click();await until(()=>document.querySelector('.cm-content[contenteditable=true]'));await wait(250);
 const workspace=document.querySelector<HTMLElement>('.file-editor-workspace')!,area=workspace.getBoundingClientRect(),root=document.querySelector('.file-manager')!.getBoundingClientRect(),surface=document.querySelector('.file-code-surface')!.getBoundingClientRect();
 if(area.width<root.width-2||area.height<root.height-2||surface.height<300)throw Error('Editor does not fill workspace');
 const line=document.querySelector('.cm-line')!.getBoundingClientRect(),number=[...document.querySelectorAll<HTMLElement>('.cm-lineNumbers .cm-gutterElement')].find(e=>e.getBoundingClientRect().height>0&&getComputedStyle(e).visibility!=='hidden')!.getBoundingClientRect();
 if(Math.abs(line.top-number.top)>2)throw Error(`Gutter is not aligned: ${line.top} / ${number.top}`);
 if(document.querySelectorAll('.cm-line span').length<8||!document.body.textContent?.includes('CRLF'))throw Error('Syntax or line ending status missing');
 if(!document.querySelector('.file-layout')?.hasAttribute('inert'))throw Error('Editor left background controls interactive');
 if(document.querySelector('.cm-tooltip-autocomplete'))throw Error('Unexpected completion popup');
 document.querySelector<HTMLButtonElement>('[aria-label="语言模式"]')!.click();await until(()=>document.querySelector('[role=listbox]'));const menu=document.querySelector('[role=listbox]')!.getBoundingClientRect();if(menu.bottom>innerHeight||menu.height<100)throw Error('Language dropdown clipped');
 [...document.querySelectorAll<HTMLButtonElement>('[role=option]')].find(e=>e.textContent==='JSON')!.click();await wait();document.querySelector<HTMLButtonElement>('[aria-label="语言模式"]')!.click();await until(()=>document.querySelector('[role=listbox]'));[...document.querySelectorAll<HTMLButtonElement>('[role=option]')].find(e=>e.textContent==='TypeScript')!.click();await wait(200);
 document.querySelector<HTMLButtonElement>('[aria-label="语言模式"]')!.click();await until(()=>document.querySelector('[role=listbox]'));document.querySelector('[role=option]')!.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));await until(()=>!document.querySelector('[role=listbox]'));if(!document.querySelector('.file-editor-workspace'))throw Error('Language menu Escape closed editor');
 console.log('FRAMELY_PREVIEW_INSTALL_CODE_EDITOR_ZH');await wait(200);
 [...document.querySelectorAll<HTMLButtonElement>('.file-code-toolbar button')].find(e=>e.textContent?.includes('查找替换'))!.click();await until(()=>document.querySelector('.file-code-search'));
 await wait();const searchSurface=document.querySelector('.file-code-surface')!.getBoundingClientRect(),scroller=document.querySelector('.cm-scroller')!.getBoundingClientRect();if(Math.abs(searchSurface.top-scroller.top)>2||Math.abs(searchSurface.height-scroller.height)>2)throw Error('Find widget consumes document space');
 const search=document.querySelector<HTMLElement>('.file-code-search')!,find=search.querySelector<HTMLInputElement>('input[name=Find]')!,replace=search.querySelector<HTMLInputElement>('input[name=Replace]')!;
 const set=(input:HTMLInputElement,value:string)=>{input.value=value;input.dispatchEvent(new Event('input',{bubbles:true}));};
 set(find,'width');await wait();if(!search.querySelector('output')?.textContent?.includes('/ 4'))throw Error('Wrong search match count');
 find.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));await wait();if(!search.querySelector('output')?.textContent?.startsWith('1 /'))throw Error('Find did not select first result');
 find.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',shiftKey:true,bubbles:true}));await wait();if(!search.querySelector('output')?.textContent?.startsWith('4 /'))throw Error('Previous did not wrap');
 search.querySelector<HTMLButtonElement>('[name="Toggle replace"]')!.click();if(replace.closest<HTMLElement>('.file-search-replacement')!.hidden)throw Error('Replace row did not expand');
 set(replace,'pixels');search.querySelector<HTMLButtonElement>('[name="replace all"]')!.click();await wait();if(!document.querySelector('.cm-content')?.textContent?.includes('pixels')||search.querySelector('output')?.textContent!=='– / 0')throw Error('Replace all failed');
 document.querySelector<HTMLButtonElement>('[aria-label="撤销"]')!.click();await wait();if(!search.querySelector('output')?.textContent?.includes('/ 4'))throw Error('Undo did not restore matches');
 search.querySelector<HTMLButtonElement>('[name=regexp]')!.click();set(find,'[');await wait();if(find.getAttribute('aria-invalid')!=='true'||!search.querySelector<HTMLButtonElement>('[name=next]')!.disabled||!search.textContent?.includes('正则表达式无效'))throw Error('Invalid regex not reported');
 set(find,'width');search.querySelector<HTMLButtonElement>('[name=regexp]')!.click();
 search.querySelector<HTMLButtonElement>('[name="match case"]')!.click();set(find,'WIDTH');await wait();if(!search.textContent?.includes('没有匹配结果'))throw Error('Case toggle ignored');
 search.querySelector<HTMLButtonElement>('[name="match case"]')!.click();search.querySelector<HTMLButtonElement>('[name="by word"]')!.click();set(find,'wid');await wait();if(!search.textContent?.includes('没有匹配结果'))throw Error('Whole word toggle ignored');
 set(find,'');find.focus();(window as any).__framelyCommitKeyboard('width');await wait();if(!search.querySelector('output')?.textContent?.includes('/ 4'))throw Error('Native keyboard did not update search');
 await wait(1200);console.log('FRAMELY_PREVIEW_INSTALL_CODE_EDITOR_SEARCH_ZH');await wait(200);
 if(search.getBoundingClientRect().right>innerWidth+1||find.getBoundingClientRect().width<100)throw Error('Find widget is clipped');
 const status=document.querySelector('.file-editor-status')!;if(status.scrollWidth>status.clientWidth||status.getBoundingClientRect().right>area.right+1)throw Error('Status bar is clipped');
 find.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));await until(()=>!document.querySelector('.file-code-search'));if(!document.querySelector('.file-editor-workspace'))throw Error('Search Escape closed file');
 const content=document.querySelector<HTMLElement>('.cm-content')!;content.focus();content.dispatchEvent(new KeyboardEvent('keydown',{key:'Home',code:'Home',ctrlKey:true,bubbles:true}));await wait();(window as any).__framelyEditKeyboard('// ');await wait();
 if(!document.querySelector('.file-editor-status output')?.textContent?.includes('列 4'))throw Error('Cursor status not updated');
 const [save,close]=[...document.querySelectorAll<HTMLButtonElement>('.file-editor-heading button')];save.click();await until(()=>text.startsWith('// // ')&&save.disabled&&!close.disabled);
 if(text.replaceAll('\r\n','').includes('\n'))throw Error('Saving changed CRLF line endings');
 const host=document.createElement('div');host.style.cssText='position:fixed;left:-5000px;width:500px;height:300px';document.body.append(host);let modified=false;
 let saves=0;
 const writable=window.FramelyCodeEditor!.mount({parent:host,path:'save.txt',value:'hello hello',readOnly:false,label:'Save shortcut test',phrases:{},onChange:()=>{modified=true;},onSave:()=>{saves++;}});writable.find();
 host.querySelector<HTMLButtonElement>('[name="Toggle replace"]')!.click();
 const mac=/Mac|iPhone|iPad/.test(navigator.platform);
 for(const target of [host.querySelector<HTMLElement>('.cm-content')!,...host.querySelectorAll<HTMLInputElement>('.file-code-search input')]){
  target.focus();const before=saves,event=new KeyboardEvent('keydown',{key:'s',code:'KeyS',ctrlKey:!mac,metaKey:mac,bubbles:true,cancelable:true});target.dispatchEvent(event);
  if(saves!==before+1||!event.defaultPrevented)throw Error('Save shortcut failed in '+(target.getAttribute('name')??'document'));
 }
 if(modified)throw Error('Save shortcut changed document text');writable.destroy();
 const readonly=window.FramelyCodeEditor!.mount({parent:host,path:'readonly.txt',value:'hello hello',readOnly:true,label:'Readonly test',phrases:{},onChange:()=>{modified=true;},onSave:()=>{}});readonly.find();
 const panel=host.querySelector('.file-code-search')!,readonlyFind=panel.querySelector<HTMLInputElement>('[name=Find]')!;set(readonlyFind,'hello');panel.querySelector<HTMLButtonElement>('[name="Toggle replace"]')!.click();
 if(!panel.querySelector<HTMLInputElement>('[name=Replace]')!.disabled||!panel.querySelector<HTMLButtonElement>('[name="replace all"]')!.disabled)throw Error('Read-only replacement allowed');
 readonlyFind.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));if(modified)throw Error('Read-only search mutated text');readonly.destroy();host.remove();
 console.log('FRAMELY_BRIDGE_PASS');
 }catch(error){console.error('FRAMELY_BRIDGE_FAIL '+error);}
};
console.log('FRAMELY_VIEW_READY');
