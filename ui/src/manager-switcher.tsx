import React from 'react';
import {t} from './i18n';
export function ManagerSwitcher({apk,onChange}:{apk:boolean;onChange:(apk:boolean)=>void}) {
 return <div className="manager-switcher" role="group" aria-label={t('切换管理面板')}>
  <button className={!apk?'active':''} aria-pressed={!apk} onClick={()=>onChange(false)}>{t('插件管理')}</button>
  <button className={apk?'active':''} aria-pressed={apk} onClick={()=>onChange(true)}>{t('APK 管理')}</button>
 </div>;
}
