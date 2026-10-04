import React from 'react';
import {t} from './i18n';
export function MemoryLimit({manifest}:{manifest:any}) {
 if(!manifest.backend&&!manifest.lifecycle)return null;
 return <small className="install-item-source">{t('后端内存上限：{0} MiB（含子进程）',{0:manifest.backend?.memoryLimitMiB??512})}</small>;
}
