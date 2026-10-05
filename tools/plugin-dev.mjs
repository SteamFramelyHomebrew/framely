#!/usr/bin/env node
import {mkdir,readFile,writeFile,cp} from 'node:fs/promises';
import {existsSync} from 'node:fs';import path from 'node:path';import {fileURLToPath} from 'node:url';import http from 'node:http';
const [command,arg,idArg]=process.argv.slice(2);
const toolRoot=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
if(command==='init'){
 if(!arg)throw Error('Usage: node tools/plugin-dev.mjs init <directory> [plugin.id]');
 const root=path.resolve(arg),id=idArg??'example.my-plugin';if(!/^[a-z0-9][a-z0-9._-]{0,79}$/.test(id)||id.includes('..'))throw Error('Invalid plugin id');
 if(existsSync(root))throw Error('Refusing to overwrite an existing directory');
 await cp(path.join(toolRoot,'templates/plugin'),root,{recursive:true,filter:p=>!['node_modules','payload','dist','.git','.framely-build'].includes(path.basename(p))});
 await mkdir(path.join(root,'vendor'),{recursive:true});await mkdir(path.join(root,'payload'));
 await cp(path.join(toolRoot,'sdk'),path.join(root,'vendor/framely-sdk'),{recursive:true,filter:p=>!p.includes('node_modules')});
 const manifest=JSON.parse(await readFile(path.join(root,'manifest.json'),'utf8'));manifest.id=id;
 await writeFile(path.join(root,'manifest.json'),JSON.stringify(manifest,null,2)+'\n');
 const pkg=JSON.parse(await readFile(path.join(root,'package.json'),'utf8'));pkg.name=id;
 pkg.scripts={build:'node dev.mjs build',dev:'node dev.mjs dev'};pkg.dependencies['@framely/sdk']='file:vendor/framely-sdk';
 await writeFile(path.join(root,'package.json'),JSON.stringify(pkg,null,2)+'\n');
 await cp(fileURLToPath(import.meta.url),path.join(root,'dev.mjs'));
 await cp(path.join(toolRoot,'LICENSE'),path.join(root,'LICENSE'));
 await cp(path.join(toolRoot,'LICENSE'),path.join(root,'vendor/framely-sdk/LICENSE'));
 console.log(`Created ${root}\ncd ${root}\nnpm install\nnpm run dev`);
}else if(command==='build'||command==='dev'){
 const {build,context}=await import('esbuild');const root=process.cwd();await mkdir(path.join(root,'payload'),{recursive:true});
 let revision=0;const clients=new Set();
 const options={entryPoints:{page:path.join(root,'page.tsx'),...(existsSync(path.join(root,'actions.ts'))?{actions:path.join(root,'actions.ts')}:{})},bundle:true,outdir:path.join(root,'payload'),minify:command==='build',sourcemap:command==='dev',define:{'process.env.NODE_ENV':JSON.stringify(command==='dev'?'development':'production')},plugins:[{name:'framely-preview',setup(builder){builder.onEnd(async result=>{if(!result.errors.length){await cp(path.join(root,'backend.py'),path.join(root,'payload/backend.py'));await cp(fileURLToPath(import.meta.resolve('@framely/sdk/python')),path.join(root,'payload/framely.py'));if(revision++)for(const client of clients)client.write('data: reload\n\n');}})}}]};
 if(command==='build'){await build(options);console.log('Built payload. Package using framely pack.');}
 else{
  const ctx=await context(options);await ctx.watch();
  const server=http.createServer(async(req,res)=>{try{
   if(req.url==='/__framely/events'){res.writeHead(200,{'Content-Type':'text/event-stream','Cache-Control':'no-cache'});res.write('data: connected\n\n');clients.add(res);req.on('close',()=>clients.delete(res));return;}
   if(req.url==='/'||req.url==='/quick'||req.url?.startsWith('/window/')||req.url?.startsWith('/?')){res.setHeader('Content-Type','text/html');res.end(`<!doctype html><meta charset="utf-8"><div id="root"></div><script>window.__framelyBridge={request:async(op,p)=>{if(op==='window.open'){window.open('/?window='+encodeURIComponent(p.window),'_blank');return true;}if(op==='window.close'){window.close();return true;}if(op==='notification.send'){alert(p.notification.title+'\\n'+p.notification.body);return true;}if(op==='ui.launchContext')return {source:'quickPanel',trigger:'shortPress'};if(op==='dependencies')return [];if(op==='notification.remove'||op==='notification.dismiss'||op==='haptic'||op==='keyboard')return true;throw Error('浏览器预览不启动后端，请安装到 Frame 测试此调用。');},subscribe:()=>()=>{}};new EventSource('/__framely/events').onmessage=e=>{if(e.data==='reload')location.reload()};history.replaceState(null,'',location.pathname.startsWith('/window/')?location.pathname:location.search.includes('window=')?'/window/'+new URLSearchParams(location.search).get('window'):'/quick');</script><script src="/page.js"></script>`);return;}
   const name=req.url?.split('?')[0];if(name!=='/page.js'&&name!=='/page.js.map'){res.writeHead(404);res.end();return;}res.setHeader('Content-Type',name.endsWith('.map')?'application/json':'application/javascript');res.end(await readFile(path.join(root,'payload',name.slice(1))));
  }catch(e){res.writeHead(500);res.end(String(e));}});
  server.listen(Number(process.env.FRAMELY_DEV_PORT??5173),'127.0.0.1',()=>console.log(`Preview: http://127.0.0.1:${server.address().port} (no device privileges)`));
 }
}else{console.log('Usage: node tools/plugin-dev.mjs init <directory> [plugin.id]\nGenerated project: npm run build | npm run dev');}
