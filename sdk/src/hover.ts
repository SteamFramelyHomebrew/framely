// Shared by the host page and isolated plugin frames.
export function installHoverFeedback(pulse:()=>void){
 let hovered:Element|null=null;
 function interactive(target:EventTarget|null):Element|null{
  const element=target instanceof Element?target:null;
  if(!element||element.closest('[inert],:disabled,[aria-disabled="true"]'))return null;
  const control=element.closest('button,a[href],input:not([type="hidden"]),select,textarea,summary,[role="button"],[role="link"],[role="checkbox"],[role="switch"],[role="slider"],[role="tab"],[role="menuitem"],[contenteditable="true"],[data-framely-interactive],[tabindex]:not([tabindex="-1"])');
  if(control)return control;
  // Cursor styles are inherited: return the control, not each of its children.
  let pointer:Element|null=null;
  for(let e:Element|null=element;e;e=e.parentElement){
   if(getComputedStyle(e).cursor==='pointer')pointer=e;
   else if(pointer)break;
  }
  return pointer;
 }
 function enter(e:Event){const next=interactive(e.target);if(next!==hovered){hovered=next;if(next)pulse();}}
 document.addEventListener('pointerover',enter);
 document.addEventListener('pointerout',e=>{if(interactive(e.relatedTarget)!==hovered)hovered=null;});
 window.addEventListener('blur',()=>{hovered=null;});
}
