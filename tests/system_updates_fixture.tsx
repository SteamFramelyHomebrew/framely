// Exercise confirmation, download-to-install chaining, failures and channels in the real UI.
// Run: bash tools/test-install-review.sh /path/to/cef tests/system_updates_fixture.tsx
import React,{useState} from 'react';
import {createRoot} from 'react-dom/client';
import {SystemUpdates} from '../ui/src/updates';
import {configureLanguage} from '../ui/src/i18n';
import '../ui/src/style.css';
const root=createRoot(document.getElementById('root')!);
const wait=(ms=70)=>new Promise(resolve=>setTimeout(resolve,ms));
let status:any={version:'0.4.3-preview.1',build:'0.4.3-preview.1-012345abcdef',database:{updateSource:{url:'https://github.com/example/framely/releases/latest/download/framely-release.json'}},previousRelease:'releases/old'};
let updateStatus:React.Dispatch<React.SetStateAction<any>>,remount:()=>void;
let kind='',empty=false,hold=false,verifying=false,failCheck=false,failDownload=false,failApply=false;
let checks=0,downloads=0,attempts=0,installs=0,rollbacks=0;
const release=()=>({version:(status.database.updateChannel==='testing'?'0.4.3-preview.2':'0.4.2')+'-abcdef012345',size:1024*1024,changelog:'Fixture release notes'});
window.fetch=async(_url,init)=>{
 const {method,params}=JSON.parse(String(init?.body));
 let result:any=true;
 if(method==='system.channel.save')status={...status,database:{...status.database,updateChannel:params.channel}};
 else if(method==='system.check.start'){kind='system.check';checks++;result={job:'fixture'};}
 else if(method==='system.download.start'){kind='system.download';downloads++;result={job:'fixture'};}
 else if(method==='system.job.status'){
  if(kind==='system.check'&&failCheck)result={kind,phase:'failed',error:'Fixture check failed'};
  else if(kind==='system.download'&&failDownload)result={kind,phase:'failed',error:'Fixture checksum failed'};
  else if(hold)result={kind,phase:verifying?'verifying':'downloading',received:1024*1024,verified:512*1024,total:1024*1024};
  else result={kind,phase:'done',result:kind==='system.check'?{release:empty?null:release(),version:status.database.updateChannel==='testing'?'0.4.3-preview.2':'0.4.2'}:release()};
 }else if(method==='system.apply'){
  if(params.version!==release().version||!params.approve)throw Error('incorrect install confirmation');
  attempts++;
  if(failApply)return new Response(JSON.stringify({error:'Fixture installation launch failed'}),{headers:{'Content-Type':'application/json'}});
  installs++;
 }else if(method==='system.rollback'){
  if(!params.approve)throw Error('incorrect rollback confirmation');
  rollbacks++;
 }else if(method!=='host.haptic'&&method!=='host.keyboard')throw Error('unexpected API '+method);
 return new Response(JSON.stringify({result}),{headers:{'Content-Type':'application/json'}});
};
function Fixture(){const[value,setValue]=useState(status),[instance,setInstance]=useState(0);updateStatus=setValue;remount=()=>setInstance(v=>v+1);return <div className="app"><main className="content"><SystemUpdates key={instance} status={value} refresh={async()=>{setValue({...status});}}/></main></div>;}
function button(text:string){const found=[...document.querySelectorAll('button')].find(b=>b.textContent===text);if(!found)throw Error('missing button '+text);return found as HTMLButtonElement;}
async function until(check:()=>boolean){for(let i=0;i<80;i++){if(check())return;await wait();}throw Error('UI wait timed out');}
async function choose(label:string){(document.querySelector('.framely-select button') as HTMLButtonElement).click();await wait();const option=[...document.querySelectorAll('[role=option]')].find(o=>o.textContent===label);if(!option)throw Error('missing channel '+label);(option as HTMLButtonElement).click();await wait();}
(window as any).runInstallReviewChecks=async()=>{
 try{
  configureLanguage('zh-CN',[]);updateStatus({...status});await wait();
  if(!document.querySelector('.framely-select button')?.textContent?.includes('正式版'))throw Error('legacy source did not default to stable');
  button('检查更新').click();await until(()=>!!document.querySelector('.release-card'));
  if(!document.querySelector('.release-card')?.textContent?.includes('较旧版本'))throw Error('channel downgrade not identified');
  button('下载并安装').focus();button('下载并安装').click();await wait();
  const dialog=document.querySelector('.update-confirmation') as HTMLDialogElement|null;
  if(downloads!==0||attempts!==0||!dialog?.open||!dialog.matches(':modal'))throw Error('update bypassed modal confirmation');
  if(!dialog.getAttribute('aria-labelledby')||!dialog.getAttribute('aria-describedby')||document.activeElement!==button('取消'))throw Error('confirmation lacked accessible focus or description');
  const bounds=dialog.getBoundingClientRect();
  if(bounds.width<=0||bounds.height<=0||bounds.left<0||bounds.top<0||bounds.right>innerWidth||bounds.bottom>innerHeight)throw Error('confirmation outside viewport');
  button('检查更新').focus();
  if(!dialog.contains(document.activeElement))throw Error('background took focus during confirmation');
  button('取消').click();await wait();
  if(downloads!==0||attempts!==0||document.querySelector('.update-confirmation')||document.activeElement!==button('下载并安装'))throw Error('cancelled update performed work or did not restore focus');
  button('下载并安装').click();await wait();
  document.querySelector('.update-confirmation')!.dispatchEvent(new Event('cancel',{cancelable:true}));await wait();
  if(document.querySelector('.update-confirmation')||downloads!==0||attempts!==0)throw Error('dialog cancellation performed work');
  failDownload=true;button('下载并安装').click();await wait();button('确认下载并安装').click();
  await until(()=>!!document.querySelector('[role=alert]'));
  if(attempts!==0||!document.querySelector('[role=alert]')?.textContent?.includes('checksum failed'))throw Error('failed verification did not stop installation');
  failDownload=false;failApply=true;button('下载并安装').click();await wait();button('确认下载并安装').click();
  await until(()=>!!document.querySelector('[role=alert]')?.textContent?.includes('installation launch failed'));
  if(attempts!==1||downloads!==2)throw Error('verified download did not automatically attempt installation');
  console.log('FRAMELY_PREVIEW_INSTALL_UPDATE_STABLE');
  button('安装已校验版本').click();await wait();button('确认安装').click();await until(()=>attempts===2);await wait();
  if(downloads!==2)throw Error('installation retry downloaded again');
  await choose('测试版');
  if(status.database.updateChannel!=='testing'||document.querySelector('.release-card'))throw Error('channel not saved or stale release retained');
  button('检查更新').click();await until(()=>!!document.querySelector('.release-card'));
  if(document.querySelector('.release-card')?.textContent?.includes('较旧版本'))throw Error('new preview reported as downgrade');
  if([...document.querySelectorAll('button')].some(b=>b.textContent==='安装已校验版本'))throw Error('download reused across channels');
  hold=true;failApply=false;button('下载并安装').click();await wait();button('确认下载并安装').click();await until(()=>downloads===3);await wait();
  if(!(document.querySelector('.framely-select button') as HTMLButtonElement).disabled||!button('处理中…').disabled)throw Error('settings mutable during download');
  if(attempts!==2)throw Error('installation started before verification');
  await until(()=>!!document.querySelector('progress[aria-label="下载进度"]'));
  verifying=true;
  await until(()=>!!document.querySelector('progress[aria-label="校验进度"]'));
  if(!document.body.textContent?.includes('下载完成，正在校验发行包')||document.querySelector('progress[aria-label="下载进度"]'))throw Error('verification phase still shown as downloading');
  const verification=document.querySelector('progress[aria-label="校验进度"]') as HTMLProgressElement;
  if(verification.value!==512*1024||verification.max!==1024*1024||attempts!==2)throw Error('verification byte progress or installation gate incorrect');
  console.log('FRAMELY_PREVIEW_INSTALL_UPDATE_TESTING');
  hold=false;await until(()=>installs===1);
  if(attempts!==3||document.querySelector('.update-confirmation'))throw Error('verified download required a second confirmation');
  remount();await wait();
  failCheck=true;button('检查更新').click();await until(()=>!!document.querySelector('[role=alert]'));
  if(document.querySelector('.release-card'))throw Error('failed recheck retained stale install');
  failCheck=false;empty=true;await choose('正式版');button('检查更新').click();await until(()=>!!document.querySelector('[role=status]'));
  if(!document.body.textContent?.includes('暂无可用发行'))throw Error('empty stable channel not shown');
  empty=false;configureLanguage('en-US',[]);updateStatus({...status});await wait();
  if(!document.querySelector('.framely-select button')?.textContent?.includes('Stable'))throw Error('English channel untranslated');
  await choose('Testing');button('Check for updates').click();await until(()=>!!document.querySelector('.release-card'));
  button('Download and install').click();await wait();
  if(!document.querySelector('.update-confirmation')?.textContent?.includes('installed automatically'))throw Error('English update flow untranslated');
  hold=true;button('Confirm download and installation').click();
  await until(()=>!!document.querySelector('progress[aria-label="Verification progress"]'));
  if(!document.body.textContent?.includes('Download complete. Verifying')||!document.body.textContent?.includes('Verified 0.5 / 1.0 MiB'))throw Error('English verification progress untranslated');
  hold=false;await until(()=>installs===2);
  if(checks!==5||downloads!==4||attempts!==4)throw Error(`unexpected workflow counts ${checks}/${downloads}/${attempts}`);
  console.log('FRAMELY_PREVIEW_INSTALL_UPDATE_ENGLISH');
  remount();await wait();button('Roll back to previous release').click();await wait();
  if(rollbacks!==0)throw Error('rollback bypassed confirmation');
  button('Confirm rollback').click();await until(()=>rollbacks===1);
  if(downloads!==4)throw Error('rollback downloaded a release');
  console.log('FRAMELY_BRIDGE_PASS');
 }catch(error){console.error('FRAMELY_BRIDGE_FAIL '+error);}
};
root.render(<Fixture/>);
wait(150).then(()=>console.log('FRAMELY_VIEW_READY'));
