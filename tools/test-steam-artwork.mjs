// Dedicated real-CEF test; does not start or modify the user's Steam instance.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawn,execFileSync} from 'node:child_process';
const cef=path.resolve(process.argv[2]??'target/cef');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'framely-steam-artwork-'));
const read=n=>fs.readFileSync('src/apk/'+n,'utf8');
const fixture=fs.readFileSync('tests/steam_artwork_browser.js','utf8');
fs.writeFileSync(dir+'/fixture.js',`const selectPalette=(${read('steam_artwork_palette.js')});const renderArtwork=(${read('steam_artwork_render.js')});\n${fixture}`);
fs.writeFileSync(dir+'/index.html','<!doctype html><meta charset="utf-8"><style>html,body{margin:0;background:transparent}#root{position:fixed;inset:0;background:#15161a;border-radius:28px;overflow:hidden;color:white;font:20px sans-serif;padding:40px;box-sizing:border-box}</style><div id="root">Steam artwork renderer test</div><script src="fixture.js"></script>');
execFileSync('node',['tools/build-scroll-gestures.mjs',dir]);
const binary=dir+'/framely-browser-probe';
execFileSync('g++',['-std=c++17','-O2','-I'+dir,'-I'+cef,'tests/browser_probe.cpp','-L'+cef+'/Release','-lcef','-pthread','-Wl,-rpath,'+cef+'/Release','-o',binary]);
fs.cpSync(cef+'/Resources',cef+'/Release',{recursive:true});
const py=`import http.server,functools\ns=http.server.ThreadingHTTPServer(('127.0.0.1',0),functools.partial(http.server.SimpleHTTPRequestHandler,directory=${JSON.stringify(dir)}))\nprint(s.server_port,flush=True)\ns.serve_forever()`;
const server=spawn('python3',['-u','-c',py],{stdio:['ignore','pipe','ignore']});
try{
 const port=await new Promise((resolve,reject)=>{server.stdout.once('data',d=>resolve(Number(d.toString().trim())));server.once('error',reject);server.once('exit',c=>reject(Error('Preview server exited '+c)));});
 const env={...process.env,FRAMELY_CEF_ROOT:cef,FRAMELY_INSTALL_REVIEW_TEST:'1',FRAMELY_STORE_TEST:'1',FRAMELY_PREVIEW_DIR:dir,GSETTINGS_BACKEND:'memory'};
 const probe=spawn(binary,[`http://127.0.0.1:${port}`,dir+'/runtime','--ozone-platform=headless','--disable-gpu'],{env,stdio:['ignore','pipe','pipe']});
 let log='';for(const stream of [probe.stdout,probe.stderr])stream.on('data',d=>{log+=d;});
 const code=await new Promise((resolve,reject)=>{probe.once('exit',resolve);probe.once('error',reject);});
 fs.writeFileSync(dir+'/test.log',log);
 console.log(log.split('\n').filter(l=>l.includes('FRAMELY_')||l.includes('BROWSER_RESULT')).join('\n'));
 if(code!==0)throw Error('CEF artwork test failed: '+dir+'/test.log');
 console.log('CEF artwork test artifacts: '+dir);
}finally{server.kill();}
