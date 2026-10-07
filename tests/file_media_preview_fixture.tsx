import React from 'react';
import {createRoot} from 'react-dom/client';
import {FileMediaPreview} from '../ui/src/file-media-preview';
import {configureLanguage} from '../ui/src/i18n';
import '../ui/src/style.css';
import '../ui/src/file-manager.css';
const wait=(ms=40)=>new Promise(r=>setTimeout(r,ms));
const until=async(f:()=>unknown)=>{for(let n=0;n<200;n++){if(f())return;await wait();}throw Error('Preview timeout '+document.body.innerText);};
const root=createRoot(document.getElementById('root')!);
let imageUrl='',videoUrl='',closed=0;const released:string[]=[],downloads:string[]=[];
window.fetch=async(_url,init)=>{const p=JSON.parse(String(init?.body));if(p.operation==='preview.release'){released.push(p.token);return new Response(JSON.stringify({result:true}));}if(p.operation!=='preview')throw Error('Unexpected operation');await wait(60);return new Response(JSON.stringify({result:{url:p.path==='/broken.png'?'data:image/png;base64,AA==':p.path.endsWith('.webm')?videoUrl:imageUrl,token:p.path}}));};
const entries=[{path:'/first.png',name:'Generated landscape.png'},{path:'/clip.webm',name:'Generated video.webm'},{path:'/last.png',name:'Second generated image.png'},{path:'/broken.png',name:'Unsupported image.png'}];
const render=(initial=0)=>root.render(<><button id="opener">Files</button><FileMediaPreview entries={entries} initial={initial} onClose={()=>{closed++;root.render(<button id="opener">Files</button>);}} onDownload={path=>downloads.push(path)}/></>);
const button=(label:string)=>document.querySelector<HTMLButtonElement>(`.file-media-overlay [aria-label="${label}"]`)!;
const loaded=()=>document.querySelector<HTMLElement>('.file-media-canvas')?.style.visibility==='visible'&&!document.querySelector('.file-media-loading')&&!document.querySelector('.file-media-error');
const transform=()=>document.querySelector<HTMLElement>('.file-media-canvas')!.style.transform;
async function samples(){const canvas=document.createElement('canvas');canvas.width=640;canvas.height=360;const ctx=canvas.getContext('2d')!;ctx.fillStyle='#63869b';ctx.fillRect(0,0,640,360);ctx.fillStyle='#bcc4c9';ctx.beginPath();ctx.moveTo(0,360);ctx.lineTo(220,100);ctx.lineTo(450,360);ctx.fill();imageUrl=canvas.toDataURL();const stream=canvas.captureStream(10),recorder=new MediaRecorder(stream,{mimeType:'video/webm;codecs=vp8'}),chunks:Blob[]=[];recorder.ondataavailable=e=>chunks.push(e.data);const stopped=new Promise<void>(r=>recorder.onstop=()=>r());recorder.start();const timer=setInterval(()=>ctx.fillRect(500,100,40,40),70);await wait(1200);recorder.stop();await stopped;clearInterval(timer);stream.getTracks().forEach(t=>t.stop());videoUrl=URL.createObjectURL(new Blob(chunks,{type:'video/webm'}));}
(window as any).runInstallReviewChecks=async()=>{try{
 configureLanguage('en-US',[]);root.render(<div className="app manager" style={{height:'100vh',background:'#222'}}><button>Files</button></div>);await wait(300);await samples();render();await until(loaded);
 const overlay=document.querySelector<HTMLElement>('.file-media-overlay')!,bounds=overlay.getBoundingClientRect();if(bounds.width!==innerWidth||bounds.height!==innerHeight)throw Error('Viewer not fullscreen');
 const toolbar=document.querySelector<HTMLElement>('.file-media-actions')!.getBoundingClientRect(),caption=document.querySelector<HTMLElement>('.file-media-caption')!.getBoundingClientRect();if(Math.abs(toolbar.x+toolbar.width/2-innerWidth/2)>1||toolbar.bottom>innerHeight||caption.top<toolbar.bottom||document.querySelector('.file-media-header'))throw Error('Bottom capsule/caption layout incorrect');
 if(!button('Previous file').disabled)throw Error('Previous not disabled on first file');
 button('Zoom in').click();await until(()=>document.querySelector('output')?.textContent==='125%');
 const stage=document.querySelector<HTMLElement>('.file-media-stage')!;stage.dispatchEvent(new WheelEvent('wheel',{deltaY:-100,clientX:180,clientY:150,bubbles:true,cancelable:true}));await until(()=>transform().includes('scale(1.5'));
 // Synthetic events cannot capture native pointers. Exercise the same pan handler
 // while retaining real browser media decoding and layout.
 const capture=Element.prototype.setPointerCapture;Element.prototype.setPointerCapture=function(){};
 stage.dispatchEvent(new PointerEvent('pointerdown',{pointerId:7,clientX:100,clientY:100,button:0,bubbles:true,cancelable:true}));const before=transform();stage.dispatchEvent(new PointerEvent('pointermove',{pointerId:7,clientX:140,clientY:120,bubbles:true,cancelable:true}));await until(()=>transform()!==before);stage.dispatchEvent(new PointerEvent('pointerup',{pointerId:7,bubbles:true}));stage.click();await wait();if(closed)throw Error('Panning closed viewer');Element.prototype.setPointerCapture=capture;
 button('Fit to window').click();await until(()=>transform()==='translate(0px, 0px) scale(1)');
 overlay.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',bubbles:true}));await until(()=>document.querySelector('video')&&loaded());
 const player=document.querySelector('video')!,videoBounds=player.getBoundingClientRect();if(player.videoWidth!==640||player.videoHeight!==360||videoBounds.height<100)throw Error('Video picture missing or collapsed');
 if(!player.controls||!released.includes('/first.png'))throw Error('Video controls/grant release missing');
 button('Download').click();if(downloads[0]!=='/clip.webm')throw Error('Wrong video downloaded');
 button('Next file').click();await until(()=>document.querySelector('img')?.getAttribute('alt')===entries[2].name&&loaded());
 if(!transform().includes('scale(1)'))throw Error('Switch retained transform');
 button('Next file').click();await until(()=>document.querySelector('.file-media-error'));if(!button('Next file').disabled)throw Error('Next not disabled on last file');
 button('Download').click();if(!downloads.includes('/broken.png'))throw Error('Decode failure download missing');
 button('Previous file').click();await until(loaded);await wait(200);console.log('FRAMELY_PREVIEW_INSTALL_FILE_MEDIA_EN');await wait(250);
 overlay.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}));await until(()=>closed===1&&!document.querySelector('.file-media-overlay'));await until(()=>released.includes('/last.png'));
  configureLanguage('zh-CN',[]);render();await until(loaded);if(!button('放大')||!button('下一个文件'))throw Error('Chinese controls missing');await wait(200);console.log('FRAMELY_PREVIEW_INSTALL_FILE_MEDIA_ZH');await wait(250);
 document.querySelector('img')!.dispatchEvent(new MouseEvent('click',{bubbles:true}));await wait();if(closed!==1)throw Error('Media click closed viewer');
 const blank=document.querySelector<HTMLElement>('.file-media-stage')!;Element.prototype.setPointerCapture=function(){};blank.dispatchEvent(new PointerEvent('pointerdown',{pointerId:9,clientX:10,clientY:10,button:0,bubbles:true}));blank.dispatchEvent(new PointerEvent('pointerup',{pointerId:9,bubbles:true}));Element.prototype.setPointerCapture=capture;blank.click();await until(()=>closed===2);URL.revokeObjectURL(videoUrl);console.log('FRAMELY_BRIDGE_PASS');
 }catch(e){console.error('FRAMELY_BRIDGE_FAIL '+String(e)+' '+(e as Error).stack);}};
console.log('FRAMELY_VIEW_READY');
