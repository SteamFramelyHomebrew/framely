import {frameFromSettings,settingsFromFrame,clamp,type ReferenceView,type FrameRect} from './cast-framing';
import type {CastSettings} from './casting';
import {fitMinimumFrame,minimumFrameWidth,type MinimumFrame} from './cast-framing-gesture';
// Canonicalize the displayed box BEFORE saving; ratio changes must not leave
// a clamped visual rectangle paired with different capture parameters.
export type FrameLimiter=(rect:FrameRect,ratio:number,minimum:number,previous?:FrameRect)=>FrameRect;
export function normalizeRegion(settings:CastSettings,reference:ReferenceView,minimum:MinimumFrame={width:0,height:0},limit?:FrameLimiter,previous?:CastSettings):CastSettings{
 const bounded={...settings,horizontalFov:clamp(settings.horizontalFov??90,10,150),centerX:clamp(settings.centerX,-60,60),centerY:clamp(settings.centerY,-60,60)};
 const ratio=settings.width/settings.height/reference.aspect;
 const minWidth=minimumFrameWidth(ratio,minimum,Math.tan(5*Math.PI/180)/reference.tanHalfHorizontal);
 let rect=fitMinimumFrame(frameFromSettings(bounded,settings.width/settings.height,reference),ratio,minWidth);
 if(limit)rect=limit(rect,ratio,minWidth,previous?frameFromSettings(previous,previous.width/previous.height,reference):undefined);
 const next=settingsFromFrame(rect,reference);
 return {...bounded,...next,centerX:clamp(next.centerX,-60,60),centerY:clamp(next.centerY,-60,60)};
}
