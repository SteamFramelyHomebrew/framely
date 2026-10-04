/** Backend protocol types. Page mounting remains a separate React lifecycle. */
export type LifecyclePhase='onInstall'|'onUpdate'|'onStart'|'onStop'|'onUninstall'|'onCrashCleanup';
export interface LifecycleContext {
 pluginId:string;phase:LifecyclePhase;reason:string;version:string;
 previousVersion:string|null;dataDir:string;
 exit:{reason:string;exitCode?:number|null;signal?:number|null;oom?:boolean;message?:string}|null;
}
export type LifecycleCallbacks=Partial<Record<LifecyclePhase,(context:LifecycleContext)=>unknown|Promise<unknown>>>;
/** Embed in a Node/TS backend's existing JSON-line request dispatcher. */
export function registerLifecycle(callbacks:LifecycleCallbacks){return async(method:string,context:LifecycleContext)=>{
 const phase=method==='framely.lifecycle.start'?'onStart':method==='framely.lifecycle.stop'?'onStop':context.phase;
 const callback=callbacks[phase];if(!callback)throw Error(`Lifecycle callback not registered: ${phase}`);
 return await callback(context);
};}
