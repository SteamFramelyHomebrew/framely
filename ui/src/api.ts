import {t} from './i18n';
import {installKeyboard} from '../../sdk/src/keyboard';
import {installHoverFeedback} from '../../sdk/src/hover';
export async function api<T=any>(method:string,params:unknown={},signal?:AbortSignal):Promise<T>{const r=await fetch('/api',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({method,params}),signal});if(r.status===401&&r.headers.get('X-Framely-Reauthenticate')==='1')location.replace('/manager');if(!r.ok)throw new Error(t('服务请求失败：{0}',{0:r.status}));const v=await r.json();if(v.error)throw new Error(t(v.error));return v.result;}
export function viewKey(){const p=location.pathname.split('/');return p[1]==='manager'?'framely.manager':p[1]==='window'?`framely.window.${p[2]}.${p[3]}`:p[1]==='cast'&&p[2]==='window'?`framely.cast.${p[3]}`:p[1]==='notifications'?'notifications':p[1]==='launcher'?'launcher':'menu';}
installKeyboard(params=>api('host.keyboard',{view:viewKey(),...params}));


installHoverFeedback(feedback=>{void api('host.haptic',{view:viewKey(),feedback}).catch(console.error);});

export async function uploadChunk(upload:string,offset:number,chunk:Blob,signal:AbortSignal):Promise<void>{
 const r=await fetch(`/api/upload/${encodeURIComponent(upload)}/${offset}`,{method:'POST',body:chunk,signal});
 if(!r.ok)throw new Error(t('服务请求失败：{0}',{0:r.status}));const v=await r.json();if(v.error)throw new Error(t(v.error));
 if(v.result?.received!==offset+chunk.size)throw new Error('Upload offset mismatch');
}

export async function exportDiagnosticLogs():Promise<string>{
 const v=await api<{name:string;path?:string;data?:string}>('diagnostics.export');
 if(v.path)return t('日志已保存到：{0}',{0:v.path});
 if(!v.data)throw Error(t('日志包缺少数据'));
 const bytes=Uint8Array.from(atob(v.data),c=>c.charCodeAt(0));
 const url=URL.createObjectURL(new Blob([bytes],{type:'application/zip'}));
 const link=document.createElement('a');link.href=url;link.download=v.name;
 document.body.appendChild(link);link.click();link.remove();setTimeout(()=>URL.revokeObjectURL(url),60000);
 return t('日志包已生成，请查看浏览器下载。');
}
