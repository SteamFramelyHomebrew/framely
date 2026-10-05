#pragma once
#include <cstdint>
#include <cstddef>
#include <vector>
#include <chrono>
#include "geometry_math.h"

struct NotificationPlacement {
 vr::HmdMatrix34_t transform{};
 float width=.32f,radius=0;
};

// Keep the compact toast at the upper right, but turn its front toward the eye.
// Copying the headset rotation leaves a side-offset overlay facing past the user.
inline bool notification_placement(const vr::HmdMatrix34_t& headset,float radius,NotificationPlacement& out){
 vr::HmdMatrix34_t head{};
 if(!dock_rigid_frame(headset,head)||!std::isfinite(radius)||radius<=0)return false;
 vr::HmdVector3_t normal{},head_up{{head.m[0][1],head.m[1][1],head.m[2][1]}};
 for(int r=0;r<3;r++){
  out.transform.m[r][3]=head.m[r][3]+head.m[r][0]*.24f+head.m[r][1]*.12f-head.m[r][2]*1.f;
  normal.v[r]=head.m[r][3]-out.transform.m[r][3];
 }
 if(!normalize_dock_vector(normal))return false;
 auto right=cross_dock_vector(head_up,normal);
 if(!normalize_dock_vector(right))return false;
 auto up=cross_dock_vector(normal,right);
 for(int r=0;r<3;r++){
  out.transform.m[r][0]=right.v[r];out.transform.m[r][1]=up.v[r];out.transform.m[r][2]=normal.v[r];
 }
 out.radius=radius;return true;
}

struct NotificationBounds {
 float x=0,y=0,width=0,height=0;
 bool visible() const {return width>0&&height>0;}
};

// BGRA rows are bottom-up after on_paint. Intersection rectangles use top-left
// coordinates, so the alpha bounds must be flipped back before submitting the mask.
inline NotificationBounds notification_bounds(const std::vector<uint8_t>& pixels,int width,int height,int scale){
 if(width<=0||height<=0||scale<=0||pixels.size()!=size_t(width)*height*4)return {};
 int left=width,low=height,right=-1,high=-1;
 for(int y=0;y<height;++y)for(int x=0;x<width;++x){
  if(!pixels[(size_t(y)*width+x)*4+3])continue;
  if(x<left)left=x;if(x>right)right=x;if(y<low)low=y;if(y>high)high=y;
 }
 if(right<left)return {};
 return {float(left)/scale,float(height-1-high)/scale,float(right-left+1)/scale,float(high-low+1)/scale};
}

// Preserve the intended button during a brief click with normal tracking jitter.
// Deliberate movement and long holds cancel stabilization.
struct NotificationClick {
 using Time=std::chrono::steady_clock::time_point;
 bool active=false,cancelled=false;int x=0,y=0;Time pressed{};
 void begin(int px,int py,Time now){active=true;cancelled=false;x=px;y=py;pressed=now;}
 bool hold(int px,int py,Time now){
  if(!active)return false;
  if(now-pressed>std::chrono::milliseconds(800)||std::hypot(float(px-x),float(py-y))>18.f)cancelled=true;
  return !cancelled;
 }
 bool release(int& px,int& py,Time now){
  bool stable=hold(px,py,now);if(stable){px=x;py=y;}active=false;return stable;
 }
 void clear(){active=false;cancelled=false;}
};
