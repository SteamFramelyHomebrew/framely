import type {FrameRect} from './cast-framing';
import {fitMinimumFrame,resizeFrameCentered} from './cast-framing-gesture';
export type VisibilityGrid={size:number;extent:number;cells:string};
export const framingMenuScale=(referenceTan:number)=>0.9/referenceTan;
export class FramingLimits {
 private readonly prefix:Int32Array;
 readonly size:number;
 constructor(grid:VisibilityGrid,referenceTan:number){
  const n=grid?.size;
  if(!Number.isInteger(n)||n<8||n>256||n%4||grid.extent!==referenceTan||grid.cells?.length!==n*n/4||!/^[0-9a-f]+$/i.test(grid.cells))throw Error('无法读取实际录制区域，请更新采集程序');
  this.size=n;this.prefix=new Int32Array((n+1)*(n+1));
  for(let y=0;y<n;y++)for(let x=0;x<n;x++){
   const i=y*n+x,invalid=(parseInt(grid.cells[i>>2],16)&(1<<(i&3)))?0:1,p=(y+1)*(n+1)+x+1;
   this.prefix[p]=invalid+this.prefix[p-1]+this.prefix[p-n-1]-this.prefix[p-n-2];
  }
 }
 contains(rect:FrameRect){
  const n=this.size;
  // Include a one-cell halo: the visible outline is outside the capture
  // boundary, and must not straddle an invalid lens/panel cell.
  const x0=Math.floor(rect.x*n+1e-9)-1,y0=Math.floor(rect.y*n+1e-9)-1,x1=Math.ceil((rect.x+rect.width)*n-1e-9)+1,y1=Math.ceil((rect.y+rect.height)*n-1e-9)+1;
  if(![rect.x,rect.y,rect.width,rect.height].every(Number.isFinite)||rect.width<=0||rect.height<=0||x0<0||y0<0||x1>n||y1>n)return false;
  const stride=n+1,p=this.prefix;
  return p[y1*stride+x1]-p[y0*stride+x1]-p[y1*stride+x0]+p[y0*stride+x0]===0;
 }
 fit(rect:FrameRect,ratio:number,minimum:number,previous?:FrameRect):FrameRect{
  rect=fitMinimumFrame(rect,ratio,minimum);
  if(this.contains(rect))return rect;
  const centered=(cx:number,cy:number,width:number):FrameRect=>({x:cx-width/2,y:cy-width/ratio/2,width,height:width/ratio});
  let cx=rect.x+rect.width/2,cy=rect.y+rect.height/2;
  if(previous&&Math.abs(previous.width/previous.height-ratio)<1e-9&&previous.width>=minimum-1e-9&&this.contains(previous)){
   if(Math.abs(rect.width-previous.width)<1e-9){
    let lo=0,hi=1;for(let k=0;k<36;k++){const t=(lo+hi)/2,next={...rect,x:previous.x+(rect.x-previous.x)*t,y:previous.y+(rect.y-previous.y)*t};if(this.contains(next))lo=t;else hi=t;}
    return {...rect,x:previous.x+(rect.x-previous.x)*lo,y:previous.y+(rect.y-previous.y)*lo};
   }
  }
  if(!this.contains(centered(cx,cy,minimum))){
   const targetX=cx,targetY=cy;
   let distance=Infinity,found=false;
   for(let y=0;y<this.size;y++)for(let x=0;x<this.size;x++){
    const nx=(x+.5)/this.size,ny=(y+.5)/this.size,d=(nx-targetX)**2+(ny-targetY)**2;
    if(d<distance&&this.contains(centered(nx,ny,minimum))){distance=d;found=true;cx=nx;cy=ny;}
   }
   if(!found)throw Error('菜单尺寸超过当前可录制范围');
  }
  let lo=minimum,hi=rect.width;
  for(let k=0;k<36;k++){const width=(lo+hi)/2;if(this.contains(centered(cx,cy,width)))lo=width;else hi=width;}
  return centered(cx,cy,lo);
 }
 reset(ratio:number,minimum:number){
  return this.fit(resizeFrameCentered({x:0,y:0,width:1,height:1},Math.min(1,ratio),ratio,minimum),ratio,minimum);
 }
}
