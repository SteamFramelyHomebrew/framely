#pragma once
#include <algorithm>
#include <chrono>
// Only compositor alpha animates; CEF can reuse its cached texture throughout.
struct LauncherFade {
 bool reduced=false,waiting=false;float from=0,to=0;int duration=0;
 std::chrono::steady_clock::time_point started{};
 float value(std::chrono::steady_clock::time_point now)const{
  if(waiting)return 0;
  if(reduced||!duration)return to;
  float t=std::clamp(std::chrono::duration<float,std::milli>(now-started).count()/duration,0.f,1.f);
  float eased=to>from?1-(1-t)*(1-t)*(1-t):t*t*(3-2*t);
  return from+(to-from)*eased;
 }
 void open(std::chrono::steady_clock::time_point now){from=0;to=1;duration=320;started=now;waiting=true;}
 void ready(std::chrono::steady_clock::time_point now){if(waiting){waiting=false;started=now;}}
 void close(std::chrono::steady_clock::time_point now){from=value(now);waiting=false;to=0;duration=240;started=now;}
 void reset(){waiting=false;from=to=0;duration=0;}
};
