export interface LaunchContext {source:'launcher'|'quickPanel'|'manager';trigger:'shortPress'|'menuAction';actionId?:string}
type Bridge={request:(op:string,p?:unknown)=>Promise<any>;subscribe:(f:(e:any)=>void)=>()=>void};
function bridge():Bridge{const b=(window as any).__framelyBridge;if(!b)throw new Error('This script requires the Framely plugin host');return b;}
export const launchContext={get:():Promise<LaunchContext>=>bridge().request('ui.launchContext'),onChanged:(callback:(context:LaunchContext)=>void)=>bridge().subscribe(e=>{if(e.type==='ui.launch')callback(e.data);})};
/** Register callbacks in a separate bundle named by a frontend launcher action. */
export function registerLauncherActions(actions:Record<string,(context:LaunchContext)=>unknown|Promise<unknown>>){
 let busy=false;const off=bridge().subscribe(async e=>{if(e.type!=='launcher.action'||busy)return;busy=true;try{const context=e.data as LaunchContext;const action=actions[context.actionId??''];if(!action)throw new Error('Launcher action handler not registered');await action(context);await bridge().request('launcher.done');}catch(error){await bridge().request('launcher.done',{error:String(error)});}finally{busy=false;}});
 void bridge().request('launcher.ready').catch(console.error);window.addEventListener('pagehide',off,{once:true});return off;
}
