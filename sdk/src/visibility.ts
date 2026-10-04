/** Visibility in the headset. Unknown state must be treated as obscured by capture plugins. */
export interface CaptureVisibility { known:boolean; captureObscured:boolean; sequence:number; sessionId?:string; views?:Record<string,boolean> }
export interface UiVisibility extends CaptureVisibility { pageVisible:boolean }
export interface VisibilityBridge { request:(op:string,p?:unknown)=>Promise<any>; subscribe:(f:(e:unknown)=>void)=>()=>void }
export function createVisibilityApi(getBridge:()=>VisibilityBridge) {
 const getVisibility=():Promise<UiVisibility>=>getBridge().request('ui.visibility.get');
 return {getVisibility,onVisibilityChanged(callback:(state:UiVisibility)=>void,onError?:(error:unknown)=>void):()=>void {
  let live=true,sequence=-1,session:string|undefined;
  const retired=new Set<string>();
  const accept=(state:UiVisibility)=>{
   if(!live||!state||!Number.isSafeInteger(state.sequence))return;
   if(state.sessionId!==session){if(state.sessionId&&retired.has(state.sessionId))return;if(session)retired.add(session);session=state.sessionId;sequence=-1;}
   if(state.sequence>=sequence){sequence=state.sequence;callback(state);}
  };
  const off=getBridge().subscribe(event=>{const e=event as {type?:string;data?:UiVisibility};if(e?.type==='ui.visibility.changed'&&e.data)accept(e.data);});
  getVisibility().then(accept).catch(error=>{if(live)onError?.(error);});
  return()=>{live=false;off();};
 }};
}
/** Handle the manager-only JSON-line method in an existing backend dispatcher. */
export function registerVisibility(callback:(state:CaptureVisibility)=>unknown|Promise<unknown>) {
 return async(method:string,state:CaptureVisibility)=>{if(method!=='framely.ui.visibility')throw Error('Not a visibility method');return await callback(state);};
}
