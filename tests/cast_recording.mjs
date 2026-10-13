import {build} from 'esbuild';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {readFile} from 'node:fs/promises';
Object.defineProperty(globalThis,'navigator',{value:{languages:['zh-CN']},configurable:true});
globalThis.document={documentElement:{lang:''}};
const bundle=await build({stdin:{contents:`import React from 'react';import {renderToStaticMarkup} from 'react-dom/server';import {RecordingButton} from './ui/src/cast-recording';export const render=(recording,disabled=false)=>renderToStaticMarkup(<RecordingButton recording={recording} disabled={disabled} onClick={()=>{}}/>);`,resolveDir:process.cwd(),loader:'tsx'},bundle:true,platform:'node',format:'cjs',write:false});
const module={exports:{}};new Function('module','exports','require',bundle.outputFiles[0].text)(module,module.exports,createRequire(import.meta.url));
const idle=module.exports.render({active:false});assert.ok(idle.includes('开始录制'));assert.ok(!idle.includes('停止录制'));
const active=module.exports.render({active:true,elapsedSeconds:125,path:'/home/steamos/Videos/framely/test.mkv'});
assert.ok(active.includes('停止录制'));assert.ok(active.includes('02:05'));assert.ok(active.includes('test.mkv'));assert.ok(active.includes(' recording'));
assert.ok(module.exports.render({active:true},true).includes('disabled=""'));
for(const locale of ['en-US','zh-CN']){
 const {messages}=JSON.parse(await readFile(`ui/locales/${locale}.json`,'utf8'));
 for(const key of ['开始录制','停止录制','录制保存到 /home/steamos/Videos/framely'])assert.ok(messages[key]);
}
console.log('PASS: recording button idle/active/disabled states, timer, path and translations');
