#pragma once
#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <vector>

// Overlay mouse events are authoritative: never synthesize another click from
// controller actions. Keep the initiating device/button paired until release.
struct EntryHold {
 using Clock=std::chrono::steady_clock;
 enum class Action { Idle, Launcher, QuickPanel };
 static constexpr auto duration=std::chrono::milliseconds(600);
 bool pressed=false,fired=false;
 unsigned device=~0u,button=0;
 Clock::time_point started{},last_inside{};
 void cancel(){pressed=false;fired=false;}
 Action tick(Clock::time_point now,bool available,bool inside){
  if(!pressed)return Action::Idle;
  if(!available){cancel();return Action::Idle;}
  if(inside)last_inside=now;
  // Brief pointer jitter is tolerated, but leaving cannot activate a hold.
  if(!inside){if(now-last_inside>std::chrono::milliseconds(150))cancel();return Action::Idle;}
  if(!fired&&now-started>=duration){fired=true;return Action::QuickPanel;}
  return Action::Idle;
 }
 Action event(bool down,unsigned source,unsigned mouse_button,bool available,bool inside,Clock::time_point now){
  if(down){
   if(!pressed&&available&&inside){pressed=true;fired=false;device=source;button=mouse_button;started=last_inside=now;}
   return Action::Idle;
  }
  if(!pressed||source!=device||mouse_button!=button)return Action::Idle;
  auto action=tick(now,available,inside);
  if(action==Action::Idle&&pressed&&!fired&&available&&inside)action=Action::Launcher;
  cancel();return action;
 }
 float progress(Clock::time_point now)const{
  return pressed?std::clamp(std::chrono::duration<float>(now-started).count()/.6f,0.f,1.f):0.f;
 }
};

inline void entry_progress_ring(std::vector<uint8_t>& pixels,float progress){
 if(progress<=0||pixels.size()!=128*128*4)return;
 constexpr float pi=3.14159265358979323846f;
 for(int y=0;y<128;y++)for(int x=0;x<128;x++){
  float dx=x-63.5f,dy=y-63.5f;
  float coverage=std::clamp(2.f-std::abs(std::hypot(dx,dy)-58.f),0.f,1.f);
  if(coverage<=0)continue;
  float angle=std::atan2(dx,-dy);if(angle<0)angle+=2*pi;
  bool filled=angle<=progress*2*pi;
  const int track[3]={66,73,81},blue[3]={147,197,237};
  int i=(y*128+x)*4;
  for(int c=0;c<3;c++)pixels[i+c]=uint8_t(pixels[i+c]+((filled?blue[c]:track[c])-pixels[i+c])*coverage);
 }
}
