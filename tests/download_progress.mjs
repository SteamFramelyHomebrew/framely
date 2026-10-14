import {build} from 'esbuild';
import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
Object.defineProperty(globalThis,'navigator',{value:{languages:['zh-CN']},configurable:true});
globalThis.document={documentElement:{lang:''},addEventListener(){}};
globalThis.window={addEventListener(){}};
const bundle=await build({stdin:{contents:`import React from 'react';import {renderToStaticMarkup} from 'react-dom/server';import {Transfer} from './ui/src/store';export const render=(job)=>renderToStaticMarkup(<Transfer job={job} close={()=>{}}/>);`,resolveDir:process.cwd(),loader:'tsx'},bundle:true,platform:'node',format:'cjs',write:false});
const module={exports:{}};
new Function('module','exports','require',bundle.outputFiles[0].text)(module,module.exports,createRequire(import.meta.url));
const render=(received,total,phase='downloading')=>module.exports.render({id:'test',kind:'inspect',phase,received,total});
for(const received of [995,999,1000,1001]){
 const html=render(received,1000);
 assert.ok(html.includes('99%'));
 assert.ok(!html.includes('100%'));
 assert.ok(html.includes('value="99"'));
}
assert.ok(render(500,1000).includes('50%'));
assert.ok(!render(500,undefined).includes('NaN'));
assert.ok(!render(1000,1000,'verifying').includes('<progress'));
console.log('PASS: incomplete downloads never display 100%, including rounded and final-byte progress');
