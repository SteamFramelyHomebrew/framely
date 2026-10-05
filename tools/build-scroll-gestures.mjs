import {build} from 'esbuild';
import {mkdir,writeFile} from 'node:fs/promises';
import {resolve} from 'node:path';
const directory=resolve(process.argv[2]);
await mkdir(directory,{recursive:true});
const result=await build({entryPoints:['sdk/src/native-scroll.ts'],bundle:true,minify:true,write:false,format:'iife',target:'chrome120'});
const script=result.outputFiles[0].text;
await writeFile(resolve(directory,'native_scroll_script.h'),`#pragma once\nstatic constexpr char native_scroll_script[] = ${JSON.stringify(script)};\n`);
