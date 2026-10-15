#pragma once
#include <array>
#include <cmath>
#include <optional>
#include "openvr.h"

// Pure geometry only. Pose validity/freshness is owned by the caller: a
// numerically rigid matrix cannot reveal whether its tracking sample is old.
namespace recording_region_depth_detail {
using Point=std::array<double,3>;
inline double dot(const Point& a,const Point& b){return a[0]*b[0]+a[1]*b[1]+a[2]*b[2];}
inline Point translation(const vr::HmdMatrix34_t& m){return {m.m[0][3],m.m[1][3],m.m[2][3]};}
inline Point rotate(const vr::HmdMatrix34_t& m,const Point& p){
 Point result{};for(int i=0;i<3;i++)for(int j=0;j<3;j++)result[i]+=m.m[i][j]*p[j];return result;
}
inline Point add(const Point& a,const Point& b){return {a[0]+b[0],a[1]+b[1],a[2]+b[2]};}
inline Point subtract(const Point& a,const Point& b){return {a[0]-b[0],a[1]-b[1],a[2]-b[2]};}
inline bool rigid(const vr::HmdMatrix34_t& m){
 for(const auto& row:m.m)for(float v:row)if(!std::isfinite(v))return false;
 for(int i=0;i<3;i++)for(int j=0;j<3;j++){
  double product=0;for(int k=0;k<3;k++)product+=double(m.m[k][i])*m.m[k][j];
  if(std::abs(product-(i==j?1.:0.))>0.01)return false;
 }
 const double determinant=m.m[0][0]*(double(m.m[1][1])*m.m[2][2]-double(m.m[1][2])*m.m[2][1])
  -m.m[0][1]*(double(m.m[1][0])*m.m[2][2]-double(m.m[1][2])*m.m[2][0])
  +m.m[0][2]*(double(m.m[1][0])*m.m[2][1]-double(m.m[1][1])*m.m[2][0]);
 return std::abs(determinant-1.)<=0.01;
}
}

// Refresh just the eye origin. Capture's basis already encodes the LUT's ray
// convention and must not receive the physical-panel rotation a second time.
inline bool recording_region_refresh_eye_origin(vr::EVREye eye,
 const vr::HmdMatrix34_t& left_to_head,const vr::HmdMatrix34_t& right_to_head,
 vr::HmdMatrix34_t& capture_to_head){
 using namespace recording_region_depth_detail;
 if((eye!=vr::Eye_Left&&eye!=vr::Eye_Right)||!rigid(left_to_head)||!rigid(right_to_head)||!rigid(capture_to_head))return false;
 const auto left=translation(left_to_head),right=translation(right_to_head),baseline=subtract(right,left);
 const double ipd=std::sqrt(dot(baseline,baseline));
 // Broad plausibility guards, not a replacement for live tracking validity.
 if(ipd<0.04||ipd>0.09||baseline[0]<0.8*ipd||std::abs(baseline[1])>0.02||std::abs(baseline[2])>0.02
  ||dot(left,left)>0.25*0.25||dot(right,right)>0.25*0.25)return false;
 const auto& selected=eye==vr::Eye_Left?left_to_head:right_to_head;
 for(int i=0;i<3;i++)capture_to_head.m[i][3]=selected.m[i][3];
 return true;
}

// Intersect the selected capture-eye center ray with a known UI window plane.
// The returned parameter D is measured along capture -Z, not Euclidean ray
// length: X = eye + D * basis * (centerTanX,-centerTanY,-1).
// It therefore plugs directly into a guide whose width is 2*tanHalfFov*D.
inline std::optional<double> recording_region_window_depth(
 const vr::HmdMatrix34_t& hmd_to_world,const vr::HmdMatrix34_t& window_to_world,
 const vr::HmdMatrix34_t& capture_to_head,double center_tan_x,double center_tan_y){
 using namespace recording_region_depth_detail;
 if(!rigid(hmd_to_world)||!rigid(window_to_world)||!rigid(capture_to_head)
  ||!std::isfinite(center_tan_x)||!std::isfinite(center_tan_y)
  ||std::abs(center_tan_x)>10.||std::abs(center_tan_y)>10.
  ||dot(translation(capture_to_head),translation(capture_to_head))>0.25*0.25)return std::nullopt;
 const Point origin=add(translation(hmd_to_world),rotate(hmd_to_world,translation(capture_to_head)));
 const Point ray=rotate(hmd_to_world,rotate(capture_to_head,{center_tan_x,-center_tan_y,-1.}));
 const Point normal={window_to_world.m[0][2],window_to_world.m[1][2],window_to_world.m[2][2]};
 const double denominator=dot(normal,ray);
 if(std::abs(denominator)<1e-5*std::sqrt(dot(ray,ray)))return std::nullopt;
 const double distance=dot(normal,subtract(translation(window_to_world),origin))/denominator;
 if(!std::isfinite(distance)||distance<0.25||distance>6.)return std::nullopt;
 return distance;
}

// ComputeOverlayIntersection can supply an exact curved/tilted UI hit. Recover
// the same capture -Z parameter, rejecting unrelated/off-ray intersection data.
inline std::optional<double> recording_region_hit_depth(
 const vr::HmdMatrix34_t& hmd_to_world,const vr::HmdMatrix34_t& capture_to_head,
 double center_tan_x,double center_tan_y,const vr::HmdVector3_t& world_hit){
 using namespace recording_region_depth_detail;
 if(!rigid(hmd_to_world)||!rigid(capture_to_head)
  ||!std::isfinite(center_tan_x)||!std::isfinite(center_tan_y)
  ||std::abs(center_tan_x)>10.||std::abs(center_tan_y)>10.
  ||dot(translation(capture_to_head),translation(capture_to_head))>0.25*0.25)return std::nullopt;
 Point hit{};for(int i=0;i<3;i++){hit[i]=world_hit.v[i];if(!std::isfinite(hit[i]))return std::nullopt;}
 const Point origin=add(translation(hmd_to_world),rotate(hmd_to_world,translation(capture_to_head)));
 const Point ray=rotate(hmd_to_world,rotate(capture_to_head,{center_tan_x,-center_tan_y,-1.}));
 const Point delta=subtract(hit,origin);
 const double squared_length=dot(ray,ray),distance=dot(delta,ray)/squared_length;
 if(!std::isfinite(distance)||distance<0.25||distance>6.)return std::nullopt;
 const Point residual={delta[0]-distance*ray[0],delta[1]-distance*ray[1],delta[2]-distance*ray[2]};
 const double tolerance=1e-4+1e-4*distance*std::sqrt(squared_length);
 if(dot(residual,residual)>tolerance*tolerance)return std::nullopt;
 return distance;
}
