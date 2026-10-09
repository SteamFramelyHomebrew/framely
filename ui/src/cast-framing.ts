export type FrameRect={x:number;y:number;width:number;height:number};
export type ReferenceView={tanHalfHorizontal:number;centerTanX:number;centerTanY:number;aspect:number};
export const clamp=(v:number,min:number,max:number)=>Math.max(min,Math.min(max,v));
export const frameRatios=[{label:'16:9',value:16/9},{label:'16:10',value:16/10},{label:'4:3',value:4/3},{label:'1:1',value:1},{label:'9:16',value:9/16}];
export function framePresets(aspect:number){
 return [1280,1920,2560,3840].map(edge=>aspect>=1?{width:edge,height:Math.round(edge/aspect/2)*2}:{width:Math.round(edge*aspect/2)*2,height:edge});
}
export function fitFrame(rect:FrameRect,aspect:number,referenceAspect=1,minWidth=.1):FrameRect{
 const ratio=aspect/referenceAspect;
 const width=clamp(rect.width,Math.min(minWidth,Math.min(1,ratio)),Math.min(1,ratio));
 const height=width/ratio;
 return {width,height,x:clamp(rect.x,0,1-width),y:clamp(rect.y,0,1-height)};
}
export function frameFromSettings(settings:{horizontalFov:number|null;centerX:number;centerY:number},aspect:number,view:ReferenceView):FrameRect{
 const maxWidth=Math.min(1,aspect/view.aspect);
 const width=settings.horizontalFov===null?maxWidth:Math.tan(settings.horizontalFov*Math.PI/360)/view.tanHalfHorizontal;
 const height=width*view.aspect/aspect;
 const x=.5+(Math.tan(settings.centerX*Math.PI/180)-view.centerTanX)/(2*view.tanHalfHorizontal)-width/2;
 const y=.5+(Math.tan(settings.centerY*Math.PI/180)-view.centerTanY)*view.aspect/(2*view.tanHalfHorizontal)-height/2;
 return fitFrame({x,y,width,height},aspect,view.aspect,Math.tan(5*Math.PI/180)/view.tanHalfHorizontal);
}
export function settingsFromFrame(rect:FrameRect,view:ReferenceView){
 return {horizontalFov:2*Math.atan(rect.width*view.tanHalfHorizontal)*180/Math.PI,
 centerX:Math.atan(view.centerTanX+(rect.x+rect.width/2-.5)*2*view.tanHalfHorizontal)*180/Math.PI,
 centerY:Math.atan(view.centerTanY+(rect.y+rect.height/2-.5)*2*view.tanHalfHorizontal/view.aspect)*180/Math.PI};
}
