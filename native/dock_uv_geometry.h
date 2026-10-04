#pragma once
#include "geometry_math.h"
struct DockUVState {float x=0,y=0;unsigned calls=0;};
template<class Overlay> inline bool dock_uv_anchor(Overlay *o,vr::VROverlayHandle_t dock,const vr::HmdMatrix34_t &center,const vr::HmdVector3_t &head,float width,float height,vr::HmdMatrix34_t &anchor,DockCurveEvidence &ev,float &physical_size,DockUVState &state){
 state.calls=0;
 auto ray=[&](float x,float y,vr::VROverlayIntersectionResults_t &h){
  ++state.calls;vr::VROverlayIntersectionParams_t p{};p.eOrigin=vr::TrackingUniverseStanding;p.vSource=head;
  for(int r=0;r<3;r++)p.vDirection.v[r]=center.m[r][3]+center.m[r][0]*(width*.5f*x)+center.m[r][1]*y-head.v[r];
  return normalize_dock_vector(p.vDirection)&&o->ComputeOverlayIntersection(dock,&p,&h)&&std::isfinite(h.vUVs.v[0])&&std::isfinite(h.vUVs.v[1]);
 };
 auto solve=[&](float u,float v,float &x,float &y,vr::VROverlayIntersectionResults_t &h){
  for(int i=0;i<14;i++){
   if(!ray(x,y,h))return false;
   float eu=u-h.vUVs.v[0],evv=v-h.vUVs.v[1];if(std::abs(eu)<.00005f&&std::abs(evv)<.00005f)return true;
   vr::VROverlayIntersectionResults_t hx{},hy{};float dx=.001f,dy=height*.005f;
   if(!ray(x+dx,y,hx)){dx=-dx;if(!ray(x+dx,y,hx))return false;}
   if(!ray(x,y+dy,hy)){dy=-dy;if(!ray(x,y+dy,hy))return false;}
   float a=(hx.vUVs.v[0]-h.vUVs.v[0])/dx,b=(hy.vUVs.v[0]-h.vUVs.v[0])/dy;
   float c=(hx.vUVs.v[1]-h.vUVs.v[1])/dx,d=(hy.vUVs.v[1]-h.vUVs.v[1])/dy,det=a*d-b*c;
   if(!std::isfinite(det)||std::abs(det)<1e-6f)return false;
   float sx=(eu*d-b*evv)/det,sy=(a*evv-eu*c)/det;
   float factor=std::min(1.f,.2f/std::max(std::abs(sx),.00001f));factor=std::min(factor,height*.4f/std::max(std::abs(sy),.00001f));
   bool accepted=false;
   for(int j=0;j<8;j++){
    vr::VROverlayIntersectionResults_t trial{};
    if(ray(x+sx*factor,y+sy*factor,trial)&&std::hypot(u-trial.vUVs.v[0],v-trial.vUVs.v[1])<std::hypot(eu,evv)){x+=sx*factor;y+=sy*factor;accepted=true;break;}
    factor*=.5f;
   }
   if(!accepted)return false;
  }return false;
 };
 float x=0,y=0;vr::VROverlayIntersectionResults_t edge{};
 if(!solve(.5f,.5f,x,y,edge)){
  bool found=false;
  for(int i=1;i<=24&&!found;i++)for(float sign:{-1.f,1.f}){
   float sx=0,sy=sign*i*height*.25f;
   if(solve(.5f,.5f,sx,sy,edge)){x=sx;y=sy;found=true;break;}
  }
  if(!found)return false;
 }
 float lo=.5f,hi=1.2f;
 float outside_x=x,outside_y=y;vr::VROverlayIntersectionResults_t outside{};
 if(solve(hi,.5f,outside_x,outside_y,outside))return false;
 for(int i=0;i<15;i++){
  float u=(lo+hi)*.5f,nx=x,ny=y;vr::VROverlayIntersectionResults_t candidate{};
  if(solve(u,.5f,nx,ny,candidate)){lo=u;x=nx;y=ny;edge=candidate;}else hi=u;
 }
 if(lo<.7f||hi-lo>.0001f)return false;
 // Retreat inside the measured intersection boundary for stable derivatives.
 float inner=lo-.001f;
 if(!solve(inner,.5f,x,y,edge))return false;
 state.x=x;state.y=y;ev.edge=edge;ev.bracket_lo=lo;ev.bracket_hi=hi;
 float nx=x,ny=y;vr::VROverlayIntersectionResults_t near{},vertical{};
 if(!solve(inner-.01f,.5f,nx,ny,near))return false;
 nx=x;ny=y;if(!solve(inner,.48f,nx,ny,vertical))return false;
 vr::HmdVector3_t z=edge.vNormal; if(!normalize_dock_vector(z))return false;
 for(int r=0;r<3;r++)ev.tangent.v[r]=edge.vPoint.v[r]-near.vPoint.v[r];
 float projection=0;for(int r=0;r<3;r++)projection+=ev.tangent.v[r]*z.v[r];
 for(int r=0;r<3;r++)ev.tangent.v[r]-=z.v[r]*projection;
 if(!normalize_dock_vector(ev.tangent))return false;
 ev.up=cross_dock_vector(z,ev.tangent);if(!normalize_dock_vector(ev.up))return false;
 float length=0;for(int r=0;r<3;r++){float v=vertical.vPoint.v[r]-edge.vPoint.v[r];length+=v*v;}
 // Keep up = normal x tangent: changing one column would create a reflection.
 physical_size=std::sqrt(length)/.02f;if(!std::isfinite(physical_size)||physical_size<.025f||physical_size>.15f)return false;
 vr::VROverlayIntersectionResults_t far{},farther{};nx=x;ny=y;
 bool samples=solve(inner-.12f,.5f,nx,ny,far);nx=x;ny=y;
 samples=samples&&solve(inner-.24f,.5f,nx,ny,farther);
 vr::HmdVector3_t circle{},axis{};
 bool curved=samples&&dock_row_circle(edge.vPoint,far.vPoint,farther.vPoint,circle,axis,ev.radius);
 // A placeholder identity plane is not valid Frame Dock geometry.
 if(!curved)return false;
 for(int r=0;r<3;r++){anchor.m[r][0]=ev.tangent.v[r];anchor.m[r][1]=ev.up.v[r];anchor.m[r][2]=z.v[r];anchor.m[r][3]=edge.vPoint.v[r]+(edge.vPoint.v[r]-near.vPoint.v[r])*.1f+ev.tangent.v[r]*physical_size*.65f;}
 if(curved){
  // Choose the upward axis using the measured texture-height direction.
  float sign=0;for(int r=0;r<3;r++)sign+=axis.v[r]*(vertical.vPoint.v[r]-edge.vPoint.v[r]);
  if(sign>0)for(float& v:axis.v)v=-v;
  vr::HmdVector3_t radial{};for(int r=0;r<3;r++)radial.v[r]=circle.v[r]-edge.vPoint.v[r];
  if(!normalize_dock_vector(radial))return false;
  auto tangent=cross_dock_vector(axis,radial);if(!normalize_dock_vector(tangent))return false;
  float axial=0,front=0;for(int r=0;r<3;r++){axial+=z.v[r]*axis.v[r];front+=z.v[r]*radial.v[r];}
  ev.pre_curve_pitch=-std::atan2(axial,front);
  for(int r=0;r<3;r++){anchor.m[r][0]=tangent.v[r];anchor.m[r][1]=axis.v[r];anchor.m[r][2]=radial.v[r];anchor.m[r][3]=edge.vPoint.v[r];}
  float chord=0;for(int r=0;r<3;r++){float d=edge.vPoint.v[r]-near.vPoint.v[r];chord+=d*d;}
  advance_dock_curve(anchor,ev.radius,std::sqrt(chord)*.1f+physical_size*.65f);
 }else{ev.radius=0;ev.pre_curve_pitch=0;}
 return true;
}
