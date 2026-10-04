import {build} from 'esbuild';
import ts from 'typescript';
import {readFile,readdir} from 'node:fs/promises';
import assert from 'node:assert/strict';
const english=JSON.parse(await readFile('ui/locales/en-US.json','utf8'));
const chinese=JSON.parse(await readFile('ui/locales/zh-CN.json','utf8'));
assert.deepEqual(Object.keys(english.messages).sort(),Object.keys(chinese.messages).sort());
const placeholders=text=>[...new Set(text.match(/\{\d+\}/g)??[])].sort();
for(const [key,text]of Object.entries(english.messages))assert.deepEqual(placeholders(text),placeholders(key),key);
for(const file of await readdir('ui/src')){
 if(!/\.tsx?$/.test(file))continue;
 const source=ts.createSourceFile(file,await readFile(`ui/src/${file}`,'utf8'),ts.ScriptTarget.Latest,true,file.endsWith('tsx')?ts.ScriptKind.TSX:ts.ScriptKind.TS);
 function visit(node){if(ts.isCallExpression(node)&&node.expression.getText(source)==='t'&&ts.isStringLiteral(node.arguments[0]))assert.ok(Object.hasOwn(english.messages,node.arguments[0].text),`${file}: missing English translation: ${node.arguments[0].text}`);ts.forEachChild(node,visit);}visit(source);
}
const bundle=await build({entryPoints:['ui/src/i18n.ts'],bundle:true,platform:'node',format:'cjs',write:false});
Object.defineProperty(globalThis,'navigator',{value:{languages:['zh-CN']},configurable:true});globalThis.document={documentElement:{lang:''}};
const module={exports:{}};new Function('module','exports',bundle.outputFiles[0].text)(module,module.exports);
const {configureLanguage,resolveLanguage,t}=module.exports;
assert.equal(resolveLanguage('auto',[],['zh-TW']),'zh-CN');
assert.equal(resolveLanguage('auto',[],['de-DE']),'en-US');
assert.equal(resolveLanguage('en-US',[],['zh-CN']),'en-US');
const partial={schemaVersion:1,locale:'fr-FR',name:'Français',messages:{'语言':'Langue'}};
configureLanguage('fr-FR',[partial]);assert.equal(t('语言'),'Langue');assert.equal(t('保存'),'Save');assert.equal(t('启用 {0}',{0:'Test plugin'}),'Enable Test plugin');assert.equal(document.documentElement.lang,'fr-FR');
configureLanguage('zh-CN',[]);assert.equal(t('保存'),'保存');
configureLanguage('auto',[partial],['fr-CA']);assert.equal(t('语言'),'Langue');
console.log('Localization checks passed: English coverage, placeholders, locale detection, manual override and fallback.');
