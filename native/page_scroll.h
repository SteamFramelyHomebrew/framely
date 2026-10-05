#pragma once
#include <algorithm>
#include <chrono>
#include <cmath>
struct PageScrollDelta {int x=0,y=0;};
// Integrate speed in logical UI pixels, independently of render scale/FPS.
struct PageScrollStick {
 std::chrono::steady_clock::time_point last{};float remainder=0;int axis=0;
 PageScrollDelta update(float x,float y,bool active,std::chrono::steady_clock::time_point now){
  float dt=last.time_since_epoch().count()?std::clamp(std::chrono::duration<float>(now-last).count(),0.f,.05f):1.f/60;
  last=now;PageScrollDelta out;
  if(!active||!std::isfinite(x)||!std::isfinite(y)||std::max(std::abs(x),std::abs(y))<=.18f){remainder=0;axis=0;return out;}
  int next=std::abs(y)>=std::abs(x)?2:1;if(next!=axis){remainder=0;axis=next;}
  float value=axis==2?y:-x;
  remainder+=std::copysign((std::min(std::abs(value),1.f)-.18f)/.82f,value)*1100.f*dt;
  int pixels=int(remainder);remainder-=pixels;if(axis==2)out.y=pixels;else out.x=pixels;return out;
 }
 void clear(){last={};remainder=0;axis=0;}
};
