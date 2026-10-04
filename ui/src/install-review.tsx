import React from 'react';
import {t} from './i18n';
import {InstallPlan} from './relations';
import {MemoryLimit} from './memory-limit';
import {PluginAuthor,PluginLinks} from './store';
import {runUser,RunUserNotice} from './run-user';

type Props={title:string;info:any;busy:boolean;error:string;confirmed:boolean;onConfirmChange:(value:boolean)=>void;onInstall:()=>void;onCancel:()=>void;onChoose:(id:string,url:string)=>void};
export function InstallReview({title,info,busy,error,confirmed,onConfirmChange,onInstall,onCancel,onChoose}:Props) {
 const manifest=info.manifest,user=runUser(manifest);
 const rootItems=(info.plan?.items??[{manifest}]).filter((item:any)=>runUser(item.manifest)==='root');
 const rootNames=rootItems.map((item:any)=>item.manifest.name).join('、');
 const includesRoot=rootItems.length>0;
 return <div className={`modal install-review ${includesRoot?'includes-root':''}`} role="dialog" aria-modal="true" aria-label={title} aria-describedby="install-run-description">
  <header className="install-review-heading">
   <h2>{title}</h2>
   <div className="install-package-meta"><span>{t('开发者：')}<PluginAuthor plugin={manifest}/></span><span className="install-version">{info.installedVersion&&<><span className="sub">{info.installedVersion}</span><span aria-label={t('目标版本')}>→</span></>}<strong>{manifest.version}</strong></span></div>
  </header>
  <div className="install-review-body">
   <RunUserNotice user={user} descriptionId="install-run-description"/>
   {includesRoot&&user!=='root'&&<section className="install-root-dependencies" role="note"><div><strong>{t('root 插件：')}</strong><span>{rootNames}</span></div><p>{t('这些插件可修改系统和设备设置，请确认来源可信。')}</p></section>}
   <section className="install-plugin-info" aria-label={t('插件介绍')}>
    <h3>{t('插件介绍')}</h3>
    <p className="install-description">{manifest.description||t('开发者尚未提供插件描述。')}</p>
    <PluginLinks plugin={manifest}/>
    {manifest.details&&manifest.details!==manifest.description&&<details className="install-details"><summary>{t('完整介绍')}</summary><p>{manifest.details}</p></details>}
   </section>
   {!info.plan&&<MemoryLimit manifest={manifest}/>}
   <InstallPlan info={info} choose={onChoose}/>
   {info.runAsChanged&&<label className="install-user-confirm"><input type="checkbox" checked={confirmed} onChange={e=>onConfirmChange(e.target.checked)}/><span>{t('我确认变更运行用户')}</span></label>}
   {error&&<p className="error" role="alert">{error}</p>}
  </div>
  <footer className="install-review-footer">
   <span className="install-footer-note">{includesRoot?t('包含系统管理员访问'):t('以 steamos 用户运行')}</span>
   <div className="install-review-actions"><button disabled={busy} onClick={onCancel}>{t('取消')}</button><button className="primary" disabled={busy||!!info.choiceRequired||(info.runAsChanged&&!confirmed)} onClick={onInstall}>{busy?t('正在处理…'):t('确认安装')}</button></div>
  </footer>
 </div>;
}
