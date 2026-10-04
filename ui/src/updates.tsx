import {t} from './i18n';
import React,{useEffect,useState} from 'react';
import {api} from './api';
import {runJob,Job,compareVersions} from './store';
import {Select} from './localized-select';
type UpdateChannel='stable'|'testing';
type UpdateSource={url:string};
export function SystemUpdates({status,refresh}:{status:any;refresh:()=>Promise<void>}){
 const source:UpdateSource|null=status.database.updateSource??null;
 const channel:UpdateChannel=status.database.updateChannel??'stable';
 const[error,setError]=useState(''),[busy,setBusy]=useState(false),[job,setJob]=useState<Job|null>(null),[release,setRelease]=useState<any>(null),[ready,setReady]=useState(false),[checked,setChecked]=useState(false),[confirm,setConfirm]=useState<'apply'|'rollback'|null>(null),[installing,setInstalling]=useState(false);
 function reset(){setRelease(null);setReady(false);setChecked(false);setConfirm(null);}
 useEffect(reset,[source?.url,channel]);
 const progress=status.systemUpdate;
 const older=release&&compareVersions(release.targetVersion??release.version,status.version)===-1;
 async function switchChannel(value:string){
  if(!source||busy||installing||value===channel)return;
  setBusy(true);setError('');reset();
  try{await api('system.channel.save',{channel:value});}
  catch(e){setError(String(e));}
  finally{await refresh();setBusy(false);}
 }
 async function run(method:string,params:unknown={},onDone?:(value:any)=>void){
  setBusy(true);setError('');
  try{const v=await runJob(method,params,setJob,'system.job.status');onDone?.(v.result);}
  catch(e){setError(String(e));}
  finally{setBusy(false);setJob(null);await refresh();}
 }
 return <section className="settings-card system-updates">
  <h2>{t('Framely 更新')}</h2>
  <p>{t('当前版本：')}<b>{status.build??status.version}</b></p>
  <p className="sub">{t('安装更新会保留插件和数据，Framely 界面会暂时关闭并重新启动。')}</p>
  {!source&&<p className="banner">{t('此版本尚未配置更新服务，请通过发行安装包更新。')}</p>}
  <Select label={t('更新渠道')} value={channel} disabled={busy||installing||!source} onChange={value=>void switchChannel(value)} options={[{value:'stable',label:t('正式版')},{value:'testing',label:t('测试版')}]}/>
  <p className="sub">{channel==='stable'?t('仅检查正式版。'):t('仅检查测试版（Preview、Beta、RC 等）。')}</p>
  <div className="row">
   <button className="primary" disabled={busy||installing||!source} onClick={()=>{reset();void run('system.check.start',{},v=>{setRelease(v.release?{...v.release,targetVersion:v.version}:null);setReady(false);setChecked(true);});}}>{busy?t('处理中…'):t('检查更新')}</button>
   <button disabled={busy||installing||!status.previousRelease} onClick={()=>setConfirm('rollback')}>{t('回滚上一版本')}</button>
  </div>
  {checked&&!release&&<p className="banner" role="status">{t('所选渠道暂无可用发行版本。')}</p>}
  {job&&<p className="banner">{job.kind==='system.check'?t('正在读取更新清单…'):t('正在下载并校验发行包…')}</p>}
  {progress?.phase==='downloading'&&<div className="download-progress"><progress max={progress.total??1} value={progress.received??0}/><p>{((progress.received??0)/1024/1024).toFixed(1)} / {((progress.total??0)/1024/1024).toFixed(1)} MiB</p></div>}
  {progress?.phase==='failed'&&<p className="error">{t('上次更新失败：')}{progress.error}<small>{t('诊断日志：/var/lib/framely/logs/update.log')}</small></p>}
  {progress?.phase==='done'&&<p className="banner">{t('上次更新操作已完成。')}</p>}
  {release&&<div className="release-card">
   <h3>{release.version}</h3>
   <p>{release.version===(status.build??status.version)?t('当前已是此发行版本'):t('发行包大小：{0} MiB',{'0':(release.size/1024/1024).toFixed(1)})}</p>
   {older&&<p className="banner">{t('所选版本早于当前版本，安装将切换到较旧版本。')}</p>}
   <p className="prose">{release.changelog||t('发布者未提供更新说明。')}</p>
   {release.version!==(status.build??status.version)&&<button className="primary" disabled={busy||installing} onClick={()=>ready?setConfirm('apply'):void run('system.download.start',{},()=>setReady(true))}>{ready?t('安装已校验版本'):t('下载并校验')}</button>}
  </div>}
  {error&&<p className="error" role="alert">{error}</p>}
  {installing&&<p className="banner">{t('正在切换版本。Framely 入口会在服务恢复后重新出现。')}</p>}
  {confirm&&<div className="confirm-box">
   <h3>{confirm==='apply'?t('安装 Framely {0}？',{'0':release.version}):t('回滚 Framely？')}</h3>
   <p>{t('插件和数据会保留，Framely 界面会暂时关闭。SteamVR 不会被重启。')}</p>
   {confirm==='apply'&&older&&<p className="banner">{t('所选版本早于当前版本，安装将切换到较旧版本。')}</p>}
   <div className="row">
    <button className="primary" disabled={installing||busy} onClick={()=>void(async()=>{setInstalling(true);setError('');try{await api(confirm==='apply'?'system.apply':'system.rollback',{approve:true,version:release?.version});setConfirm(null);}catch(e){setError(String(e));setInstalling(false);}})()}>{confirm==='apply'?t('确认安装'):t('确认回滚')}</button>
    <button disabled={installing} onClick={()=>setConfirm(null)}>{t('取消')}</button>
   </div>
  </div>}
 </section>;
}
