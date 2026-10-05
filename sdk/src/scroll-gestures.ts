// Installed by the host bootstrap as well as the manager; old plugins benefit
// without rebuilding their own SDK. The launcher owns its separate gestures.
export function installScrollGestures(){
 if((document as any).__framelyScrollGestures)return;
 (document as any).__framelyScrollGestures=true;
 type Drag={id:number;x:number;y:number;last:number;nodes:HTMLElement[];active:boolean;styles:string[]};
 let drag:Drag|null=null,suppressClick=false,timer:ReturnType<typeof setTimeout>|null=null;
 const clear=()=>{if(drag?.active){drag.nodes.forEach((n,i)=>n.style.scrollBehavior=drag!.styles[i]);try{drag.nodes[0].releasePointerCapture(drag.id);}catch{}}drag=null;};
 const cancel=()=>{clear();suppressClick=false;if(timer)clearTimeout(timer);timer=null;};
 const scroll=(dy:number)=>{if(!drag)return;for(const node of drag.nodes){const before=node.scrollTop;node.scrollTop+=dy;dy-=node.scrollTop-before;if(Math.abs(dy)<.5)break;}};
 document.addEventListener('pointerdown',e=>{
  cancel();if(e.button!==0||document.documentElement.classList.contains('launcher-view'))return;
  const target=e.target instanceof Element?e.target:null;
  if(!target||target.closest('input,textarea,select,[contenteditable]:not([contenteditable="false"]),[role=slider],[draggable=true],[data-framely-no-scroll-drag]'))return;
  const nodes:HTMLElement[]=[];
  for(let n:Element|null=target;n;n=n.parentElement){if(n instanceof HTMLElement&&n.scrollHeight>n.clientHeight+1&&/auto|scroll/.test(getComputedStyle(n).overflowY))nodes.push(n);}
  const root=document.scrollingElement;if(root instanceof HTMLElement&&root.scrollHeight>root.clientHeight+1&&!nodes.includes(root))nodes.push(root);
  if(nodes.length)drag={id:e.pointerId,x:e.clientX,y:e.clientY,last:e.clientY,nodes,active:false,styles:nodes.map(n=>n.style.scrollBehavior)};
 },true);
 document.addEventListener('pointermove',e=>{
  if(!drag||e.pointerId!==drag.id)return;
  if(!e.buttons){clear();return;}
  const dy=e.clientY-drag.y,dx=e.clientX-drag.x;
  if(!drag.active){if(Math.abs(dy)<20||Math.abs(dy)<Math.abs(dx)*1.25)return;drag.active=true;suppressClick=true;drag.nodes.forEach(n=>n.style.scrollBehavior='auto');try{drag.nodes[0].setPointerCapture(e.pointerId);}catch{}window.getSelection()?.removeAllRanges();}
  e.preventDefault();e.stopImmediatePropagation();scroll(drag.last-e.clientY);drag.last=e.clientY;
 },{capture:true,passive:false});
 document.addEventListener('pointerup',e=>{
  if(!drag||e.pointerId!==drag.id)return;
  if(drag.active){e.preventDefault();e.stopImmediatePropagation();scroll(drag.last-e.clientY);timer=setTimeout(()=>{suppressClick=false;timer=null;},200);}
  clear();
 },true);
 document.addEventListener('click',e=>{if(suppressClick){e.preventDefault();e.stopImmediatePropagation();suppressClick=false;if(timer)clearTimeout(timer);timer=null;}},true);
 document.addEventListener('pointercancel',cancel,true);
 document.addEventListener('lostpointercapture',()=>{if(drag?.active)clear();},true);
 window.addEventListener('blur',cancel);window.addEventListener('pagehide',cancel);
 document.addEventListener('visibilitychange',()=>{if(document.hidden)cancel();});
}
