// Check routing and migrated settings in the actual manager, not a reproduced page.
import {configureLanguage} from '../ui/src/i18n';
const wait=(ms=80)=>new Promise(r=>setTimeout(r,ms));const until=async(f:()=>any)=>{for(let i=0;i<150;i++){if(f())return;await wait(30);}throw Error('Timed out '+document.body.innerText.slice(0,350));};
history.replaceState(null,'','/manager#launcher-settings');let language='en-US';let launcher:any={primaryTrigger:false,menuAutoClose:true,menuTimeoutSeconds:10,allCategories:['plugin','steam','lepton','desktop']};
window.fetch=async(_url,init)=>{const {method,params}=JSON.parse(String(init?.body));let result:any=true;
 if(method==='status')result={version:'0.4.3-preview.2',build:'0.4.3-preview.2',agreement:{version:1,accepted:true},database:{plugins:{},sources:[],safeMode:false,language,launcher,networkPanel:{enabled:true,port:15915},proxy:{http:'',github:''}},running:[],notifications:[],inbox:[]};
 else if(method==='language.list')result={packs:[],invalidFiles:[]};else if(method==='ui.events')result={cursor:0,events:[]};else if(method==='network.status')result={passwordConfigured:true,localOrigin:'http://localhost:15915',ips:[],listener:{}};else if(method==='launcher.settings.save')launcher=params;else if(method==='language.save')language=params.language;
 return new Response(JSON.stringify({result}));};
const nav=(label:string)=>[...document.querySelectorAll<HTMLButtonElement>('.workspace>nav button')].find(b=>b.textContent===label)!;
(window as any).runInstallReviewChecks=async()=>{try{
 await until(()=>!!document.querySelector('.launcher-settings'));if(nav('Launcher').getAttribute('aria-current')!=='page')throw Error('Hash did not select Launcher');if(document.querySelectorAll('.launcher-settings [role=switch]').length!==6)throw Error('Missing launcher controls');
 nav('Settings').click();await until(()=>!document.querySelector('.launcher-settings'));nav('Launcher').click();await until(()=>!!document.querySelector('.launcher-settings'));
 const switcher=document.querySelector<HTMLInputElement>('.launcher-settings [role=switch]')!;switcher.click();await until(()=>launcher.primaryTrigger);await until(()=>!switcher.disabled);
 const categories=document.querySelectorAll<HTMLInputElement>('.launcher-settings [role=switch]');categories[3].click();await until(()=>!launcher.allCategories.includes('steam'));await until(()=>!categories[3].disabled);
 await wait(450);console.log('FRAMELY_PREVIEW_INSTALL_LAUNCHER_MANAGER_EN');const content=document.querySelector<HTMLElement>('.content')!;content.scrollTop=content.scrollHeight;await wait(300);console.log('FRAMELY_PREVIEW_INSTALL_LAUNCHER_CATEGORIES_EN');content.scrollTop=0;language='zh-CN';configureLanguage(language,[]);location.hash='#launcher-settings';window.dispatchEvent(new Event('hashchange'));await until(()=>document.documentElement.lang==='zh-CN'&&!!nav('启动台'));await wait(450);console.log('FRAMELY_PREVIEW_INSTALL_LAUNCHER_MANAGER_ZH');content.scrollTop=content.scrollHeight;await wait(300);console.log('FRAMELY_PREVIEW_INSTALL_LAUNCHER_CATEGORIES_ZH');
 const first=document.querySelector('.launcher-settings .switch-row')!.getBoundingClientRect();const second=document.querySelectorAll('.launcher-settings')[1].querySelector('.switch-row')!.getBoundingClientRect();if(Math.abs(first.width-second.width)>1)throw Error('Settings widths differ');console.log('FRAMELY_BRIDGE_PASS');
}catch(e){console.error('FRAMELY_BRIDGE_FAIL '+e);}};
void import('../ui/src/main');
