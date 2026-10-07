import React from 'react';
import {IconFolder,IconInfoCircle} from '@tabler/icons-react';
import {PluginImage} from './store';
import {Select} from './localized-select';
import {t} from './i18n';

type Metadata={name:string;package:string;version:string;versionCode:number;minSdk:number;icon?:string;activities:string[];abis:string[]};
type Props={metadata:Metadata;size:string;previous?:Metadata;updating:boolean;containerName:string;running:boolean;displayMode:string;busy:boolean;error:string;onDisplayMode:(value:string)=>void;onStop:()=>void;onCancel:()=>void;onConfirm:()=>void};
export function ApkInstallReview({metadata,size,previous,updating,containerName,running,displayMode,busy,error,onDisplayMode,onStop,onCancel,onConfirm}:Props){
 const mismatch=!!previous&&previous.package!==metadata.package;
 return <div className="apk-install-review">
  <h2>{t(updating?'确认更新 APK':'确认安装 APK')}</h2>
  <p className="apk-install-intro">{t(updating?'更新应用并保留现有存档和设置。':'核对应用信息，选择合适的显示方式。')}</p>
  <header className="apk-install-app"><PluginImage name={metadata.name} src={metadata.icon} size={60}/><div><h3>{metadata.name}</h3><code>{metadata.package}</code><p>{t('版本')} {previous&&`${previous.version} → `}{metadata.version||t('未声明')} <span>·</span> {size}</p></div></header>
  <section className="apk-install-location" aria-label={t('目标容器')}><IconFolder size={23} aria-hidden="true"/><div><b>{t(updating?'保留原容器':'创建独立容器')}</b>{updating&&<code>{containerName}</code>}<small>{t('每个新应用使用独立容器，更新保留原容器。')}</small></div></section>
  <div className="apk-install-display"><div><b>{t('显示方式')}</b><small>{t(displayMode==='vr'?'隐藏二维窗口，应用本身需要支持 VR。':'通过二维窗口显示并操作应用。')}</small></div><Select label={t('显示方式')} value={displayMode} disabled={busy} onChange={onDisplayMode} options={[{value:'flat',label:t('平面窗口')},{value:'vr',label:t('VR 应用')}]}/></div>
  <details className="apk-install-technical"><summary>{t('查看 APK 详情')}</summary><dl className="apk-meta"><dt>{t('版本代码')}</dt><dd>{metadata.versionCode}</dd><dt>{t('最低 Android SDK')}</dt><dd>{metadata.minSdk||t('未声明')}</dd><dt>{t('架构')}</dt><dd>{metadata.abis.join(', ')||t('未声明')}</dd><dt>{t('启动入口')}</dt><dd>{metadata.activities.length||t('未发现启动入口')}</dd></dl></details>
  {updating&&<div className="apk-install-update-note"><IconInfoCircle size={21} aria-hidden="true"/><div><p>{t('同包名更新保留数据；签名不匹配或降级不会自动卸载重装。')}</p><small>{t('共享容器的备份需手动恢复；独立侧载容器支持一键恢复。')}</small></div></div>}
  {running&&<div className="apk-notice"><p>{t('安装前需要关闭容器，以安全备份应用数据。关闭会中断其中正在运行的应用，保留存档和设置。')}</p><button disabled={busy} onClick={onStop}>{t('关闭容器')}</button></div>}
  {mismatch&&<p className="error" role="alert">{t('APK 包名与原应用不一致，请选择同一应用的更新文件。')}</p>}
  {error&&<div className="error" role="alert"><p>{error}</p>{updating&&!running&&/container|backup|容器|备份/i.test(error)&&<button disabled={busy} onClick={onStop}>{t('关闭容器')}</button>}</div>}
  <footer><button disabled={busy} onClick={onCancel}>{t('取消')}</button><button className="primary" disabled={busy||mismatch} onClick={onConfirm}>{t(updating?'确认更新':'确认安装')}</button></footer>
 </div>;
}
