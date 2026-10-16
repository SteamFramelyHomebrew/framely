window.runInstallReviewChecks=async()=>{
 try{
  await document.fonts.ready;
  const check=(ok,message)=>{if(!ok)throw Error(message);};
  const source=document.createElement('canvas');source.width=256;source.height=256;
  const c=source.getContext('2d');c.fillStyle='#ff6999';c.fillRect(0,0,256,256);
  c.fillStyle='white';c.fillRect(60,60,136,136);
  const p={image:source.toDataURL(),name:'哔哩哔哩HD',package:'test.example'};
  const sample=document.createElement('canvas');sample.width=sample.height=64;sample.getContext('2d').drawImage(source,0,0,64,64);
  const palette=selectPalette(sample.getContext('2d').getImageData(0,0,64,64).data,64,64);
  check(palette.selected==='#ff6999'&&palette.accent==='#cc547a','Wrong 80% palette');
  const a=await renderArtwork(p,selectPalette);
  const expected={0:[600,900],1:[3840,1240],2:[1280,360],3:[920,430],4:[256,256]};
  check(a.length===5,'Wrong image count');
  for(const v of a){check(v.canvas.width===expected[v.kind][0]&&v.canvas.height===expected[v.kind][1],'Wrong artwork dimensions');check(v.data.length>100,'Missing PNG');}
  const logo=a.find(v=>v.kind===2).canvas;
  check(logo.getContext('2d').getImageData(0,0,1,1).data[3]===0,'Logo lost alpha');
  const decoded=await new Promise((resolve,reject)=>{const image=new Image();image.onload=()=>resolve(image);image.onerror=reject;image.src='data:image/png;base64,'+a.find(v=>v.kind===2).data;});
  const roundtrip=document.createElement('canvas');roundtrip.width=logo.width;roundtrip.height=logo.height;roundtrip.getContext('2d').drawImage(decoded,0,0);
  const before=logo.getContext('2d').getImageData(0,0,logo.width,logo.height).data,after=roundtrip.getContext('2d').getImageData(0,0,logo.width,logo.height).data;
  check(before.every((v,i)=>v===after[i]),'PNG roundtrip changes ownership pixel hashes');
  const portrait=a.find(v=>v.kind===0).canvas,ctx=portrait.getContext('2d');
  const pixels=ctx.getImageData(0,475,600,100).data;let min=600,max=0,count=0;
  for(let i=0;i<pixels.length;i+=4)if(pixels[i]>245&&pixels[i+1]>245&&pixels[i+2]>245){const x=(i/4)%600;min=Math.min(min,x);max=Math.max(max,x);count++;}
  check(count>500,'Chinese title missing');check(Math.abs((min+max)/2-300)<8,'Portrait title is not centered');
  let accentPixels=0;const all=ctx.getImageData(0,0,600,900).data;
  for(let i=0;i<all.length;i+=4)if(all[i]===204&&all[i+1]===84&&all[i+2]===122)accentPixels++;
  check(accentPixels>1000,'Main ornaments do not preserve 80% source color');
  const b=await renderArtwork(p,selectPalette);check(a.every((v,i)=>v.data===b[i].data),'Rendering is not deterministic');
  const long=await renderArtwork({...p,name:'极长的应用名称 & <script>window.bad=true</script> '.repeat(3)},selectPalette);
  check(long.length===5&&!window.bad,'Unsafe title rendering');
  document.getElementById('root').textContent='Steam artwork renderer passed: 5 PNGs, CJK title, alpha, centering, palette and deterministic output';
  // Let the existing probe observe a completed frame before announcing success.
  await new Promise(r=>setTimeout(r,250));console.log('FRAMELY_BRIDGE_PASS');
 }catch(e){console.error('FRAMELY_BRIDGE_FAIL '+e);}
};
console.log('FRAMELY_VIEW_READY');
