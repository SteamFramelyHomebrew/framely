#pragma once
#include <algorithm>
#include <chrono>
// Compositor alpha fades even when a cached CEF texture has not repainted yet.
struct LauncherFade {
 bool reduced=false;float from=1,to=1;int duration=0;
 std::chrono::steady_clock::time_point started{};
 float value(std::chrono::steady_clock::time_point now)const{
  if(reduced||!duration)return to;
  float t=std::clamp(std::chrono::duration<float,std::milli>(now-started).count()/duration,0.f,1.f);
  float eased=t*t*(3-2*t);return from+(to-from)*eased;
 }
 void open(std::chrono::steady_clock::time_point now){from=0;to=1;duration=580;started=now;}
 void close(std::chrono::steady_clock::time_point now){from=value(now);to=0;duration=440;started=now;}
};
