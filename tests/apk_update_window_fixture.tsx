import React from 'react';
import {createRoot} from 'react-dom/client';
import {ApkUpdate} from '../ui/src/apk-update';
import {configureLanguage} from '../ui/src/i18n';
import '../ui/src/style.css';
const id='0123456789abcdef0123456789abcdef';
const metadata={package:'com.example.game',name:'Update test game',version:'1.0',versionCode:1,minSdk:24,activities:['com.example.game.Main'],declaredActivities:['com.example.game.Main'],abis:['arm64-v8a'],vr:false};
const app={id:'test/com.example.game',context:'test',metadata,installed:true,running:false,managed:true,stateKnown:true,size:100};
const request={id,app:app.id,name:metadata.name,previous:'1.0',version:'2.0',bytes:200};
let language='zh-CN';
let pending=true,fail=false,installs=0,closes=0,reviews=0,jobCounter=0,receipts=0;
const jobs=new Map<string,any>();
window.fetch=async(_url,init)=>{
 const {method,params:p}=JSON.parse(String(init?.body));let result:any=true;
 if(method==='status')result={database:{language}};
 else if(method==='language.list')result={packs:[]};
 else if(method==='apk.updates.list')result=pending?[request]:[];
 else if(method==='apk.q2f.status')result={id:'test',bytes:100,cached:true};
 else if(method==='apk.review.drop')result=true;
 else if(method==='apk.updates.window.ready'){if(p.id!==id||!document.querySelector('.apk-install-review'))throw Error('Receipt preceded rendered confirmation');receipts++;}
 else if(method==='apk.updates.window.close'){if(p.id!==id)throw Error('Wrong close identity');closes++;}
 else if(method==='job.status')result=jobs.get(p.job);
 else if(method==='apk.list.start'||method==='apk.updates.review'||method==='apk.operation.start'){
  let value:any;
  if(method==='apk.list.start')value={apps:[app],containers:[{id:'test',name:'test',managed:true,running:false}],roots:[],warnings:[],available:true};
  if(method==='apk.updates.review'){reviews++;if(p.id!==id)throw Error('Wrong review identity');value={ticket:'review-'+reviews,metadata:{...metadata,version:'2.0',versionCode:2},bytes:200};}
  if(method==='apk.operation.start'){
   if(p.operation!=='install'||p.updateRequest!==id||p.app!==app.id||p.context!=='test'||!p.approve)throw Error('Confirmation lost normal installation parameters');
   installs++;if(!fail){pending=false;value={backup:'preserved-test-backup'};}
  }
  const job='job-'+jobCounter++;jobs.set(job,{id:job,kind:'apk',phase:method==='apk.operation.start'&&fail?'failed':'done',error:'Fixture signature mismatch',result:value,received:0});result={job};
 }
 return new Response(JSON.stringify({result}));
};
const root=createRoot(document.getElementById('root')!);
const wait=()=>new Promise(r=>setTimeout(r,30));
async function until(check:()=>unknown){for(let i=0;i<250;i++){if(check())return;await wait();}throw Error('Timeout: '+document.body.innerText);}
const button=(label:string)=>Array.from(document.querySelectorAll<HTMLButtonElement>('button')).find(b=>b.textContent?.trim()===label)!;
(window as any).runInstallReviewChecks=async()=>{
 try{
  await until(()=>!!button('确认更新'));
  if(document.documentElement.lang!=='zh-CN')throw Error('Saved Chinese language was ignored');
  await until(()=>receipts===1);
  if(installs!==0||reviews!==1)throw Error('Update installed without explicit confirmation');
  if(document.querySelector('.apk-tabs')||document.querySelector('.list-heading'))throw Error('Management panel leaked into update confirmation');
  if(!document.body.textContent?.includes('1.0 → 2.0'))throw Error('Version comparison missing');
  document.dispatchEvent(new MouseEvent('mousemove',{clientX:150,clientY:180,bubbles:true}));
  await until(()=>document.querySelector('.apk-update-pointer'));
  const cursor=document.querySelector<HTMLElement>('.apk-update-pointer')!;
  const bounds=cursor.getBoundingClientRect();
  if(Math.abs(bounds.x+bounds.width/2-150)>1||Math.abs(bounds.y+bounds.height/2-180)>1)throw Error('Laser marker does not match hit coordinates');
  if(getComputedStyle(cursor).pointerEvents!=='none'||Number(getComputedStyle(cursor).zIndex)<10000)throw Error('Laser marker obscures input or sits behind confirmation');
  if(document.elementFromPoint(150,180)===cursor)throw Error('Marker intercepts clicks');
  document.dispatchEvent(new MouseEvent('mouseleave'));
  await until(()=>!document.querySelector('.apk-update-pointer'));
  console.log('FRAMELY_PREVIEW_INSTALL_APP_UPDATE');
  button('取消').click();await until(()=>closes===1);
  if(!pending||installs)throw Error('Cancel deleted or installed the queued update');
  language='en-US';root.render(<ApkUpdate key="retry" id={id}/>);
  await until(()=>reviews===2&&!!button('Confirm update'));
  fail=true;button('Confirm update').click();
  await until(()=>document.body.textContent?.includes('Fixture signature mismatch'));
  if(!pending||closes!==1)throw Error('Failed installation lost queued APK or closed confirmation');
  await until(()=>!!button('Confirm update')&&!button('Confirm update').disabled);
  fail=false;button('Confirm update').click();
  await until(()=>!pending&&!!button('Close')&&!document.querySelector('.apk-install-review'));
  if(installs!==2)throw Error('Unexpected install submission count');
  button('Close').click();await until(()=>closes===2);
  console.log('FRAMELY_BRIDGE_PASS');
 }catch(error){console.error('FRAMELY_BRIDGE_FAIL '+error);}
};
configureLanguage('en-US',[]);document.documentElement.classList.add('apk-update-view');root.render(<ApkUpdate id={id}/>);console.log('FRAMELY_VIEW_READY');
