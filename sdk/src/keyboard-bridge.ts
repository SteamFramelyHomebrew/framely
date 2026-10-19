// Host SDK adapter. Reuses installKeyboard's editor; no layout/window changes.
export type KeyboardBridgeCall=(method:string,params:Record<string,unknown>)=>Promise<any>;
export function createKeyboardBridge(call:KeyboardBridgeCall){
 let field:HTMLElement|null=null,context='',active=false,stopped=false;
 const client=`${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
 let serial=0,timer:ReturnType<typeof setTimeout>|undefined;
 const valid=()=>!!field?.isConnected&&document.activeElement===field&&!field.closest('[inert]')&&!(field as HTMLInputElement).disabled&&!(field as HTMLInputElement).readOnly;
 const close=()=>{const old=context;active=false;context='';if(timer)clearTimeout(timer);if(old)void call('webInputClose',{context:old}).catch(()=>{});};
 const key=(op:any)=>{
  if(op.kind==='text'){(window as any).__framelyEditKeyboard(op.text);return true;}
  const input=field as HTMLInputElement|HTMLTextAreaElement;
  const map:Record<string,string>={BackSpace:'\b',Left:'\x1b[D',Right:'\x1b[C',Up:'\x1b[A',Down:'\x1b[B'};
  if(!op.ctrl&&!op.alt&&map[op.key]){(window as any).__framelyEditKeyboard(map[op.key]);return true;}
  if(op.ctrl&&op.key.toLowerCase()==='a'&&typeof input.select==='function'){input.select();return true;}
  if(op.ctrl||op.alt)return false;
  if(['Home','End','Delete'].includes(op.key)&&typeof input.value==='string'){
   const a=input.selectionStart??input.value.length,b=input.selectionEnd??a;
   if(op.key==='Delete'){
    if(a===b){const next=Array.from(new (Intl as any).Segmenter(undefined,{granularity:'grapheme'}).segment(input.value.slice(a)))[0] as any;input.setSelectionRange(a,a+(next?.segment.length??0));}
    if((input.selectionStart??0)!==(input.selectionEnd??0))(window as any).__framelyEditKeyboard('\b');
   }else{const p=op.key==='Home'?0:input.value.length;input.setSelectionRange(op.shift?a:p,p);}
   return true;
  }
  const event={key:op.key==='Return'?'Enter':op.key==='Escape'?'Escape':op.key,code:op.key==='Return'?'Enter':op.key,bubbles:true,cancelable:true,shiftKey:!!op.shift};
  const allowed=field!.dispatchEvent(new KeyboardEvent('keydown',event));
  if(op.key==='Return'&&allowed){
   if(field instanceof HTMLTextAreaElement)(window as any).__framelyEditKeyboard('\n');
   else if(field instanceof HTMLInputElement&&field.form)field.form.requestSubmit();
  }
  if(op.key==='Tab'&&allowed){const fields=Array.from(document.querySelectorAll<HTMLElement>('input:not([disabled]),textarea:not([disabled]),button:not([disabled]),select:not([disabled]),[tabindex="0"]')).filter(e=>e.getClientRects().length>0&&!e.closest('[inert]'));const index=fields.indexOf(field!);fields[(index+(op.shift?-1:1)+fields.length)%fields.length]?.focus();}
  field?.dispatchEvent(new KeyboardEvent('keyup',event));return true;
 };
 const poll=async()=>{
  const token=context;if(!active||!token||stopped)return;
  if(!valid()){close();return;}
  try{
   const response=await call('webInputPoll',{context:token});
   if(context!==token||!active)return;
   if(!response.active){close();return;}
   for(const edit of response.edits??[]){
    let delivered=false;
    if(edit.context===token&&context===token&&valid()&&Date.now()<edit.expiresAt){
     const claim=await call('webInputClaim',{context:token,id:edit.id,generation:edit.generation});
     if(claim.accepted&&context===token&&valid()&&Date.now()<edit.expiresAt)delivered=key(edit.operation);
    }
    await call('webInputAck',{context:token,id:edit.id,generation:edit.generation,delivered});
   }
  }catch{close();return;}
  if(context===token&&active)timer=setTimeout(poll,20);
 };
 const lost=(event:FocusEvent)=>{if(event.target===field)close();};
 const dispose=()=>{stopped=true;close();field=null;document.removeEventListener('focusout',lost,true);window.removeEventListener('pagehide',dispose);window.removeEventListener('blur',close);};
 document.addEventListener('focusout',lost,true);window.addEventListener('pagehide',dispose);window.addEventListener('blur',close);
 const start=async(target:HTMLElement,password:boolean):Promise<boolean>=>{
  if(stopped||document.activeElement!==target)return false;
  if(active&&field===target&&valid())return true;
  close();field=target;context=`${client}-${++serial}`;
  const token=context;
  try{
   const response=await call('webInputOpen',{context:token,client,sequence:serial,expiresAt:Date.now()+900,password});
   if(context!==token||!valid()){if(response.ok)void call('webInputClose',{context:token});return true;}
   active=!!response.ok;if(active){void poll();return true;}context='';return false;
  }catch{if(context!==token||stopped||!valid())return true;context='';return false;}

 };
 return Object.assign(start,{dispose});
}
