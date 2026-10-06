import React from 'react';import {createRoot} from 'react-dom/client';
import {Launcher} from '../ui/src/launcher';import {configureLanguage} from '../ui/src/i18n';import {fitCalibration,correctedGaze,calibrationBlocks,calibrationValidation} from '../ui/src/gaze-calibration';import '../ui/src/style.css';
const wait=(ms:number)=>new Promise(r=>setTimeout(r,ms));const calls:{method:string;params:any}[]=[];
window.fetch=async(_url,init)=>{const request=JSON.parse(String(init?.body));calls.push(request);return new Response(JSON.stringify({result:request.method==='steam.list'||request.method==='desktop.list'?[]:request.method==='launcher.gaze.calibration.get'?null:request.method==='launcher.search.index'?{}:true}));};
location.hash='#calibrate';configureLanguage('en-US',[]);const root=createRoot(document.getElementById('root')!);root.render(<Launcher plugins={{}} safeMode={false} favorites={[]} refresh={async()=>{}}/>);
(window as any).runInstallReviewChecks=async()=>{try{
 const degenerate=Array.from({length:8},()=>({raw:{x:.5,y:.5},target:{x:.5,y:.5}}));const stable=fitCalibration(degenerate,1440,800);if(!stable||Math.abs(stable.matrix[0]-1)>.001)throw Error('Head-facing identity prior failed');if(fitCalibration(degenerate.slice(0,5),1440,800))throw Error('Insufficient samples accepted');
 // Realistic correlated jitter, intermittent spikes, head motion and one
 // mis-fixated target. Every target has equal influence despite frame counts.
 const noisy=[];
 for(let target=0;target<23;target++){const frames=[];for(let n=0;n<60+(target%3)*6;n++){const yaw=(target%5-2)*.2-Math.sin(n/18)*.15,pitch=(Math.floor(target/5)-2)*.13-Math.cos(n/22)*.08;const wrong=target===22?.18:0;frames.push({raw:{x:(yaw-.045)/.93+Math.sin(n*1.7)*.028+wrong,y:(pitch+.035)/1.06+Math.cos(n*1.3)*.02},target:{x:yaw,y:pitch}});}noisy.push(...calibrationBlocks(frames,target));}
 const robust=fitCalibration(noisy,1440,800);if(!robust)throw Error('Noisy fit failed');
 for(const yaw of [-.35,0,.35]){const q=correctedGaze({x:(yaw-.045)/.93,y:(.12+.035)/1.06},robust);if(Math.hypot(q.x-yaw,q.y-.12)>.012)throw Error('Noise or bad target corrupted model');}
 const identity={space:'headAngles' as const,matrix:[1,0,0,0,1,0],width:1440,height:800,error:0,maxError:0};
 const fixation=Array.from({length:80},(_,n)=>({raw:{x:.2+(n===30?.12:Math.sin(n)*.02),y:.1+Math.cos(n)*.02},target:{x:.2,y:.1}}));
 if(calibrationValidation(fixation,identity).bias>.005)throw Error('One noisy frame invalidated fixation');
 const biased=fixation.map(p=>({...p,raw:{x:p.raw.x+.09,y:p.raw.y}}));if(calibrationValidation(biased,identity).bias<.065)throw Error('Systematic bad gaze wrongly passed');
 const marker=document.createElement('span');marker.style.cssText='position:fixed;left:calc(50% - 2px);top:0;width:4px;height:20px;background:white';document.body.append(marker);await wait(500);const button=(label:string)=>Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(b=>b.textContent===label)!;
 if(document.querySelectorAll('[data-calibration-target]').length!==24)throw Error('Missing calibration targets');
 console.log('FRAMELY_PREVIEW_INSTALL_CALIBRATION_INTRO_EN');
 configureLanguage('zh-CN',[]);root.render(<Launcher plugins={{}} safeMode={false} favorites={[]} refresh={async()=>{}}/>);await wait(160);console.log('FRAMELY_PREVIEW_INSTALL_CALIBRATION_INTRO_ZH');
 configureLanguage('en-US',[]);root.render(<Launcher plugins={{}} safeMode={false} favorites={[]} refresh={async()=>{}}/>);await wait(100);

 let clock=1000;Object.defineProperty(performance,'now',{value:()=>clock,configurable:true});button('Start calibration').click();await wait(100);if(document.querySelector('.calibration-dialog'))throw Error('Intro dialog hid calibration targets');const notice=document.querySelector('.calibration-live')!.getBoundingClientRect();for(const node of document.querySelectorAll('[data-calibration-target]')){const b=node.getBoundingClientRect();if(b.bottom>notice.top&&b.top<notice.bottom&&b.right>notice.left&&b.left<notice.right)throw Error('Progress notice obscured target');}
 let completed=0;
 while(!button('Save calibration')&&completed<32){const node=document.querySelector<HTMLElement>('.calibration-target-active');if(!node)throw Error('No active target');const rect=node.getBoundingClientRect();const tx=(rect.x+rect.width/2)/innerWidth-.5,ty=(.5-(rect.y+rect.height/2)/innerHeight)*.6;const target=calls.filter(r=>r.method==='host.launcher.calibration.target').at(-1)!.params;
  window.dispatchEvent(new CustomEvent('framely.launcher.gaze',{detail:{valid:false,targetId:target.id}}));
  for(let n=0;n<80;n++){clock+=30;const headYaw=Math.sin(n/16)*.25,headPitch=Math.cos(n/19)*.12,translation=Math.sin(n/13)*.025;const yaw=tx-headYaw+translation,pitch=ty-headPitch;const raw=[(yaw-.045)/.93+Math.sin(n)*.001,(pitch+.035)/1.06+Math.cos(n)*.001];const c=Math.cos(headYaw),s=Math.sin(headYaw);window.dispatchEvent(new CustomEvent('framely.launcher.gaze',{detail:{valid:true,targetId:target.id,rawAngles:raw,targetAngles:[yaw,pitch],headPose:[c,0,s,translation,0,1,0,1.65,-s,0,c,0],x:rect.x+rect.width/2,y:rect.y+rect.height/2}}));await wait(3);}
  if(completed===0)console.log('FRAMELY_PREVIEW_INSTALL_CALIBRATION_LIVE_EN');
  await wait(80);completed++;
 }
 if(completed!==29)throw Error('Wrong collection/validation count '+completed);
 await wait(350);console.log('FRAMELY_PREVIEW_INSTALL_CALIBRATION_RESULT_EN');
 button('Save calibration').click();await wait(100);const saved=calls.find(r=>r.method==='launcher.gaze.calibration.save')?.params;if(!saved||saved.error>.005)throw Error('No reliable calibration saved');
 const q=correctedGaze({x:(.4-.045)/.93,y:(.6+.035)/1.06},saved);if(Math.hypot(q.x-.4,q.y-.6)>.005)throw Error('Correction did not recover known offset/scale');
 configureLanguage('zh-CN',[]);root.render(<Launcher plugins={{}} safeMode={false} favorites={[]} refresh={async()=>{}}/>);await wait(350);console.log('FRAMELY_PREVIEW_INSTALL_CALIBRATION_RESULT_ZH');button('完成').click();await wait(150);if(document.querySelector('.launcher-calibration'))throw Error('Calibration did not exit');if(!calls.some(r=>r.method==='host.launcher.calibrate'&&r.params.active===false))throw Error('Native calibration mode not disabled');if(calls.some(r=>r.method==='plugin.launch'))throw Error('Calibration launched real app');
 console.log('FRAMELY_BRIDGE_PASS');
 }catch(e){console.error('FRAMELY_BRIDGE_FAIL '+e);}};
wait(150).then(()=>console.log('FRAMELY_VIEW_READY'));
