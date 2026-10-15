import {clamp,fitFrame,type FrameRect} from './cast-framing';
export type MinimumFrame={width:number;height:number};
export function minimumFrameWidth(ratio:number,minimum:MinimumFrame,fovMinimum=0){
 return Math.min(1,ratio,Math.max(fovMinimum,minimum.width,minimum.height*ratio));
}
export function fitMinimumFrame(rect:FrameRect,ratio:number,minimum:number):FrameRect{
 const width=clamp(rect.width,minimum,Math.min(1,ratio)),height=width/ratio;
 const cx=clamp(rect.x+rect.width/2,width/2,1-width/2),cy=clamp(rect.y+rect.height/2,height/2,1-height/2);
 return {x:cx-width/2,y:cy-height/2,width,height};
}
export function resizeFrameCentered(rect:FrameRect,width:number,ratio:number,minimum:number):FrameRect{
 const cx=rect.x+rect.width/2,cy=rect.y+rect.height/2;
 const maximum=Math.min(2*cx,2*(1-cx),2*cy*ratio,2*(1-cy)*ratio);
 width=clamp(width,Math.min(minimum,maximum),maximum);
 return {x:cx-width/2,y:cy-width/ratio/2,width,height:width/ratio};
}
export class FrameGesture {
 private drag:{id:number;x:number;y:number;rect:FrameRect;corner:string|null}|null=null;
 begin(id:number,x:number,y:number,rect:FrameRect,corner:string|null){this.drag={id,x,y,rect:{...rect},corner};}
 active(id?:number){return !!this.drag&&(id===undefined||this.drag.id===id);}
 finish(id?:number){if(!this.active(id))return false;this.drag=null;return true;}
 move(id:number,x:number,y:number,buttons:number,stage:{width:number;height:number},ratio:number,minimum:number):FrameRect|null{
  const g=this.drag;if(!g||g.id!==id||stage.width<=0||stage.height<=0)return null;
  if(!(buttons&1)){this.finish(id);return null;}
  const dx=(x-g.x)/stage.width,dy=(y-g.y)/stage.height;
  if(!g.corner)return fitFrame({...g.rect,x:g.rect.x+dx,y:g.rect.y+dy},ratio,1,minimum);
  const sx=g.corner.includes('w')?-1:1,sy=g.corner.includes('n')?-1:1;
  // Project pointer travel onto the corner's aspect-constrained diagonal.
  // Both sides move equally: no opposite-corner/top-left anchoring.
  const delta=2*(sx*dx+sy*dy/ratio)/(1+1/(ratio*ratio));
  return resizeFrameCentered(g.rect,g.rect.width+delta,ratio,minimum);
 }
}
