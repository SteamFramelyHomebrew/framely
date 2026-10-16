import React,{useEffect,useState} from 'react';
import {IconFile,IconFolder,IconLock} from '@tabler/icons-react';
import {managerApi} from './manager-api';
import {t} from './i18n';
export type FilePropertiesData={name:string;path:string;directory:boolean;regular?:boolean;symlink:boolean;size:number;allocated?:number;modified?:number;accessed?:number;changed?:number;created?:number;mode:number;uid:number;gid:number;owner?:string;group?:string;inode?:string;device?:string;links?:number;linkTarget?:string;resolvedPath?:string;canChmod?:boolean};
export const validFileMode=(value:string)=>/^0?[0-7]{3}$/.test(value);
export function permissionText(mode:number){return [8,7,6,5,4,3,2,1,0].map((bit,i)=>{const present=!!(mode&(1<<bit));if(i===2&&(mode&0o4000))return present?'s':'S';if(i===5&&(mode&0o2000))return present?'s':'S';if(i===8&&(mode&0o1000))return present?'t':'T';return present?'rwx'[i%3]:'-';}).join('');}
const size=(n:number)=>`${n.toLocaleString()} B${n>=1024?` (${(n/1024**(n>=1024**3?3:n>=1024**2?2:1)).toFixed(2)} ${n>=1024**3?'GiB':n>=1024**2?'MiB':'KiB'})`:''}`;
export function FileProperties({entry,onClose,onSaved}:{entry:FilePropertiesData;onClose:()=>void;onSaved:()=>void}){
 const [data,setData]=useState(entry),[mode,setMode]=useState((entry.mode&0o777).toString(8).padStart(3,'0')),[loading,setLoading]=useState(true),[busy,setBusy]=useState(false),[error,setError]=useState('');
 useEffect(()=>{let alive=true;setLoading(true);void managerApi<FilePropertiesData>('files',{operation:'properties',path:entry.path}).then(value=>{if(alive){setData(value);setMode((value.mode&0o777).toString(8).padStart(3,'0'));}}).catch(e=>{if(alive)setError(String(e));}).finally(()=>{if(alive)setLoading(false);});return()=>{alive=false;};},[entry.path]);
 const date=(value?:number)=>value===undefined||value===null?t('不可用'):new Date(value*1000).toLocaleString(document.documentElement.lang,{year:'numeric',month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit',second:'2-digit'});
 const writable=!loading&&!busy&&data.canChmod===true&&!data.symlink&&!!data.inode&&!!data.device;
 const numeric=validFileMode(mode)?parseInt(mode,8):null;
 const changed=numeric!==null&&numeric!==(data.mode&0o777);
 const rows:[[string,React.ReactNode],...[string,React.ReactNode][]]=[
  ['完整路径',<code>{data.path}</code>],['文件类型',t(data.symlink?'符号链接':data.directory?'文件夹':data.regular===false?'特殊文件':'普通文件')],
  [data.directory?'目录自身大小（不含内容）':'文件大小',size(data.size)],['磁盘占用',data.allocated===undefined?t('不可用'):size(data.allocated)],
  ['所有者',`${data.owner??t('未知')} (UID ${data.uid})`],['所属组',`${data.group??t('未知')} (GID ${data.gid})`],
  ['修改时间',date(data.modified)],['访问时间',date(data.accessed)],['状态变更时间',date(data.changed)],['创建时间',date(data.created)],
  ['Inode',data.inode??t('不可用')],['设备编号',data.device??t('不可用')],['硬链接数',data.links??t('不可用')]
 ];
 if(data.linkTarget)rows.splice(2,0,['链接目标',<code>{data.linkTarget}</code>]);
 if(data.symlink)rows.splice(3,0,['解析后的路径',<code>{data.resolvedPath??t('不可用')}</code>]);
 async function save(){if(!writable||numeric===null||!changed)return;setBusy(true);setError('');try{await managerApi('files',{operation:'chmod',path:data.path,mode:numeric,identity:{device:data.device,inode:data.inode}});onSaved();onClose();}catch(e){setError(String(e));}finally{setBusy(false);}}
 return <div className="modal file-properties-modal" role="dialog" aria-modal="true" aria-label={t('文件属性')} aria-busy={loading||busy}>
  <header className="file-properties-heading">{data.directory?<IconFolder size={30}/>:<IconFile size={30}/>}<div><h2>{data.name}</h2><p>{t('文件属性')}</p></div></header>
  {loading&&<p role="status">{t('正在读取属性…')}</p>}
  {error&&<p className="error" role="alert">{error}</p>}
  <dl className="file-property-grid">{rows.map(([label,value])=><div key={label}><dt>{t(label)}</dt><dd>{value}</dd></div>)}</dl>
  <section className="file-permissions"><h3><IconLock size={19}/>{t('访问权限')} <code>{(data.mode&0o7000).toString(8)}{numeric===null?'???':mode.replace(/^0(?=\d{3}$)/,'')} / {permissionText((data.mode&0o7000)|(numeric??(data.mode&0o777)))}</code></h3>
   <div className="file-permission-grid"><span/>{['读取','写入','执行／进入'].map(label=><span key={label}>{t(label)}</span>)}{['所有者','所属组','其他用户'].map((label,row)=><React.Fragment key={label}><strong>{t(label)}</strong>{[4,2,1].map((bit,col)=>{const mask=bit<<(6-row*3);return <button key={bit} type="button" role="checkbox" aria-label={`${t(label)}: ${t(['读取','写入','执行／进入'][col])}`} aria-checked={numeric!==null&&!!(numeric&mask)} disabled={!writable||numeric===null} onClick={()=>setMode((numeric!^mask).toString(8).padStart(3,'0'))}>{numeric!==null&&(numeric&mask)?'✓':'-'}</button>;})}</React.Fragment>)}</div>
   <label>{t('权限（八进制）')}<input value={mode} maxLength={4} inputMode="numeric" disabled={!writable} aria-invalid={!validFileMode(mode)} onChange={e=>setMode(e.target.value)}/></label>
   {!validFileMode(mode)&&<p className="error" role="alert">{t('请输入 000 至 777 的八进制权限。')}</p>}
   {!loading&&(data.canChmod===false||data.symlink)&&<p>{t(data.symlink?'符号链接权限不可修改；此处显示链接本身的属性。':'当前用户不是文件所有者，无法修改权限。')}</p>}
   {!!(data.mode&0o7000)&&<p>{t('现有特殊权限位会保留，本面板仅修改读、写、执行权限。')}</p>}
  </section>
  <footer><button disabled={busy} onClick={onClose}>{t('关闭')}</button><button className="primary" disabled={!writable||!validFileMode(mode)||!changed} onClick={()=>void save()}>{t('保存权限')}</button></footer>
 </div>;
}
