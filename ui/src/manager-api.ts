import {t} from './i18n';
export class ManagerError extends Error {constructor(public readonly raw:string){super(t(raw));}}
export async function managerApi<T=any>(area:string,params:unknown={},signal?:AbortSignal):Promise<T>{const r=await fetch('/manager-api/'+area,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(params),signal});if(r.status===401){location.replace('/manager');throw new Error(t('请重新登录'));}if(!r.ok)throw new Error(t('服务请求失败：{0}',{0:r.status}));const v=await r.json();if(v.error)throw new ManagerError(v.error);return v.result;}
