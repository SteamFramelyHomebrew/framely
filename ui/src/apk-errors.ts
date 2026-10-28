import {t} from './i18n';

// Keep the original diagnostic chain (including paths) below a localized outcome.
export function apkError(error:unknown):string {
 const text=String(error).replace(/^Error: /,'');
 for(const [prefix,key] of [
  ['New installation failed; newly created container was cleaned up: ','安装失败，本次新建的容器已清理：{0}'],
  ['Installation failed; container retained because safe cleanup failed: ','安装失败，安全清理未完成，容器已保留：{0}'],
 ] as const) if(text.startsWith(prefix))return t(key,{0:apkError(text.slice(prefix.length))});
 const update=text.match(/^(Installation|Update) failed; existing container and data retained \(backup: (.*?)\): ([\s\S]*)$/);
 if(update)return t(update[1]==='Update'?'更新失败，原容器和数据已保留。备份：{0}。详情：{1}':'安装失败，已有容器和数据已保留。备份：{0}。详情：{1}',{0:update[2],1:apkError(update[3])});
 const abi=text.match(/APK architectures \((.*?)\) are not supported by this Android runtime \((.*?)\)/);
 if(abi)return t('APK 架构（{0}）与当前 Android 支持的架构（{1}）不兼容。请使用匹配架构的 APK；32 位 ARM 应用需要额外的兼容运行环境。',{0:abi[1],1:abi[2]});
 return t(text);
}
