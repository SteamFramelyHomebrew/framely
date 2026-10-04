// Preview and interaction checks for the real install review component.
import React from 'react';
import {createRoot} from 'react-dom/client';
import {InstallReview} from '../ui/src/install-review';
import {configureLanguage,t} from '../ui/src/i18n';
import '../ui/src/style.css';
const root=createRoot(document.getElementById('root')!);
const wait=(ms=600)=>new Promise(resolve=>setTimeout(resolve,ms));
const plugin={id:'tooru.termix',name:'Termix',author:'tooru',version:'0.1.7',description:'通过内网浏览器管理 Frame，提供终端、文件管理与远程桌面功能。',details:'所有设置位于快捷面板。打开大窗口可显示 Termix；内网浏览器仍需账号密码。Web 账号与系统账号分离。',homepage:'https://github.com/Termix-SSH/Termix',backend:{runAs:'steamos'}};
const camera={id:'framely.framexr-camera',name:'FrameXR Camera',author:'tooru',version:'0.1.9',description:'在 VR 中显示设备相机画面，并控制相机采集与显示设置。',backend:{runAs:'root',memoryLimitMiB:2048}};
function info(manifest:any,dependencies:any[]=[]){return {manifest,installedVersion:manifest===plugin?'0.1.6':undefined,plan:{items:[{manifest,action:manifest===plugin?'update':'install'},...dependencies.map(manifest=>({manifest,action:'install'}))],addSources:[],disable:[],enable:[manifest.id],affected:[manifest.id]}};}
let installed=0,cancelled=0;
async function show(value:any,overrides:any={}) {
 root.render(<div className="app"><InstallReview title={t(value.installedVersion?'更新 {0}':'安装 {0}',{0:value.manifest.name})} info={value} busy={false} error="" confirmed={false} onConfirmChange={()=>{}} onChoose={()=>{}} onInstall={()=>installed++} onCancel={()=>cancelled++} {...overrides}/></div>);
 await wait();
 const modal=document.querySelector('.install-review')!,footer=modal.querySelector('footer')!;
 const bounds=modal.getBoundingClientRect(),actions=footer.getBoundingClientRect();
 if(bounds.top<0||bounds.bottom>innerHeight+1||actions.bottom>bounds.bottom+1)throw Error('dialog or actions overflow viewport');
 if(modal.scrollWidth>modal.clientWidth+1)throw Error('dialog horizontal overflow');
 for(const button of footer.querySelectorAll('button'))if(button.getBoundingClientRect().height>65)throw Error('action wrapped');
 return modal;
}
(window as any).runInstallReviewChecks=async()=>{
 try {
  configureLanguage('zh-CN',[]);
  let modal=await show(info(plugin));
  if(!modal.textContent?.includes(plugin.description)||!modal.querySelector('.plugin-links'))throw Error('plugin description or links missing');
  if(modal.querySelector('.is-root'))throw Error('steamos shows root warning');
  if(!modal.textContent?.includes('512 MiB'))throw Error('default memory limit missing');
  if(modal.textContent?.includes('启用依赖链：')||modal.textContent?.includes('其他受影响的插件：'))throw Error('redundant single-plugin effects');
  console.log('FRAMELY_PREVIEW_INSTALL_STEAMOS');
  (modal.querySelector('button.primary') as HTMLButtonElement).click();
  (modal.querySelector('footer button') as HTMLButtonElement).click();
  if(installed!==1||cancelled!==1)throw Error('actions not connected');
  const details=modal.querySelector('details')!;details.open=true;await wait();
  if(details.getBoundingClientRect().height<60)throw Error('full description not expandable');
  modal=await show(info(camera));
  if(!modal.querySelector('.install-run-user.is-root')||!modal.textContent?.includes('请确认你信任此插件'))throw Error('root warning missing');
  if(modal.querySelector('.install-run-user')!.getBoundingClientRect().height>130&&innerWidth>700)throw Error('root warning occupies too much space');
  if(!modal.textContent?.includes('2048 MiB'))throw Error('custom memory limit missing');
  console.log('FRAMELY_PREVIEW_INSTALL_ROOT');
  modal=await show(info(plugin,[camera]));
  if(!modal.querySelector('.install-root-dependencies')||!modal.querySelector('.run-user-badge.is-root'))throw Error('root dependency hidden');
  console.log('FRAMELY_PREVIEW_INSTALL_DEPENDENCY');
  modal=await show({...info(camera),runAsChanged:true});
  if(!(modal.querySelector('button.primary') as HTMLButtonElement).disabled)throw Error('run user change bypassed');
  modal=await show({...info(camera),runAsChanged:true},{confirmed:true});
  if((modal.querySelector('button.primary') as HTMLButtonElement).disabled)throw Error('confirmed user change blocked');
  modal=await show(info(camera),{busy:true});
  if([...modal.querySelectorAll('footer button')].some(button=>!(button as HTMLButtonElement).disabled))throw Error('busy actions enabled');
  modal=await show(info(camera),{error:'安装失败，请重新检查安装包。'});
  if(!modal.querySelector('[role=alert]'))throw Error('inline error missing');
  configureLanguage('en-US',[]);await show(info(camera));
  console.log('FRAMELY_PREVIEW_INSTALL_ENGLISH');
  console.log('FRAMELY_BRIDGE_PASS');
 }catch(error){console.error('FRAMELY_BRIDGE_FAIL '+error);}
};
show(info(plugin)).then(()=>console.log('FRAMELY_VIEW_READY'));
