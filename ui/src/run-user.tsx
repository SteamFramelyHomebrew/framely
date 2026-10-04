import React,{useId} from 'react';
import {t} from './i18n';
export type RuntimeUser='steamos'|'root';
export function runUser(manifest:any):RuntimeUser {
 return manifest.backend?.runAs??manifest.lifecycle?.runAs??'steamos';
}
export function RunUserBadge({manifest,user}:{manifest?:any;user?:RuntimeUser|null}) {
 const value=user??(manifest?runUser(manifest):undefined);
 return <span className={`run-user-badge ${value==='root'?'is-root':''}`} aria-label={value?t('运行用户：{0}',{0:value}):t('运行用户未标明')}>{value??t('未标明')}</span>;
}
export function RunUserNotice({user,descriptionId}:{user?:RuntimeUser|null;descriptionId?:string}) {
 const labelId=useId();
 return <section className={`install-run-user run-user-notice ${user==='root'?'is-root':''}`} aria-labelledby={labelId}>
  <div className="install-run-heading"><span id={labelId}>{t('运行用户')}</span><strong className="install-account">{user??t('未标明')}</strong>{user&&<span className="install-account-kind">{user==='root'?t('系统管理员'):t('Steam 会话用户')}</span>}</div>
  <p id={descriptionId}>{user==='root'?t('可修改系统文件和设备设置，请确认你信任此插件。'):user==='steamos'?t('可访问 Steam 会话用户的文件，并以该用户操作设备资源。'):t('来源未提供运行用户，请在安装确认时查看包内信息。')}</p>
 </section>;
}
