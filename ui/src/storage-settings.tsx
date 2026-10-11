import React, {useEffect, useState} from 'react';
import {IconArrowLeft, IconChevronRight, IconDeviceMobile, IconFolder, IconFolderOpen, IconLoader2, IconPackage, IconPuzzle, IconX} from '@tabler/icons-react';
import {api} from './api';
import {managerApi} from './manager-api';
import {t} from './i18n';
import './storage-settings.css';

type Kind = 'plugin' | 'apk' | 'contexts';
type Detail = {id: string; name: string; bytes: number; programBytes?: number; dataBytes?: number; sizeIncomplete?: boolean};
type Location = {path: string; bytes: number; device?: number | null; details?: Detail[]; sizeIncomplete?: boolean; available?: boolean; interrupted?: string | null};
type Places = Record<Kind, Location>;
type Volume = {total: number; free: number; device: number};
type Job = {phase: string; step?: string; error?: string; result?: {path: string; cleanupWarnings?: string[]}};
type Listing = {path: string; parent: string | null; roots: {name: string; path: string}[]; entries: {name: string; path: string; directory: boolean}[]};

const kinds: Kind[] = ['plugin', 'apk', 'contexts'];
const kindIcons = {plugin: IconPuzzle, apk: IconPackage, contexts: IconDeviceMobile};
const labels: Record<Kind, {title: string; description: string; folder: string}> = {
  plugin: {title: '插件', description: '插件程序和运行数据', folder: 'Framely-Plugins'},
  apk: {title: 'APK 文件', description: '安装包、上传文件和备份', folder: 'Framely-APK'},
  contexts: {title: '安卓容器', description: 'Framely 创建的 Lepton 容器和应用数据', folder: 'Framely-Android'},
};
const steps: Record<string, string> = {
  queued: '准备迁移', loading: '准备迁移', stopping: '停止相关程序',
  copying: '复制数据', verifying: '核对文件', switching: '切换目录',
  restarting: '恢复运行', cleaning: '清理旧目录',
};
function size(n: number) {
  return n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(1)} KiB`
    : n < 1073741824 ? `${(n / 1048576).toFixed(1)} MiB` : `${(n / 1073741824).toFixed(1)} GiB`;
}
async function browse(path: string): Promise<Listing> {
  return managerApi<Listing>('files', {operation: 'list', path: path || undefined, hidden: false});
}

function FolderBrowser({close, choose}: {close: () => void; choose: (path: string) => void}) {
  const [listing, setListing] = useState<Listing | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  async function open(path: string) {
    setLoading(true);
    setError('');
    try { setListing(await browse(path)); }
    catch (e) { setError(String(e)); }
    finally { setLoading(false); }
  }
  useEffect(() => { void open(''); }, []);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') close(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [close]);
  return <div className="storage-picker-backdrop" onClick={event => { if (event.target === event.currentTarget) close(); }}>
    <div className="storage-picker" role="dialog" aria-modal="true" aria-label={t('选择目标目录')}>
      <header><div><h2>{t('选择目标目录')}</h2><p>{t('选择上级目录，Framely 会在其中创建专用文件夹。')}</p></div><button className="icon-button" aria-label={t('关闭')} onClick={close}><IconX size={22}/></button></header>
      <div className="storage-picker-location"><button className="icon-button" disabled={!listing?.parent || loading} onClick={() => listing?.parent && void open(listing.parent)} aria-label={t('上级目录')}><IconArrowLeft size={22}/></button><code title={listing?.path}>{listing?.path ?? t('正在读取目录…')}</code></div>
      {!!listing?.roots.length && <div className="storage-picker-roots">{listing.roots.map(root => <button key={root.path} disabled={loading} onClick={() => void open(root.path)}>{root.name}</button>)}</div>}
      <div className="storage-picker-folders">{listing?.entries.filter(entry => entry.directory).map(entry => <button key={entry.path} disabled={loading} onClick={() => void open(entry.path)}><IconFolder size={21}/><span>{entry.name}</span></button>)}{!loading && listing && !listing.entries.some(entry => entry.directory) && <p className="sub">{t('此目录下没有文件夹')}</p>}{loading && <p className="sub" role="status">{t('正在读取目录…')}</p>}</div>
      {error && <p role="alert" className="error">{error}</p>}
      <footer><button onClick={close}>{t('取消')}</button><button className="primary" disabled={!listing || loading} onClick={() => { choose(listing!.path); close(); }}>{t('选择此位置')}</button></footer>
    </div>
  </div>;
}

export function StorageSettings() {
  const [places, setPlaces] = useState<Places | null>(null);
  const [volume, setVolume] = useState<Volume | null>(null);
  const [paths, setPaths] = useState<Record<Kind, string>>({plugin: '', apk: '', contexts: ''});
  const [editing, setEditing] = useState<Kind | null>(null);
  const [picker, setPicker] = useState<Kind | null>(null);
  const [active, setActive] = useState<Kind | null>(null);
  const [job, setJob] = useState<{kind: Kind; id: string} | null>(null);
  const [step, setStep] = useState('');
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [loading, setLoading] = useState(true);

  async function refresh() {
    const [plugin, other] = await Promise.all([
      api<Location>('storage.plugin.status'),
      api<{apk: Location; contexts: Location; volume: Volume; interrupted?: {kind: Kind; target: string} | null}>('apk.storage.status'),
    ]);
    const next: Places = {
      plugin,
      apk: {...other.apk, interrupted: other.interrupted?.kind === 'apk' ? other.interrupted.target : null},
      contexts: {...other.contexts, interrupted: other.interrupted?.kind === 'contexts' ? other.interrupted.target : null},
    };
    setPlaces(next);
    setVolume(other.volume);
    setPaths(previous => ({
      plugin: previous.plugin || plugin.interrupted || plugin.path,
      apk: previous.apk || next.apk.interrupted || next.apk.path,
      contexts: previous.contexts || next.contexts.interrupted || next.contexts.path,
    }));
  }
  async function reload() {
    setLoading(true);
    setError('');
    try { await refresh(); }
    catch (e) { setError(String(e)); }
    finally { setLoading(false); }
  }
  useEffect(() => { void reload(); }, []);
  useEffect(() => {
    if (!job) return;
    let alive = true;
    const poll = async () => {
      try {
        const value = await api<Job>(job.kind === 'plugin' ? 'storage.plugin.job' : 'job.status', {job: job.id});
        if (!alive) return;
        setStep(value.step ?? value.phase);
        if (value.phase === 'done') {
          setJob(null); setActive(null); setEditing(null);
          setNotice(value.result?.cleanupWarnings?.length ? t('迁移完成。旧目录未能清理，请检查原位置。') : t('迁移完成'));
          setPaths(previous => ({...previous, [job.kind]: value.result?.path ?? previous[job.kind]}));
          await refresh();
        } else if (value.phase === 'failed' || value.phase === 'cancelled') {
          setJob(null); setActive(null);
          setError(value.error ?? t('迁移失败'));
          await refresh();
        }
      } catch (e) { if (alive) setError(String(e)); }
    };
    void poll();
    const timer = window.setInterval(() => void poll(), 750);
    return () => { alive = false; window.clearInterval(timer); };
  }, [job]);
  async function migrate(kind: Kind) {
    setError(''); setNotice(''); setActive(kind);
    try {
      const result = await api<{job: string}>(kind === 'plugin' ? 'storage.plugin.start' : 'apk.storage.start', kind === 'plugin' ? {path: paths[kind]} : {kind, path: paths[kind]});
      setJob({kind, id: result.job}); setStep('queued');
    } catch (e) { setError(String(e)); setActive(null); }
  }
  function edit(kind: Kind) {
    setError(''); setNotice(''); setEditing(kind);
    if (places) setPaths(previous => ({...previous, [kind]: places[kind].interrupted || places[kind].path}));
  }
  return <div className="storage-settings">
    <div className="list-heading"><div><h1>{t('存储')}</h1><p>{t('管理插件、APK 文件和安卓容器的存放位置。')}</p></div></div>
    {notice && <p className="banner" role="status">{notice}</p>}
    {error && <p className="error" role="alert">{error}{!places && <button onClick={() => void reload()}>{t('重试')}</button>}</p>}
    {loading && !places && <div className="storage-loading" role="status" aria-label={t('正在读取存储位置')}>{kinds.map(kind => <div key={kind}><i/><span/><small/></div>)}</div>}
    {places && volume && (() => {
      const onVolume = (kind: Kind) => places[kind].bytes === 0 || places[kind].device === volume.device;
      const managed = kinds.reduce((sum, kind) => sum + (onVolume(kind) ? places[kind].bytes : 0), 0);
      const free = Math.min(volume.free, volume.total);
      const used = Math.max(0, volume.total - free);
      const other = Math.max(0, used - managed);
      const segments = [
        ...kinds.map(kind => ({key: kind, label: t(labels[kind].title), bytes: onVolume(kind) ? places[kind].bytes : 0, displayBytes: places[kind].bytes, moved: !onVolume(kind)})),
        {key: 'other', label: t('系统及其他'), bytes: other, displayBytes: other, moved: false},
        {key: 'free', label: t('空闲'), bytes: free, displayBytes: free, moved: false},
      ];
      return <section className="settings-card storage-overview" aria-labelledby="storage-overview-title">
        <div className="storage-overview-heading"><div><h2 id="storage-overview-title">{t('存储占用')}</h2><p className="sub">{t('头显用户存储卷')}</p></div><div className="storage-overview-total"><strong>{size(volume.total)}</strong><span>{t('总容量')}</span></div></div>
        <div className="storage-meter" role="img" aria-label={`${t('已使用')} ${size(used)}，${t('空闲')} ${size(free)}`}>{segments.map(segment => <span key={segment.key} className={`storage-meter-${segment.key}`} style={{width: `${volume.total ? segment.bytes / volume.total * 100 : 0}%`}} title={`${segment.label} ${size(segment.bytes)}`}/>)}</div>
        <div className="storage-overview-usage"><span>{t('已使用')} <strong>{size(used)}</strong></span><span>{t('空闲')} <strong>{size(free)}</strong></span></div>
        <div className="storage-legend">{segments.map(segment => <div key={segment.key} className="storage-legend-item"><i className={`storage-dot-${segment.key}`}/><span>{segment.label}{segment.moved && <small> · {t('其他磁盘')}</small>}</span><strong>{size(segment.displayBytes)}</strong></div>)}</div>
        {kinds.some(kind => !onVolume(kind)) && <p className="storage-overview-note">{t('已迁移到其他磁盘的数据仅显示在下方明细，不计入此卷。')}</p>}
        {kinds.some(kind => places[kind].sizeIncomplete) && <p className="storage-overview-note">{t('部分目录无法读取，Framely 占用可能低于实际值。')}</p>}
      </section>;
    })()}
    {places && kinds.map(kind => {
      const place = places[kind];
      const busy = !!job || !!active;
      const changing = editing === kind;
      const migrating = active === kind;
      const unchanged = !paths[kind] || paths[kind] === place.path;
      const details = [...(place.details ?? [])].sort((a, b) => b.bytes - a.bytes);
      const remainder = Math.max(0, place.bytes - details.reduce((sum, detail) => sum + detail.bytes, 0));
      const detailCount = details.length + (remainder > 0 ? 1 : 0);
      const KindIcon = kindIcons[kind];
      return <section className="settings-card storage-section" key={kind} aria-labelledby={`storage-${kind}`}>
        <div className="storage-section-layout">
          <div className="storage-section-heading">
            <span className={`storage-kind-icon storage-kind-${kind}`} aria-hidden="true"><KindIcon size={26} stroke={1.7}/></span>
            <div><h2 id={`storage-${kind}`}>{t(labels[kind].title)}</h2><p className="sub">{t(labels[kind].description)}</p></div>
            <span className="storage-size" title={place.sizeIncomplete ? t('部分目录无法读取，显示可统计的最小占用') : undefined}>{place.sizeIncomplete ? '≥ ' : ''}{size(place.bytes)}</span>
          </div>
          <div className="storage-section-content">
            <div className="storage-location">
              <IconFolder size={22} stroke={1.7} aria-hidden="true"/>
              <div><span>{t('当前位置')}</span><code title={place.path}>{place.path}</code></div>
              <button disabled={busy || place.available === false} onClick={() => changing ? setEditing(null) : edit(kind)}>{changing ? t('取消更改') : place.interrupted ? t('继续迁移') : t('更改位置')}</button>
            </div>
            {place.available === false && <p className="storage-warning" role="status">{t('当前目录不可用，请先连接存储设备。')}</p>}
            {place.interrupted && <p className="storage-warning" role="status">{t('上次迁移未完成，原目录仍在使用。可重试原目标或选择新的空目录。')}<code>{place.interrupted}</code></p>}
            {changing && <div className="storage-change">
              <label htmlFor={`storage-target-${kind}`}>{t('新位置')}</label>
              <div className="storage-target"><input id={`storage-target-${kind}`} value={paths[kind]} disabled={busy} onChange={event => setPaths(previous => ({...previous, [kind]: event.target.value}))} spellCheck={false}/><button disabled={busy} onClick={() => setPicker(kind)}><IconFolderOpen size={21}/>{t('浏览')}</button></div>
              <p className="sub">{t('目标须为空目录，并支持 Linux 文件权限。迁移期间相关程序会暂时停止。')}</p>
              <div className="storage-actions"><button className="primary" disabled={busy || place.available === false || unchanged} onClick={() => void migrate(kind)}>{t('迁移到新位置')}</button></div>
            </div>}
            {migrating && <div className="storage-progress" role="status"><IconLoader2 size={20}/><span>{t(steps[step] ?? step)}</span></div>}
            <details className="storage-details">
              <summary><span>{t('占用明细')}</span><small>{t('{0} 项', {'0': detailCount})}</small><IconChevronRight size={19} aria-hidden="true"/></summary>
              <div className="storage-detail-list">
                {details.map(detail => <div className="storage-detail-row" key={detail.id}><div><strong>{detail.name || detail.id}</strong>{detail.id !== detail.name && <small title={detail.id}>{detail.id}</small>}{kind === 'plugin' && <small>{t('程序')} {size(detail.programBytes ?? 0)} · {t('数据')} {size(detail.dataBytes ?? 0)}</small>}</div><span title={detail.sizeIncomplete ? t('部分目录无法读取，显示可统计的最小占用') : undefined}>{detail.sizeIncomplete ? '≥ ' : ''}{size(detail.bytes)}</span></div>)}
                {remainder > 0 && <div className="storage-detail-row storage-detail-other"><div><strong>{t(kind === 'apk' ? '备份、上传文件及其他' : kind === 'plugin' ? '其他插件文件' : '其他容器文件')}</strong></div><span>{size(remainder)}</span></div>}
                {detailCount === 0 && <p className="storage-empty">{t('暂无数据')}</p>}
              </div>
            </details>
          </div>
        </div>
      </section>;
    })}
    {picker && <FolderBrowser close={() => setPicker(null)} choose={parent => setPaths(previous => ({...previous, [picker]: `${parent.replace(/\/$/, '')}/${labels[picker].folder}`}))}/>}
  </div>;
}
