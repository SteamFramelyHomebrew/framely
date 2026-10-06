export type GazeSample={valid:boolean;x?:number;y?:number;phase?:'down'|'up'|'cancel';targetId?:string;rawAngles?:[number,number];targetAngles?:[number,number];headPose?:number[];targetUnavailable?:boolean};
// DOM targets stay fixed during a press; gaze movement must never become drag.
export function installLauncherGaze(root:HTMLElement,blocked:()=>boolean,feedback:()=>void){
 let focus:HTMLElement|null=null,candidate:HTMLElement|null=null,since=0,lastFeedback=-Infinity,pressed:HTMLElement|null=null,armed=false,lastValid=-Infinity,filtered:{x:number;y:number;at:number}|null=null;
 const selector='button:not(:disabled),input:not(:disabled)';
 const usable=(node:HTMLElement|null):node is HTMLElement=>!!node&&node.isConnected&&!node.matches(':disabled')&&root.contains(node);
 function highlight(next:HTMLElement|null){if(focus===next)return;focus?.classList.remove('gaze-focused');focus?.dispatchEvent(new PointerEvent('pointerout',{bubbles:true,pointerId:901,pointerType:'mouse',relatedTarget:next}));const previous=focus;focus=next;focus?.classList.add('gaze-focused');focus?.dispatchEvent(new PointerEvent('pointerover',{bubbles:true,pointerId:901,pointerType:'mouse',relatedTarget:previous}));if(next&&performance.now()-lastFeedback>=180){lastFeedback=performance.now();feedback();}}
 function cancel(){if(pressed){pressed.dispatchEvent(new PointerEvent('pointercancel',{bubbles:true,pointerId:901,pointerType:'mouse'}));pressed=null;}armed=false;candidate=null;filtered=null;highlight(null);}
 const invalidTimer=window.setInterval(()=>{if(performance.now()-lastValid>280)cancel();},50);
 function event(e:Event){const sample=(e as CustomEvent<GazeSample>).detail;const d=sample?{...sample}:null;if(!d||d.phase==='cancel'||blocked()){cancel();return;}
  // Brief sensor dropouts keep hover, but never complete a held click.
  if(!d.valid){if(pressed)cancel();return;}
  const now=performance.now();lastValid=now;
  if(d.phase==='down'){
   if(!armed||!usable(focus)||pressed)return;
   pressed=focus;const b=pressed.getBoundingClientRect();pressed.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,button:0,buttons:1,pointerId:901,pointerType:'mouse',clientX:b.x+b.width/2,clientY:b.y+b.height/2}));return;
  }
  if(d.phase==='up'){
   const node=pressed;pressed=null;armed=false;
   if(usable(node)){const b=node.getBoundingClientRect();node.dispatchEvent(new PointerEvent('pointerup',{bubbles:true,button:0,buttons:0,pointerId:901,pointerType:'mouse',clientX:b.x+b.width/2,clientY:b.y+b.height/2}));if(usable(node)){if(node instanceof HTMLInputElement)node.focus();node.click();}}return;
  }
  if(pressed){if(!usable(pressed))cancel();return;}
  if(!Number.isFinite(d.x)||!Number.isFinite(d.y)){cancel();return;}
  // Smooth normal fixation jitter; saccades jump immediately to avoid lag.
  if(!filtered||now-filtered.at>150||Math.hypot(d.x!-filtered.x,d.y!-filtered.y)>150)filtered={x:d.x!,y:d.y!,at:now};
  else{const alpha=1-Math.exp(-(now-filtered.at)/55);filtered={x:filtered.x+alpha*(d.x!-filtered.x),y:filtered.y+alpha*(d.y!-filtered.y),at:now};}
  const {x,y}=filtered;
  let next=document.elementFromPoint(x,y)?.closest<HTMLElement>(selector)??null;
  // A modal/menu takes priority over icons behind it.
  const modal=root.querySelector<HTMLElement>('.modal'),menu=root.querySelector<HTMLElement>('.launch-menu');
  if(next&&(!usable(next)||(modal&&!modal.contains(next))))next=null;
  if(!next){const nodes=Array.from((modal??menu??root).querySelectorAll<HTMLElement>(selector));let best=Infinity;for(const node of nodes){if(!usable(node)||node.closest('.launcher-adjacent-page'))continue;const b=node.getBoundingClientRect();if(b.width<=0||b.height<=0)continue;const padding=node.matches('.launch-icon')?32:16;if(x<b.left-padding||x>b.right+padding||y<b.top-padding||y>b.bottom+padding)continue;const distance=Math.hypot(x-(b.left+b.width/2),y-(b.top+b.height/2));if(distance<best){best=distance;next=node;}}}
  if(usable(focus)){const b=focus.getBoundingClientRect();if(x>=b.left-32&&x<=b.right+32&&y>=b.top-32&&y<=b.bottom+32)next=focus;}
  if(next!==candidate){candidate=next;since=now;}
  if(now-since>=120){highlight(candidate);armed=!!candidate;}
 }
 const reset=()=>cancel();
 window.addEventListener('framely.launcher.gaze',event);
 for(const name of ['framely.launcher.open','framely.launcher.cancelInput','framely.launcher.closing','framely.launcher.navigate','framely.keyboard'])window.addEventListener(name,reset);
 return()=>{clearInterval(invalidTimer);cancel();window.removeEventListener('framely.launcher.gaze',event);for(const name of ['framely.launcher.open','framely.launcher.cancelInput','framely.launcher.closing','framely.launcher.navigate','framely.keyboard'])window.removeEventListener(name,reset);};
}
