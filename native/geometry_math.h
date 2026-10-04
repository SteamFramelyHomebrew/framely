#pragma once
#include "openvr.h"
#include <cmath>
#include <algorithm>
#include <cstdio>
struct DockCurveEvidence {
 float bracket_lo=0,bracket_hi=0,radius=0,pre_curve_pitch=0;
 vr::VROverlayIntersectionResults_t edge{};
 vr::HmdVector3_t tangent{},up{};
};
// Coordinate transforms returned by OpenVR include surface scale. Strip it
// before carrying a measured pose with the Dock's rigid motion between probes.
inline bool dock_rigid_frame(const vr::HmdMatrix34_t& surface,vr::HmdMatrix34_t& frame){
 frame=surface;
 for(int c=0;c<3;c++){
  float n=0;for(int r=0;r<3;r++)n+=surface.m[r][c]*surface.m[r][c];
  n=std::sqrt(n);if(!std::isfinite(n)||n<1e-6f)return false;
  for(int r=0;r<3;r++)frame.m[r][c]/=n;
 }
 for(int r=0;r<3;r++)if(!std::isfinite(frame.m[r][3]))return false;
 for(int a=0;a<3;a++)for(int b=a+1;b<3;b++){
  float dot=0;for(int r=0;r<3;r++)dot+=frame.m[r][a]*frame.m[r][b];
  if(std::abs(dot)>.001f)return false;
 }
 return true;
}
inline vr::HmdMatrix34_t dock_follow_pose(const vr::HmdMatrix34_t& pose,const vr::HmdMatrix34_t& previous,const vr::HmdMatrix34_t& current){
 vr::HmdMatrix34_t result{};
 for(int c=0;c<4;c++)for(int r=0;r<3;r++){
  for(int k=0;k<3;k++){
   float local=0;for(int j=0;j<3;j++)local+=previous.m[j][k]*(pose.m[j][c]-(c==3?previous.m[j][3]:0));
   result.m[r][c]+=current.m[r][k]*local;
  }
  if(c==3)result.m[r][c]+=current.m[r][3];
 }
 return result;
}
inline bool normalize_dock_vector(vr::HmdVector3_t &v){
 float n=0;for(float f:v.v){if(!std::isfinite(f))return false;n+=f*f;}
 n=std::sqrt(n);if(n<1e-6f)return false;for(float &f:v.v)f/=n;return true;
}
inline vr::HmdVector3_t cross_dock_vector(const vr::HmdVector3_t &a,const vr::HmdVector3_t &b){return {{a.v[1]*b.v[2]-a.v[2]*b.v[1],a.v[2]*b.v[0]-a.v[0]*b.v[2],a.v[0]*b.v[1]-a.v[1]*b.v[0]}};}

// Fit a radius from measured 3D surface points and normals. Projecting onto
// world XZ incorrectly changes the radius when the user tilts the Dashboard.
inline float dock_surface_radius(const vr::VROverlayIntersectionResults_t& a,const vr::VROverlayIntersectionResults_t& b){
 auto na=a.vNormal,nb=b.vNormal;if(!normalize_dock_vector(na)||!normalize_dock_vector(nb))return 0;
 float dot=0,chord=0;for(int r=0;r<3;r++){dot+=na.v[r]*nb.v[r];float d=a.vPoint.v[r]-b.vPoint.v[r];chord+=d*d;}
 float sine=std::sqrt(std::max(0.f,(1.f-std::clamp(dot,-1.f,1.f))*.5f));
 if(sine<.002f)return 0;
 float radius=std::sqrt(chord)/(2*sine);
 return std::isfinite(radius)&&radius>.2f&&radius<20.f?radius:0;
}
inline void advance_dock_curve(vr::HmdMatrix34_t& pose,float radius,float distance){
 float angle=distance/radius,c=std::cos(angle),s=std::sin(angle);
 for(int r=0;r<3;r++){float t=pose.m[r][0],n=pose.m[r][2];pose.m[r][3]+=radius*(t*s+n*(1-c));pose.m[r][0]=t*c+n*s;pose.m[r][2]=n*c-t*s;}
}

// Probe from the front of the measured surface, independent of headset placement.
inline bool dock_probe_origin(const vr::HmdMatrix34_t& center, vr::HmdVector3_t& origin) {
 vr::HmdVector3_t normal{{center.m[0][2],center.m[1][2],center.m[2][2]}};
 if(!normalize_dock_vector(normal))return false;
 for(int r=0;r<3;r++){origin.v[r]=center.m[r][3]+normal.v[r];if(!std::isfinite(origin.v[r]))return false;}
 return true;
}

// A row of Steam's pitched Dock is a circle. Fit its actual center and axis
// from three points: cross(surface normals) is not its axis for a pitched fan.
inline bool dock_row_circle(const vr::HmdVector3_t& p,const vr::HmdVector3_t& q,const vr::HmdVector3_t& r,vr::HmdVector3_t& center,vr::HmdVector3_t& axis,float& radius){
 vr::HmdVector3_t a{},b{};float aa=0,bb=0;
 for(int i=0;i<3;i++){a.v[i]=q.v[i]-p.v[i];b.v[i]=r.v[i]-p.v[i];aa+=a.v[i]*a.v[i];bb+=b.v[i]*b.v[i];}
 axis=cross_dock_vector(a,b);float nn=0;for(float v:axis.v)nn+=v*v;
 if(nn<1e-12f||!std::isfinite(nn))return false;
 auto ca=cross_dock_vector(axis,a),bc=cross_dock_vector(b,axis);float rr=0;
 for(int i=0;i<3;i++){float d=(bb*ca.v[i]+aa*bc.v[i])/(2*nn);center.v[i]=p.v[i]+d;rr+=d*d;}
 radius=std::sqrt(rr);return normalize_dock_vector(axis)&&std::isfinite(radius)&&radius>.2f&&radius<20.f;
}
