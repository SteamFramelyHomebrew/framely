// Exercise channel persistence, stale-download invalidation and confirmation in the real UI.
// Run: bash tools/test-install-review.sh /path/to/cef tests/system_updates_fixture.tsx
import React,{useState} from 'react';
import {createRoot} from 'react-dom/client';
import {SystemUpdates} from '../ui/src/updates';
import {configureLanguage} from '../ui/src/i18n';
import '../ui/src/style.css';
const root=createRoot(document.getElementById('root')!);
const wait=(ms=70)=>new Promise(resolve=>setTimeout(resolve,ms));
let status:any={version:'0.4.3-preview.1',build:'0.4.3-preview.1-012345abcdef',database:{updateSource:{url:'https://github.com/example/framely/releases/latest/download/framely-release.json'}},previousRelease:'releases/old'};
let updateStatus:React.Dispatch<React.SetStateAction<any>>;
let kind='',empty=false,hold=false,fail=false;
let checks=0,downloads=0,installs=0;
const release=()=>({version:(status.database.updateChannel==='testing'?'0.4.3-preview.2':'0.4.2')+'-abcdef012345',size:1024*1024,changelog:'Fixture release notes'});
window.fetch=async(_url,init)=>{
 const {method,params}=JSON.parse(String(init?.body));
 let result:any=true;
 if(method==='system.channel.save')status={...status,database:{...status.database,updateChannel:params.channel}};
 else if(method==='system.check.start'){kind='system.check';checks++;result={job:'fixture'};}
 else if(method==='system.download.start'){kind='system.download';downloads++;result={job:'fixture'};}
 else if(method==='system.job.status'){
  if(fail)result={kind,phase:'failed',error:'Fixture check failed'};
  else if(hold)result={kind,phase:'loading'};
  else result={kind,phase:'done',result:kind==='system.check'?{release:empty?null:release(),version:status.database.updateChannel==='testing'?'0.4.3-preview.2':'0.4.2'}:{}};
 }else if(method==='system.apply'){if(params.version!==release().version||!params.approve)throw Error('incorrect install confirmation');installs++;}
 else if(method!=='host.haptic'&&method!=='host.keyboard')throw Error('unexpected API '+method);
 return new Response(JSON.stringify({result}),{headers:{'Content-Type':'application/json'}});
};
function Fixture(){const[value,setValue]=useState(status);updateStatus=setValue;return <div className="app"><main className="content"><SystemUpdates status={value} refresh={async()=>{setValue({...status});}}/></main></div>;}
function button(text:string){const found=[...document.querySelectorAll('button')].find(b=>b.textContent===text);if(!found)throw Error('missing button '+text);return found as HTMLButtonElement;}
async function until(check:()=>boolean){for(let i=0;i<80;i++){if(check())return;await wait();}throw Error('UI wait timed out');}
async function choose(label:string){(document.querySelector('.framely-select button') as HTMLButtonElement).click();await wait();const option=[...document.querySelectorAll('[role=option]')].find(o=>o.textContent===label);if(!option)throw Error('missing channel '+label);(option as HTMLButtonElement).click();await wait();}
(window as any).runInstallReviewChecks=async()=>{
 try{
  configureLanguage('zh-CN',[]);updateStatus({...status});await wait();
  if(!document.querySelector('.framely-select button')?.textContent?.includes('正式版'))throw Error('legacy source did not default to stable');
  button('检查更新').click();await until(()=>!!document.querySelector('.release-card'));
  if(!document.querySelector('.release-card')?.textContent?.includes('较旧版本'))throw Error('channel downgrade not identified');
  button('下载并校验').click();await until(()=>!![...document.querySelectorAll('button')].find(b=>b.textContent==='安装已校验版本'));
  console.log('FRAMELY_PREVIEW_INSTALL_UPDATE_STABLE');
  await choose('测试版');
  if(status.database.updateChannel!=='testing'||document.querySelector('.release-card'))throw Error('channel not saved or stale release retained');
  button('检查更新').click();await until(()=>!!document.querySelector('.release-card'));
  if(document.querySelector('.release-card')?.textContent?.includes('较旧版本'))throw Error('new preview reported as downgrade');
  if([...document.querySelectorAll('button')].some(b=>b.textContent==='安装已校验版本'))throw Error('download reused across channels');
  hold=true;button('下载并校验').click();await wait();
  if(!(document.querySelector('.framely-select button') as HTMLButtonElement).disabled)throw Error('channel mutable during download');
  hold=false;await until(()=>!![...document.querySelectorAll('button')].find(b=>b.textContent==='安装已校验版本'));
  console.log('FRAMELY_PREVIEW_INSTALL_UPDATE_TESTING');
  button('安装已校验版本').click();await wait();
  if(installs!==0||!document.querySelector('.confirm-box'))throw Error('install bypassed confirmation');
  button('取消').click();await wait();
  fail=true;button('检查更新').click();await until(()=>!!document.querySelector('[role=alert]'));
  if(document.querySelector('.release-card'))throw Error('failed recheck retained stale install');
  fail=false;empty=true;await choose('正式版');button('检查更新').click();await until(()=>!!document.querySelector('[role=status]'));
  if(!document.body.textContent?.includes('暂无可用发行'))throw Error('empty stable channel not shown');
  empty=false;configureLanguage('en-US',[]);updateStatus({...status});await wait();
  if(!document.querySelector('.framely-select button')?.textContent?.includes('Stable'))throw Error('English channel untranslated');
  await choose('Testing');button('Check for updates').click();await until(()=>!!document.querySelector('.release-card'));
  button('Download and verify').click();await until(()=>!![...document.querySelectorAll('button')].find(b=>b.textContent==='Install verified release'));
  button('Install verified release').click();await wait();button('Confirm installation').click();await until(()=>installs===1);
  if(checks!==5||downloads!==3)throw Error(`unexpected workflow counts ${checks}/${downloads}`);
  console.log('FRAMELY_PREVIEW_INSTALL_UPDATE_ENGLISH');
  console.log('FRAMELY_BRIDGE_PASS');
 }catch(error){console.error('FRAMELY_BRIDGE_FAIL '+error);}
};
root.render(<Fixture/>);
wait(150).then(()=>console.log('FRAMELY_VIEW_READY'));
