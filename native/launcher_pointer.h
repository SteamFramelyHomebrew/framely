#pragma once
#include <chrono>
#include <cmath>
// Keep the initial hit during ordinary laser jitter, including a long press.
// A deliberate move or an abandoned hold releases the lock.
struct LauncherPointer {
 using Time=std::chrono::steady_clock::time_point;
 bool active=false,cancelled=false;int x=0,y=0;Time pressed{};
 void begin(int px,int py,Time now){active=true;cancelled=false;x=px;y=py;pressed=now;}
 bool holding(Time now)const{return active&&!cancelled&&now-pressed<=std::chrono::milliseconds(2500);}
 bool constrain(int& px,int& py,Time now){
  if(!active)return false;
  if(!holding(now)||std::hypot(float(px-x),float(py-y))>36.f)cancelled=true;
  if(cancelled)return false;
  px=x;py=y;return true;
 }
 void clear(){active=false;cancelled=false;}
};
