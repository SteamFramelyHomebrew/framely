// Render the real quick panel with simulated API responses. No device changes.
const wait=(ms=80)=>new Promise(r=>setTimeout(r,ms));
const until=async(f:()=>unknown)=>{for(let i=0;i<150;i++){if(f())return;await wait(30);}throw Error('Timed out: '+document.body.innerText);};
history.replaceState(null,'','/?fixture=1#casting');
const casting={source:'screen',codec:'h265',output:'eye',eye:'right',width:2560,height:1920,fps:90,bitrateMbps:12,horizontalFov:null,centerX:.5,centerY:.5,systemAudio:true,microphone:false,airplay:false,dlna:false,receiverName:'Framely',recordingIndicatorVisible:true,recordingRegionVisible:false};
let streaming=false,recording=false,failSave=false,framing=0,opens=0;
const calls:string[]=[];
window.fetch=async(_url,init)=>{
 const {method,params}=JSON.parse(String(init?.body));calls.push(method);let result:any=true;
 if(method==='status')result={version:'test',agreement:{version:1,accepted:true},database:{casting,language:new URLSearchParams(location.search).get('lang')??'zh-CN',plugins:{},sources:[],safeMode:false},running:[],notifications:[],inbox:[]};
 else if(method==='language.list')result={packs:[],invalidFiles:[]};
 else if(method==='ui.events')result={cursor:0,events:[]};
 else if(method==='cast.status')result={running:streaming,recording:{active:recording,elapsedSeconds:recording?83:0},watchAddresses:['http://192.168.5.67:15915/cast/watch']};
 else if(method==='cast.start')streaming=true;
 else if(method==='cast.stop')streaming=false;
 else if(method==='cast.record.start')recording=true;
 else if(method==='cast.record.stop')recording=false;
 else if(method==='cast.recordingIndicator.visible.save'){if(failSave)return new Response(JSON.stringify({error:'Fixture save failed'}));casting.recordingIndicatorVisible=params.enabled;}
 else if(method==='cast.region.visible.save')casting.recordingRegionVisible=params.enabled;
 else if(method==='cast.settings.save'){if(params.width!==2560||params.codec!=='h265')throw Error('Receiver reset capture config');Object.assign(casting,params);}
 else if(method==='cast.framing.open')framing++;
 else if(method==='host.manager.open')opens++;
 return new Response(JSON.stringify({result}));
};
const button=(selector:string)=>document.querySelector<HTMLButtonElement>(selector)!;
const click=async(selector:string)=>{await until(()=>!button(selector).disabled);button(selector).click();await wait(150);};
const checkLayout=()=>{
 if(document.documentElement.scrollWidth>innerWidth)throw Error('Horizontal overflow');
 for(const e of document.querySelectorAll<HTMLElement>('.cast-quick button')){
  const r=e.getBoundingClientRect();if(r.width<44||r.height<44||r.left<0||r.right>innerWidth)throw Error('Invalid hit target: '+e.textContent);
 }
 for(const e of document.querySelectorAll<HTMLElement>('.cast-quick .ui-switch'))if(e.getBoundingClientRect().width!==60)throw Error('Switch width regressed');
 if(innerWidth>=500){const a=button('.cast-stream-actions>button'),b=button('.cast-record-button');if(a.getBoundingClientRect().top!==b.getBoundingClientRect().top)throw Error('Primary actions not aligned');}
};
(window as any).runInstallReviewChecks=async()=>{try{
 await until(()=>!!document.querySelector('nav button'));[...document.querySelectorAll<HTMLButtonElement>('nav button')].find(b=>b.textContent==='串流')!.click();
 await until(()=>document.querySelector('.cast-quick')&&!button('.cast-stream-actions>button').disabled);await wait(180);checkLayout();
 console.log('FRAMELY_PREVIEW_INSTALL_CAST_IDLE');
 await click('.cast-record-button');await until(()=>recording&&!button('.cast-record-button').disabled);
 if(streaming||calls.includes('cast.start'))throw Error('Recording started streaming');
 if(!button('.cast-quick-framing-row>button').disabled)throw Error('Framing allowed during recording');
 failSave=true;await click('[aria-label="显示录制指示器"]');await until(()=>document.body.textContent?.includes('Fixture save failed'));
 if(button('[aria-label="显示录制指示器"]').getAttribute('aria-checked')!=='true')throw Error('Failed save changed switch');
 failSave=false;await click('[aria-label="显示录制指示器"]');await until(()=>!casting.recordingIndicatorVisible&&!button('.cast-record-button').disabled);
 if(!recording)throw Error('Hiding indicator stopped recording');
 await click('.cast-record-button');await until(()=>!recording&&!button('.cast-stream-actions>button').disabled);
 await click('.cast-quick-framing-row>button');await until(()=>framing===1&&!button('.cast-stream-actions>button').disabled);
 await click('[aria-label="录制区域显示"]');await until(()=>casting.recordingRegionVisible&&!button('.cast-stream-actions>button').disabled);
 await click('[aria-label="AirPlay"]');await until(()=>casting.airplay&&!button('.cast-stream-actions>button').disabled);
 await click('[aria-label="DLNA"]');await until(()=>casting.dlna&&!button('.cast-stream-actions>button').disabled);
 await click('.cast-stream-actions>button');await until(()=>streaming&&!button('.cast-record-button').disabled);
 await click('.cast-record-button');await until(()=>recording&&!button('.cast-record-button').disabled);await wait(200);checkLayout();
 console.log('FRAMELY_PREVIEW_INSTALL_CAST_ACTIVE');await wait(100);
 await click('.cast-stream-actions>button');await until(()=>!streaming&&!button('.cast-record-button').disabled);
 await click('.cast-record-button');await until(()=>!recording&&!button('.cast-quick>header button').disabled);
 await click('.cast-quick>header button');await until(()=>opens===1);
 console.log('FRAMELY_BRIDGE_PASS');
 }catch(e){console.error('FRAMELY_BRIDGE_FAIL '+e);}
};
void import('../ui/src/main');
