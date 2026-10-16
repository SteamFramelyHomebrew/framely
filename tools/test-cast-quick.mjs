// Focused CEF check of the real quick streaming tab with a local mock API.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawn,execFileSync} from 'node:child_process';
import {build} from 'esbuild';
import {crc32,deflateSync} from 'node:zlib';
// Export the native probe's lossless screenshots without an image dependency.
function png(file){
 const ppm=fs.readFileSync(file);let offset=0;
 const line=()=>{const end=ppm.indexOf(10,offset),value=ppm.toString('ascii',offset,end);offset=end+1;return value;};
 if(line()!=='P6')throw Error('Invalid PPM');const [w,h]=line().split(' ').map(Number);if(line()!=='255')throw Error('Invalid PPM depth');
 const pixels=ppm.subarray(offset),stride=w*3;if(pixels.length!==stride*h)throw Error('Invalid PPM size');
 const raw=Buffer.alloc((stride+1)*h);for(let y=0;y<h;y++)pixels.copy(raw,y*(stride+1)+1,y*stride,(y+1)*stride);
 const chunk=(type,data)=>{const name=Buffer.from(type),size=Buffer.alloc(4),sum=Buffer.alloc(4);size.writeUInt32BE(data.length);sum.writeUInt32BE(crc32(Buffer.concat([name,data])));return Buffer.concat([size,name,data,sum]);};
 const header=Buffer.alloc(13);header.writeUInt32BE(w);header.writeUInt32BE(h,4);header[8]=8;header[9]=2;
 fs.writeFileSync(file.replace(/\.ppm$/,'.png'),Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',deflateSync(raw)),chunk('IEND',Buffer.alloc(0))]));
}
const cef=path.resolve(process.argv[2]??'target/cef');
const framing=process.argv[3]==='framing';
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'framely-cast-quick-'));
await build({entryPoints:[framing?'tests/cast_framing_drag_fixture.tsx':'tests/cast_quick_fixture.tsx'],bundle:true,outfile:dir+'/fixture.js',define:{'process.env.NODE_ENV':'"production"'}});
fs.writeFileSync(dir+'/index.html','<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="fixture.css"><div id="root"></div><script src="fixture.js"></script>');
fs.cpSync('ui/locales',dir+'/assets/locales',{recursive:true});
fs.cpSync('assets/branding',dir+'/assets/branding',{recursive:true});
execFileSync('node',['tools/build-scroll-gestures.mjs',dir]);
const binary=dir+'/framely-browser-probe';
execFileSync('g++',['-std=c++17','-O2','-I'+dir,'-I'+cef,'tests/browser_probe.cpp','-L'+cef+'/Release','-lcef','-pthread','-Wl,-rpath,'+cef+'/Release','-o',binary]);
const server=spawn('python3',['-u','-c',`import http.server,functools\ns=http.server.ThreadingHTTPServer(('127.0.0.1',0),functools.partial(http.server.SimpleHTTPRequestHandler,directory=${JSON.stringify(dir)}))\nprint(s.server_port,flush=True)\ns.serve_forever()`],{stdio:['ignore','pipe','ignore']});
try{
 const port=await new Promise((resolve,reject)=>{server.stdout.once('data',d=>resolve(Number(d.toString().trim())));server.once('error',reject);});
 for(const width of framing?[600]:[600,390]){
  const output=dir+'/'+width;fs.mkdirSync(output);
  const env={...process.env,FRAMELY_CEF_ROOT:cef,FRAMELY_INSTALL_REVIEW_TEST:'1',FRAMELY_TEST_VIEW_WIDTH:String(width),FRAMELY_PREVIEW_DIR:output,GSETTINGS_BACKEND:'memory'};
  const probe=spawn(binary,[`http://127.0.0.1:${port}`,output+'/runtime','--ozone-platform=headless','--disable-gpu'],{env,stdio:['ignore','pipe','pipe']});
  let log='';for(const stream of [probe.stdout,probe.stderr])stream.on('data',d=>{log+=d;});
  const code=await new Promise((resolve,reject)=>{probe.once('exit',resolve);probe.once('error',reject);});
  fs.writeFileSync(output+'/test.log',log);
  console.log(width,log.split('\n').filter(l=>l.includes('FRAMELY_')||l.includes('BROWSER_RESULT')).join('\n'));
  if(code!==0)throw Error('Quick casting test failed: '+output+'/test.log');
  for(const name of fs.readdirSync(output).filter(n=>n.endsWith('.ppm')))png(output+'/'+name);
 }
 console.log('Quick casting test artifacts: '+dir);
}finally{server.kill();}
