// Exercise the real quick panel across status polling and repeated host launches.
const wait=(ms=80)=>new Promise(r=>setTimeout(r,ms));
const until=async(f:()=>any)=>{for(let i=0;i<150;i++){if(f())return;await wait(30);}throw Error('Timed out '+document.body.innerText.slice(0,350));};
history.replaceState(null,'','/?fixture=1#plugin:test.quick');
let polls=0,opens=0;
window.fetch=async(_url,init)=>{
 const {method}=JSON.parse(String(init?.body));let result:any=true;
 if(method==='status'){polls++;result={version:'0.4.3-preview.2',agreement:{version:1,accepted:true},database:{language:'en-US',plugins:{'test.quick':{manifest:{id:'test.quick',name:'Quick fixture',version:'1',author:'Test',description:'',ui:{quickPage:'page.js',windows:{}}},enabled:true,favorite:true,order:0}},sources:[],safeMode:false},running:[],notifications:[],inbox:[]};}
 else if(method==='language.list')result={packs:[],invalidFiles:[]};
 else if(method==='ui.events')result={cursor:0,events:[]};
 else if(method==='plugin.open')opens++;
 return new Response(JSON.stringify({result}));
};
const back=()=>document.querySelector<HTMLButtonElement>('.page-title button')!;
(window as any).runInstallReviewChecks=async()=>{try{
 await until(()=>!!back()&&!!document.querySelector('iframe'));
 if(location.hash||location.search!=='?fixture=1')throw Error('Plugin hash was not consumed correctly');
 back().click();await until(()=>!!document.querySelector('.workspace'));
 const before=polls;await until(()=>polls>=before+2);
 if(back()||document.querySelector('iframe')||opens!==1)throw Error('Status polling reopened plugin');
 location.hash='#plugin:test.quick';await until(()=>!!back()&&opens===2);
 window.dispatchEvent(new Event('framely.back'));await until(()=>!!document.querySelector('.workspace'));
 const second=polls;await until(()=>polls>=second+2);
 if(back()||location.hash||opens!==2)throw Error('Back event reopened plugin');
 location.hash='#plugin:test.quick';await until(()=>!!back()&&opens===3);
 window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape'}));await until(()=>!!document.querySelector('.workspace'));
 const third=polls;await until(()=>polls>=third+2);
 if(back()||location.hash||opens!==3)throw Error('Escape reopened plugin');
 console.log('FRAMELY_BRIDGE_PASS');
 }catch(e){console.error('FRAMELY_BRIDGE_FAIL '+e);}
};
void import('../ui/src/main');
