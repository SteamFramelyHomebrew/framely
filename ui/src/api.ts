import {t} from './i18n';
import {installKeyboard} from '../../sdk/src/keyboard';
import {installHoverFeedback} from '../../sdk/src/hover';
export async function api<T=any>(method:string,params:unknown={},signal?:AbortSignal):Promise<T>{const r=await fetch('/api',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({method,params}),signal});if(r.status===401&&r.headers.get('X-Framely-Reauthenticate')==='1')location.replace('/manager');if(!r.ok)throw new Error(t('服务请求失败：{0}',{0:r.status}));const v=await r.json();if(v.error)throw new Error(t(v.error));return v.result;}
export function viewKey(){const p=location.pathname.split('/');return p[1]==='manager'?'framely.manager':p[1]==='window'?`framely.window.${p[2]}.${p[3]}`:p[1]==='notifications'?'notifications':p[1]==='launcher'?'launcher':'menu';}
installKeyboard(params=>api('host.keyboard',{view:viewKey(),...params}));


installHoverFeedback(()=>{void api('host.haptic',{view:viewKey()}).catch(console.error);});

export async function uploadChunk(upload:string,offset:number,chunk:Blob,signal:AbortSignal):Promise<void>{
 const r=await fetch(`/api/upload/${encodeURIComponent(upload)}/${offset}`,{method:'POST',body:chunk,signal});
 if(!r.ok)throw new Error(t('服务请求失败：{0}',{0:r.status}));const v=await r.json();if(v.error)throw new Error(t(v.error));
 if(v.result?.received!==offset+chunk.size)throw new Error('Upload offset mismatch');
}
