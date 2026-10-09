import React from 'react';
import {IconPuzzle,IconBrandAndroid,IconTerminal2,IconFolder,IconSettings,IconCast} from '@tabler/icons-react';
import {t} from './i18n';
export type ManagerSection='plugins'|'apk'|'terminal'|'files'|'casting'|'settings';
export function sectionFor(tab:string):ManagerSection {
 if(['casting','casting-picture','casting-receive','casting-devices'].includes(tab))return 'casting';
 if(tab.startsWith('apk'))return 'apk';
 if(tab==='terminal'||tab==='files')return tab;
 return ['settings','launcher-settings','about','updates'].includes(tab)?'settings':'plugins';
}
export function managerTab(hash:string){const key=hash.slice(1);return ['casting','casting-picture','casting-receive','casting-devices','installed','catalog','sources','notification-settings','settings','launcher-settings','about','updates','terminal','files','apk','apk-containers','apk-cleanup','apk-settings'].includes(key)?key:'installed';}
export function ManagerSwitcher({section,onChange}:{section:ManagerSection;onChange:(section:ManagerSection)=>void}) {
 return <div className="manager-switcher" role="group" aria-label={t('切换管理面板')}>
  {([['plugins','插件'],['apk','APK'],['terminal','终端'],['files','文件'],['casting','投屏'],['settings','设置']] as const).map(([key,label])=><button key={key} className={section===key?'active':''} aria-pressed={section===key} onClick={()=>onChange(key)}><span className="manager-section-icon">{React.createElement(({plugins:IconPuzzle,apk:IconBrandAndroid,terminal:IconTerminal2,files:IconFolder,casting:IconCast,settings:IconSettings})[key],{size:22})}</span><span>{t(label)}</span></button>)}
 </div>;
}
