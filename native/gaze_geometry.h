#pragma once
#include "launcher_gaze.h"
#include <algorithm>
struct GazeAngles {float yaw=0,pitch=0;};
inline bool gaze_head_angles(const vr::HmdMatrix34_t& head,const vr::HmdVector3_t& world,GazeAngles& out){
 float local[3]{};for(int i=0;i<3;i++)for(int r=0;r<3;r++){if(!std::isfinite(head.m[r][i])||!std::isfinite(world.v[r]))return false;local[i]+=head.m[r][i]*world.v[r];}
 if(local[0]*local[0]+local[1]*local[1]+local[2]*local[2]<1e-8)return false;
 out={std::atan2(local[0],-local[2]),std::atan2(local[1],std::hypot(local[0],local[2]))};return true;
}
inline vr::VROverlayIntersectionParams_t gaze_world_ray(const vr::HmdMatrix34_t& head,const vr::HmdVector3_t& origin,GazeAngles a){
 vr::VROverlayIntersectionParams_t ray{};ray.eOrigin=vr::TrackingUniverseStanding;ray.vSource=origin;
 float local[]{std::sin(a.yaw)*std::cos(a.pitch),std::sin(a.pitch),-std::cos(a.yaw)*std::cos(a.pitch)};
 for(int r=0;r<3;r++)for(int i=0;i<3;i++)ray.vDirection.v[r]+=head.m[r][i]*local[i];return ray;
}
inline bool gaze_angles_to_point(const vr::HmdMatrix34_t& head,const vr::HmdVector3_t& origin,const vr::HmdVector3_t& target,GazeAngles& angles){
 vr::HmdVector3_t direction{};for(int r=0;r<3;r++)direction.v[r]=target.v[r]-origin.v[r];return gaze_head_angles(head,direction,angles);
}
inline GazeAngles gaze_correct(GazeAngles raw,const double (&m)[6]){return {float(m[0]*raw.yaw+m[1]*raw.pitch+m[2]),float(m[3]*raw.yaw+m[4]*raw.pitch+m[5])};}
// Invert the runtime's actual curved-dashboard intersection. Its coordinate
// transform API alone may describe a flat surface. Sampling this inverse is
// needed only when the target or window changes; cache the returned world point.
template<class Sample> bool gaze_target_inverse(GazeAngles seed,float u,float v,Sample sample,GazeAngles& out){
 auto current=seed;float x=0,y=0;if(!sample(current,x,y))return false;
 for(int n=0;n<18;n++){
  float ex=x-u,ey=y-v;if(std::hypot(ex,ey)<.0003f){out=current;return true;}
  constexpr float delta=.0005f;float xx,xy,yx,yy;
  if(!sample({current.yaw+delta,current.pitch},xx,xy)||!sample({current.yaw,current.pitch+delta},yx,yy))return false;
  float a=(xx-x)/delta,b=(yx-x)/delta,c=(xy-y)/delta,d=(yy-y)/delta,det=a*d-b*c;if(!std::isfinite(det)||std::abs(det)<1e-6)return false;
  float dx=std::clamp((d*ex-b*ey)/det,-.15f,.15f),dy=std::clamp((-c*ex+a*ey)/det,-.15f,.15f);bool improved=false;
  for(float factor=1;factor>=.03125f;factor*=.5f){GazeAngles next{current.yaw-factor*dx,current.pitch-factor*dy};float nx,ny;if(sample(next,nx,ny)&&std::hypot(nx-u,ny-v)<std::hypot(ex,ey)){current=next;x=nx;y=ny;improved=true;break;}}
  if(!improved)return false;
 }
 return false;
}
