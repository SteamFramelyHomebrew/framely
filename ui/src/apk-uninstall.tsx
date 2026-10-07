import React,{useState} from 'react';
import {exportDiagnosticLogs} from './api';
import {IconAlertTriangle} from '@tabler/icons-react';
import {PluginImage} from './store';
import {SwitchRow} from './switch';
import {t} from './i18n';

type Props={
 app:{installed:boolean;metadata:{name:string;package:string;icon?:string}};
 size:string;purge:boolean;deleteContainer:boolean;exclusive:boolean;
 busy:boolean;error:string;onPurge:(value:boolean)=>void;
 onDeleteContainer:(value:boolean)=>void;onCancel:()=>void;onConfirm:()=>void;
};

export function ApkUninstall({app,size,purge,deleteContainer,exclusive,busy,error,onPurge,onDeleteContainer,onCancel,onConfirm}:Props){
 const [exporting,setExporting]=useState(false),[diagnostic,setDiagnostic]=useState('');
 async function exportLogs(){setExporting(true);try{setDiagnostic(await exportDiagnosticLogs());}catch(e){setDiagnostic(String(e));}finally{setExporting(false);}}
 const message=deleteContainer?'将永久删除应用及容器数据。历史备份保留。':purge?'将永久删除存档和应用数据。容器保留。':'存档和设置会保留，之后可以重新安装。';
 return <div className="apk-uninstall">
  <h2>{t(app.installed?'卸载应用':'清除保留数据')}</h2>
  <p className="apk-uninstall-intro">{t('对应的 Steam 库条目、图标和封面也会移除。')}</p>
  <header className="apk-uninstall-app">
   <PluginImage kind="lepton" name={app.metadata.name} src={app.metadata.icon} size={60}/>
   <div><h3>{app.metadata.name}</h3><code>{app.metadata.package}</code><small>{t('空间占用')} {size}</small></div>
  </header>
  <div className="apk-uninstall-options" role="group" aria-label={t('卸载选项')}>
   <SwitchRow label={t('删除应用数据')} description={t('删除存档、设置及应用下载的数据。')} checked={purge} disabled={busy||!app.installed} onChange={onPurge}/>
   <SwitchRow label={t('删除容器')} description={t(!exclusive?'容器包含其他应用或状态未知，无法同时删除。':!purge?'开启删除应用数据后可选择。':'此容器仅包含该应用，同时清理 APK 缓存及关联文件。')} checked={deleteContainer} disabled={busy||!exclusive||!purge} onChange={onDeleteContainer}/>
  </div>
  <div className={`apk-uninstall-result ${purge?'destructive':''}`} role="status">
   {purge&&<IconAlertTriangle size={22} aria-hidden="true"/>}<p>{t(message)}{purge&&<small>{t('永久删除的数据无法撤销。')}</small>}</p>
  </div>
  {error&&<p className="error" role="alert">{error}</p>}
  {diagnostic&&<p role="status">{diagnostic}</p>}
  <footer>{error&&<button disabled={busy||exporting} onClick={()=>void exportLogs()}>{t(exporting?'正在导出日志…':'导出日志')}</button>}<button disabled={busy} onClick={onCancel}>{t('取消')}</button><button className="danger" disabled={busy} onClick={onConfirm}>{error?t('重试'):t(app.installed?(deleteContainer?'卸载并删除容器':'卸载'):(deleteContainer?'删除数据和容器':'删除'))}</button></footer>
 </div>;
}
