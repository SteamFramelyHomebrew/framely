import {build} from 'esbuild';
import {mkdir,copyFile} from 'node:fs/promises';
export async function buildCodeEditor(out='ui/dist/assets'){
 await mkdir(out,{recursive:true});
 await build({entryPoints:['ui/src/file-code-engine.ts'],bundle:true,minify:true,outfile:out+'/code-editor.js',globalName:'FramelyCodeEditor',format:'iife',target:'chrome140',legalComments:'eof'});
 // Preserve third-party licenses in the installed assets as well as source.
 const packages=['@codemirror/view','@codemirror/state','@codemirror/commands','@codemirror/language','@codemirror/search','@codemirror/lang-javascript','@codemirror/lang-json','@codemirror/lang-css','@codemirror/lang-html','@codemirror/lang-python','@codemirror/lang-rust','@codemirror/autocomplete','@codemirror/lint','@lezer/common','@lezer/highlight','@lezer/lr','@lezer/javascript','@lezer/json','@lezer/css','@lezer/html','@lezer/python','@lezer/rust','style-mod','w3c-keyname','crelt'];
 await mkdir(out+'/code-editor-licenses',{recursive:true});
 for(const name of packages)await copyFile(`node_modules/${name}/LICENSE`,`${out}/code-editor-licenses/${name.replaceAll('/','-')}.txt`);
}
if(import.meta.url===new URL(process.argv[1],'file://'+process.cwd()+'/').href)await buildCodeEditor(process.argv[2]);
