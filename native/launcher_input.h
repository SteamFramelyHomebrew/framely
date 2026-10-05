#pragma once
#include <chrono>
#include <algorithm>
#include <cmath>
#include "openvr.h"
struct EntryTrigger {
 bool held=false,ready=false;
 std::chrono::steady_clock::time_point last{};
 bool update(bool active,bool down,bool hit,std::chrono::steady_clock::time_point now){
  if(!active){held=false;ready=false;return false;}
  if(!ready){held=down;ready=true;return false;}
  bool rising=down&&!held;held=down;
  if(!rising||!hit||now-last<std::chrono::milliseconds(180))return false;
  last=now;return true;
 }
};

// Steam's laser supplies the actual tip hit and originating controller. Keep
// its point while the trigger locks the pointer, until an explicit focus leave.
struct EntryPointer {
 unsigned device=~0u;bool inside=false;
 void move(unsigned source,float x,float y){device=source;inside=x>=256&&x<=384&&y>=0&&y<=128;}
 void leave(){device=~0u;inside=false;}
 bool hit(unsigned source)const{return inside&&source!=~0u&&source==device;}
};

inline vr::HmdMatrix34_t entry_tip_pose(const vr::HmdMatrix34_t& tracked,const vr::HmdMatrix34_t& tip){
 vr::HmdMatrix34_t out{};
 for(int r=0;r<3;r++)for(int c=0;c<4;c++){
  for(int k=0;k<3;k++)out.m[r][c]+=tracked.m[r][k]*tip.m[k][c];
  if(c==3)out.m[r][c]+=tracked.m[r][3];
 }
 return out;
}

inline float entry_rotation_angle(const vr::HmdMatrix34_t& from,const vr::HmdMatrix34_t& to){
 float trace=0;for(int r=0;r<3;r++)for(int c=0;c<3;c++)trace+=from.m[r][c]*to.m[r][c];
 return std::acos(std::clamp((trace-1.f)*.5f,-1.f,1.f));
}
inline bool entry_trigger_pull(const vr::RenderModel_ComponentState_t& rest,const vr::RenderModel_ComponentState_t& full,const vr::RenderModel_ComponentState_t& current,float& pull){
 float travel=entry_rotation_angle(rest.mTrackingToComponentRenderModel,full.mTrackingToComponentRenderModel);
 if(!std::isfinite(travel)||travel<.001f)return false;
 pull=std::clamp(entry_rotation_angle(rest.mTrackingToComponentRenderModel,current.mTrackingToComponentRenderModel)/travel,0.f,1.f);return std::isfinite(pull);
}
struct EntryAnalogTrigger {
 bool held=false;
 bool update(float pull){held=std::isfinite(pull)&&pull>=(held?.45f:.5f);return held;}
};

// The press that opens a modal surface must not dismiss it as an outside click.
struct LauncherDismissGuard {
 std::chrono::steady_clock::time_point opened_at{};bool released=false;
 void opened(std::chrono::steady_clock::time_point now){opened_at=now;released=false;}
 void observe(bool held){if(!held)released=true;}
 bool allows(std::chrono::steady_clock::time_point now,bool held,float event_age=0){
  observe(held);
  return released&&now-opened_at>=std::chrono::milliseconds(300)&&
   event_age>=0&&std::chrono::duration<float>(now-opened_at).count()>=event_age;
 }
};

// One axis per gesture, with a dead zone and neutral re-arm. A diagonal never
// flips a category and a page together, even if its dominant axis changes.
struct LauncherStick {
 bool ready=false,latched=false;
 int update(float x,float y,bool active){
  if(!active||!std::isfinite(x)||!std::isfinite(y)){ready=false;latched=false;return 0;}
  float strength=std::max(std::abs(x),std::abs(y));
  if(strength<.3f){ready=true;latched=false;return 0;}
  if(!ready||latched||strength<.65f)return 0;
  latched=true;
  return std::abs(x)>=std::abs(y)?(x>0?1:-1):(y>0?2:-2);
 }
};
inline void launcher_rotation_vector(const vr::HmdMatrix34_t& rest,const vr::HmdMatrix34_t& value,float out[3]){
 float m[3][3]{};for(int r=0;r<3;r++)for(int c=0;c<3;c++)for(int k=0;k<3;k++)m[r][c]+=rest.m[k][r]*value.m[k][c];
 out[0]=m[2][1]-m[1][2];out[1]=m[0][2]-m[2][0];out[2]=m[1][0]-m[0][1];
}
inline bool launcher_stick_axes(const vr::RenderModel_ComponentState_t& rest,const vr::RenderModel_ComponentState_t& full_x,const vr::RenderModel_ComponentState_t& full_y,const vr::RenderModel_ComponentState_t& current,float& x,float& y){
 float a[3],b[3],v[3];launcher_rotation_vector(rest.mTrackingToComponentRenderModel,full_x.mTrackingToComponentRenderModel,a);launcher_rotation_vector(rest.mTrackingToComponentRenderModel,full_y.mTrackingToComponentRenderModel,b);launcher_rotation_vector(rest.mTrackingToComponentRenderModel,current.mTrackingToComponentRenderModel,v);
 float aa=0,ab=0,bb=0,av=0,bv=0;for(int i=0;i<3;i++){aa+=a[i]*a[i];ab+=a[i]*b[i];bb+=b[i]*b[i];av+=a[i]*v[i];bv+=b[i]*v[i];}
 float det=aa*bb-ab*ab;if(det<.00001f)return false;
 x=std::clamp((av*bb-bv*ab)/det,-1.f,1.f);y=std::clamp((bv*aa-av*ab)/det,-1.f,1.f);return std::isfinite(x)&&std::isfinite(y);
}
