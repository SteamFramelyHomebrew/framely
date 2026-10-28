import {build} from 'esbuild';
import assert from 'node:assert/strict';

Object.defineProperty(globalThis,'navigator',{value:{languages:['zh-CN']},configurable:true});
globalThis.document={documentElement:{lang:''}};
const bundle=await build({entryPoints:['ui/src/apk-errors.ts'],bundle:true,platform:'node',format:'cjs',write:false});
const module={exports:{}};
new Function('module','exports',bundle.outputFiles[0].text)(module,module.exports);
const {apkError}=module.exports;
assert.equal(apkError(new Error('New installation failed; newly created container was cleaned up: /test/baked: missing')),'安装失败，本次新建的容器已清理：/test/baked: missing');
assert.equal(apkError(new Error('Installation failed; container retained because safe cleanup failed: install; cleanup: ownership')),'安装失败，安全清理未完成，容器已保留：install; cleanup: ownership');
assert.equal(apkError(new Error('Installation failed; existing container and data retained (backup: backup-1): missing\npath')),'安装失败，已有容器和数据已保留。备份：backup-1。详情：missing\npath');
assert.match(apkError('APK state is unknown; reconcile before changing the application'),/APK 状态未知/);
assert.equal(apkError(new Error('unmapped diagnostic /path')),'unmapped diagnostic /path');

assert.equal(apkError('Update failed; existing container and data retained (backup: old): rejected'),'更新失败，原容器和数据已保留。备份：old。详情：rejected');
assert.match(apkError('APK architectures (armeabi-v7a) are not supported by this Android runtime (arm64-v8a).'),/32 位 ARM/);
assert.match(apkError('Installation failed; existing container and data retained (backup: old): APK architectures (armeabi-v7a) are not supported by this Android runtime (arm64-v8a).'),/安装失败.*32 位 ARM/);
console.log('APK failure outcomes are localized and diagnostic details are preserved.');
