import React from 'react';
import {createRoot} from 'react-dom/client';
import {SteamLaunchChoice} from '../ui/src/steam-launch-choice';
import {configureLanguage,t} from '../ui/src/i18n';
import '../ui/src/style.css';
import '../ui/src/launcher.css';
const root=createRoot(document.getElementById('root')!);
const wait=()=>new Promise(r=>setTimeout(r,450));
let chosen:string|undefined='unset',closed=false;
async function show(locale:string){configureLanguage(locale,[]);root.render(<div className="app launcher-shell" style={{height:'100%',background:'#161618'}}><SteamLaunchChoice name="Hades" local targets={[{client:'12',name:'TOORU-PC'},{client:'13',name:'Living Room PC'}]} onChoose={client=>chosen=client} onClose={()=>closed=true}/></div>);await wait();return document.querySelector('.launch-choice')!;}
(window as any).runInstallReviewChecks=async()=>{try{
 let dialog=await show('zh-CN');
 if(!dialog.textContent?.includes('选择启动方式')||!dialog.textContent.includes('本机启动')||!dialog.textContent.includes('TOORU-PC')||!dialog.textContent.includes('Living Room PC'))throw Error('Missing localized labels or device names');
 if(dialog.scrollWidth>dialog.clientWidth+1)throw Error('Horizontal overflow');
 let buttons=dialog.querySelectorAll<HTMLButtonElement>('.launch-choice-target');buttons[2].click();if(chosen!=='13')throw Error('Wrong remote host selected');buttons[0].click();if(chosen!==undefined)throw Error('Local launch still passes remote host');
 console.log('FRAMELY_PREVIEW_INSTALL_CHINESE');await wait();
 dialog=await show('en-US');if(!dialog.textContent?.includes('Choose how to launch'))throw Error('English missing');
 console.log('FRAMELY_PREVIEW_INSTALL_ENGLISH');await wait();
 dialog.querySelector<HTMLButtonElement>('.icon-button')!.click();if(!closed)throw Error('Close action broken');
 console.log('FRAMELY_BRIDGE_PASS');
}catch(e){console.error('FRAMELY_BRIDGE_FAIL '+e)}};
show('zh-CN').then(()=>console.log('FRAMELY_VIEW_READY'));
