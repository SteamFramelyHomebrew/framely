import React from 'react';import {createRoot} from 'react-dom/client';
import {Launcher} from '../ui/src/launcher';import {configureLanguage} from '../ui/src/i18n';import '../ui/src/style.css';
const wait=(ms:number)=>new Promise(r=>setTimeout(r,ms));const calls:string[]=[];
window.fetch=async(_url,init)=>{const {method}=JSON.parse(String(init?.body));calls.push(method);return new Response(JSON.stringify({result:method==='steam.list'||method==='desktop.list'?[]:method==='launcher.search.index'?{}:true}));};
const plugin:any={manifest:{id:'demo.test',name:'Gaze app',ui:{quickPage:'page.js',windows:{}}},enabled:true,favorite:false,order:0};
createRoot(document.getElementById('root')!).render(<Launcher plugins={{'demo.test':plugin}} safeMode={false} favorites={[]} refresh={async()=>{}}/>);
function send(d:any){window.dispatchEvent(new CustomEvent('framely.launcher.gaze',{detail:d}));}
async function gaze(node:Element,duration=180){const b=node.getBoundingClientRect(),x=b.x+b.width/2,y=b.y+b.height/2;for(let t=0;t<duration;t+=30){send({valid:true,x,y});await wait(30);}}
async function hold(node:Element,duration:number){send({valid:true,phase:'down'});for(let t=0;t<duration;t+=30){send({valid:true,x:0,y:0});await wait(30);}}
(window as any).runInstallReviewChecks=async()=>{try{
 configureLanguage('en-US',[]);const marker=document.createElement('div');marker.style.cssText='position:fixed;left:calc(50% - 2px);top:0;width:4px;height:20px;background:white';document.body.append(marker);await wait(900);
 const icon=()=>document.querySelector<HTMLButtonElement>('.launch-icon[aria-label="Gaze app"]')!;
 await gaze(icon());if(!icon().classList.contains('gaze-focused'))throw Error('No gaze focus');
 const bounds=icon().getBoundingClientRect();for(let n=0;n<16;n++){send({valid:true,x:bounds.right+20+Math.sin(n)*8,y:bounds.y+bounds.height/2+Math.cos(n)*12});await wait(20);}if(!icon().classList.contains('gaze-focused'))throw Error('Edge jitter lost focus');
 send({valid:false});await wait(90);if(!icon().classList.contains('gaze-focused'))throw Error('Brief dropout cleared hover');await gaze(icon());
 send({valid:true,phase:'down'});await gaze(document.querySelector('.launcher-filters button')!,60);send({valid:true,phase:'up'});await wait(80);
 if(calls.filter(m=>m==='plugin.launch').length!==1)throw Error('Press did not lock target');
 await gaze(icon());await hold(icon(),450);send({valid:true,phase:'up'});await wait(80);
 if(!document.querySelector('.launch-menu')||calls.filter(m=>m==='plugin.launch').length!==1)throw Error('400 ms hold did not open only menu');
 window.dispatchEvent(new Event('framely.launcher.cancelInput'));await wait(80);
 await gaze(icon());await hold(icon(),850);send({valid:true,phase:'up'});await wait(80);
 if(!document.querySelector('.launcher-editing'))throw Error('800 ms hold did not edit');
 window.dispatchEvent(new Event('framely.back'));await wait(80);
 await gaze(icon());send({valid:true,phase:'down'});send({valid:false,phase:'cancel'});await wait(450);send({valid:true,phase:'up'});await wait(80);
 if(document.querySelector('.launch-menu')||calls.filter(m=>m==='plugin.launch').length!==1)throw Error('Tracking loss launched or held');
 await gaze(icon());await wait(360);if(icon().classList.contains('gaze-focused'))throw Error('Stale gaze remains active');
 const search=document.querySelector<HTMLInputElement>('.launcher-search input')!;await gaze(search);send({valid:true,phase:'down'});send({valid:true,phase:'up'});if(document.activeElement!==search)throw Error('Search not focused');
 window.dispatchEvent(new CustomEvent('framely.keyboard',{detail:true}));await gaze(icon());if(icon().classList.contains('gaze-focused'))throw Error('Gaze active behind keyboard');
 window.dispatchEvent(new CustomEvent('framely.keyboard',{detail:false}));
 const shortcuts=Array.from(document.querySelectorAll<HTMLButtonElement>('.launcher-shortcuts button'));
 const calibration=shortcuts.find(b=>b.getAttribute('aria-label')==='Eye tracking calibration');
 if(!calibration||shortcuts.indexOf(calibration)!==shortcuts.findIndex(b=>b.getAttribute('aria-label')==='Settings')-1)throw Error('Calibration shortcut is not above settings');
 calibration.click();await wait(80);if(!calls.includes('host.launcher.calibrate'))throw Error('Calibration shortcut did not call native calibration');
 console.log('FRAMELY_BRIDGE_PASS');
 }catch(e){console.error('FRAMELY_BRIDGE_FAIL '+e);}};
wait(150).then(()=>console.log('FRAMELY_VIEW_READY'));
