import React from 'react';
import {createRoot} from 'react-dom/client';
import {FileThumbnail} from '../ui/src/file-thumbnail';
import '../ui/src/file-manager.css';
const wait=(ms=40)=>new Promise(r=>setTimeout(r,ms));
const until=async(f:()=>unknown)=>{for(let n=0;n<200;n++){if(f())return;await wait();}throw Error('Thumbnail timeout');};
const root=createRoot(document.getElementById('root')!);
const requests:string[]=[],released:string[]=[],cancelled:string[]=[];
let active=0,maxActive=0,imageUrl='',videoUrl='',revision='1';
window.fetch=async(_url,init)=>{
 const p=JSON.parse(String(init?.body));
 if(p.operation==='preview.release'){released.push(p.token);return new Response(JSON.stringify({result:true}));}
 if(p.operation!=='preview')throw Error('Unexpected request');
 requests.push(p.path);active++;maxActive=Math.max(maxActive,active);
 try{await new Promise<void>((resolve,reject)=>{const timer=setTimeout(resolve,p.path==='/slow.png'?1000:80);init?.signal?.addEventListener('abort',()=>{clearTimeout(timer);cancelled.push(p.path);reject(new DOMException('Aborted','AbortError'));},{once:true});});}
 finally{active--;}
 return new Response(JSON.stringify({result:{url:p.path==='/bad.png'?'data:image/png;base64,AA==':p.path.endsWith('.webm')?videoUrl:imageUrl,token:p.path}}));
};
const render=(paths:string[],active=true)=>root.render(<div className="file-manager" style={{background:'#222',color:'#ddd',padding:20}}><div id="viewport" style={{height:180,width:360,overflow:'auto'}}>{paths.map(path=><div key={path} data-path={path} style={{height:140,display:'flex',gap:20,alignItems:'center'}}><span style={{width:100,height:100}}><FileThumbnail active={active} entry={{path,name:path,directory:false,size:100,mediaRevision:revision}}><span className="fallback">File icon</span></FileThumbnail></span>{path}</div>)}</div></div>);
const thumbnail=(path:string)=>document.querySelector(`[data-path="${path}"] .file-media-thumbnail`);
async function samples(){
 const canvas=document.createElement('canvas');canvas.width=120;canvas.height=80;const ctx=canvas.getContext('2d')!;
 ctx.fillStyle='#77b0cf';ctx.fillRect(0,0,120,80);imageUrl=canvas.toDataURL();
 const stream=canvas.captureStream(10),recorder=new MediaRecorder(stream,{mimeType:'video/webm;codecs=vp8'}),chunks:Blob[]=[];
 recorder.ondataavailable=e=>chunks.push(e.data);
 const stopped=new Promise<void>(r=>recorder.onstop=()=>r());recorder.start();
 const timer=setInterval(()=>{ctx.fillStyle='#c89665';ctx.fillRect(10,10,100,60);},70);
 await wait(1300);recorder.stop();await stopped;clearInterval(timer);stream.getTracks().forEach(t=>t.stop());videoUrl=URL.createObjectURL(new Blob(chunks,{type:'video/webm'}));
}
(window as any).runInstallReviewChecks=async()=>{try{
 await samples();render(['/one.png','/two.png','/three.png','/clip.webm','/bad.png']);
 await until(()=>thumbnail('/one.png')&&thumbnail('/two.png'));
 if(requests.includes('/three.png')||requests.includes('/clip.webm'))throw Error('Offscreen media loaded');
 document.getElementById('viewport')!.scrollTop=280;await until(()=>thumbnail('/three.png')&&thumbnail('/clip.webm'));
 if(!document.querySelector('[data-path="/clip.webm"] .file-video-marker'))throw Error('Video marker missing');
 document.getElementById('viewport')!.scrollTop=560;await until(()=>released.includes('/bad.png'));await wait(100);
 if(thumbnail('/bad.png')||!document.querySelector('[data-path="/bad.png"] .fallback'))throw Error('Decode failure did not retain icon');
 const count=requests.filter(p=>p==='/one.png').length;document.getElementById('viewport')!.scrollTop=0;await until(()=>thumbnail('/one.png'));
 if(requests.filter(p=>p==='/one.png').length!==count)throw Error('Thumbnail cache not reused');
 revision='2';render(['/one.png']);await until(()=>requests.filter(p=>p==='/one.png').length===count+1&&thumbnail('/one.png'));
 render(['/slow.png']);await until(()=>requests.includes('/slow.png'));render(['/notes.txt']);await until(()=>cancelled.includes('/slow.png'));
 if(requests.includes('/notes.txt')||maxActive>2)throw Error('Invalid source request or unbounded parallelism');
 render(['/inactive.png'],false);await wait(300);if(requests.includes('/inactive.png'))throw Error('Inactive panel fetched media');
 render(['/one.png','/clip.webm']);await until(()=>thumbnail('/one.png')&&thumbnail('/clip.webm'));
 console.log('FRAMELY_PREVIEW_INSTALL_FILE_THUMBNAILS');await wait(300);
 URL.revokeObjectURL(videoUrl);console.log('FRAMELY_BRIDGE_PASS');
 }catch(e){console.error('FRAMELY_BRIDGE_FAIL '+String(e)+' '+(e as Error).stack);}};
console.log('FRAMELY_VIEW_READY');
