import english from '../locales/en-US.json';
import chinese from '../locales/zh-CN.json';
export type LanguagePack={schemaVersion:number;locale:string;name:string;messages:Record<string,string>};
const builtins:LanguagePack[]=[chinese,english];
let installed:LanguagePack[]=[],active=chinese.locale;
export function languages(packs:LanguagePack[]=installed){return [...new Map([...builtins,...packs].map(p=>[p.locale,p])).values()];}
export function resolveLanguage(preference:string,packs:LanguagePack[],system:readonly string[]):string {
 const available=languages(packs);
 if(preference!=='auto')return available.find(p=>p.locale.toLowerCase()===preference.toLowerCase())?.locale??'en-US';
 for(const code of system){const exact=available.find(p=>p.locale.toLowerCase()===code.toLowerCase());if(exact)return exact.locale;const base=available.find(p=>p.locale.split('-')[0].toLowerCase()===code.split('-')[0].toLowerCase());if(base)return base.locale;}
 return 'en-US';
}
export function configureLanguage(preference:string,packs:LanguagePack[],system:readonly string[]=navigator.languages){installed=packs;active=resolveLanguage(preference,packs,system);document.documentElement.lang=active;}
export function currentLanguage(){return active;}
export function t(key:string,params:Record<string,string|number>={}):string {
 const packs=languages();const local=packs.find(p=>p.locale===active);
 const fallback:Record<string,string>=english.messages;
 const text=local?.messages[key]??fallback[key]??key;
 return text.replace(/\{(\d+)\}/g,(match,index)=>Object.hasOwn(params,index)?String(params[index]):match);
}
configureLanguage('auto',[]);
