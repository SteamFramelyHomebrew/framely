import {build} from 'esbuild';
import {mkdir,copyFile,cp} from 'node:fs/promises';
await mkdir('ui/dist/assets',{recursive:true});
await build({entryPoints:['ui/src/main.tsx'],bundle:true,minify:true,outfile:'ui/dist/assets/app.js',define:{'process.env.NODE_ENV':'"production"'}});
await build({entryPoints:['sdk/src/bootstrap.ts'],bundle:true,minify:true,outfile:'ui/dist/assets/plugin-bootstrap.js'});
await copyFile('ui/index.html','ui/dist/index.html');
await cp('ui/locales','ui/dist/assets/locales',{recursive:true});
await mkdir('examples/showcase/payload',{recursive:true});
await build({entryPoints:['examples/showcase/page.tsx'],bundle:true,minify:true,outfile:'examples/showcase/payload/page.js',define:{'process.env.NODE_ENV':'"production"'}});
await copyFile('examples/showcase/backend.py','examples/showcase/payload/backend.py');

await copyFile('sdk/python/framely.py','examples/showcase/payload/framely.py');
