import React,{useState} from 'react';
import {api} from './api';
import {Select} from './localized-select';
import {t,languages,resolveLanguage,LanguagePack} from './i18n';
export function LanguageSettings({value,packs,invalidFiles,refresh}:{value:string;packs:LanguagePack[];invalidFiles:string[];refresh:()=>Promise<void>}){
 const[busy,setBusy]=useState(false),[error,setError]=useState('');
 async function select(language:string){setBusy(true);setError('');try{await api('language.save',{language});await refresh();}catch(e){setError((e as Error).message);}finally{setBusy(false);}}
 async function install(file:File){setBusy(true);setError('');try{if(file.size>1024*1024)throw new Error(t('语言文件超过 1 MiB'));const pack=JSON.parse(await file.text());await api('language.install',{pack});await refresh();}catch(e){setError((e as Error).message);}finally{setBusy(false);}}

 return <section className="settings-card language-settings"><h2>{t('语言')}</h2><div className="row"><LanguagePicker value={value} packs={packs} disabled={busy} onChange={select}/><label className="language-import">{t('安装语言文件')}<input aria-label={t('安装语言文件')} disabled={busy} type="file" accept=".json,application/json" onChange={e=>{const file=e.target.files?.[0];e.target.value='';if(file)void install(file);}}/></label><a href="/assets/locales/en-US.json" download="framely-language-template.json">{t('下载语言文件模板')}</a></div>{invalidFiles.length>0&&<p className="error">{t('无法读取语言文件：{0}',{0:invalidFiles.join(', ')})}</p>}{error&&<p className="error" role="alert">{error}</p>}</section>;
}

export function LanguagePicker({value,packs,disabled,onChange}:{value:string;packs:LanguagePack[];disabled?:boolean;onChange:(language:string)=>void}){
 const options=languages(packs);const systemLanguage=resolveLanguage('auto',packs,navigator.languages);const name=options.find(p=>p.locale===systemLanguage)?.name??systemLanguage;
 return <div className="language-picker"><Select label={t('界面语言')} disabled={disabled} value={value} onChange={onChange} options={[{value:"auto",label:t('跟随系统（{0}）',{0:name})},...options.map(p=>({value:p.locale,label:p.name}))]}/></div>;
}
