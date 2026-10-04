#pragma once
#include "geometry_math.h"
struct OverlayPlacement {
 vr::HmdMatrix34_t transform{};float width=0,curvature=-1,pitch=0;bool valid=false;
 bool changed(const vr::HmdMatrix34_t& next,float next_width)const{
  if(!valid||std::abs(next_width-width)>.0005f)return true;
  for(int r=0;r<3;r++)for(int c=0;c<4;c++)if(std::abs(next.m[r][c]-transform.m[r][c])>(c==3?.00075f:.001f))return true;
  return false;
 }
 template<class Overlay> bool curve(Overlay* o,vr::VROverlayHandle_t handle,float next_width,float radius,float next_pitch=0){
  float next=radius>0?next_width/(2.f*3.141592653589793f*radius):0;
  if(std::abs(next-curvature)<.00002f&&std::abs(next_pitch-pitch)<.0005f)return false;
  if(o->SetOverlayCurvature(handle,next)||o->SetOverlayPreCurvePitch(handle,next_pitch))return false;
  curvature=next;pitch=next_pitch;return true;
 }
 template<class Overlay> bool apply(Overlay* o,vr::VROverlayHandle_t handle,const vr::HmdMatrix34_t& next,float next_width){
  if(!changed(next,next_width))return false;
  if(!valid||std::abs(width-next_width)>.0005f){if(o->SetOverlayWidthInMeters(handle,next_width))return false;}
  if(o->SetOverlayTransformAbsolute(handle,vr::TrackingUniverseStanding,&next))return false;
  transform=next;width=next_width;valid=true;return true;
 }
};
