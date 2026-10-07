#pragma once
#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <vector>

inline std::chrono::steady_clock::time_point entry_event_time(std::chrono::steady_clock::time_point received,float age){
 if(!std::isfinite(age)||age<0)return received;
 return received-std::chrono::duration_cast<std::chrono::steady_clock::duration>(std::chrono::duration<float>(age));
}

// Overlay mouse events are authoritative: never synthesize another click from
// controller actions. Keep the initiating device/button paired until release.
struct EntryHold {
 using Clock=std::chrono::steady_clock;
 enum class Action { Idle, Launcher, QuickPanel };
 static constexpr auto quick_panel_delay=std::chrono::milliseconds(600);
 static constexpr auto feedback_delay=std::chrono::milliseconds(100);
 bool pressed=false,fired=false;
 unsigned device=~0u,button=0;
 Clock::time_point started{},armed_since{},last_inside{};
 void cancel(){pressed=false;fired=false;}
 Action tick(Clock::time_point now,bool available,bool inside){
  if(!pressed)return Action::Idle;
  if(!available){cancel();return Action::Idle;}
  if(inside)last_inside=now;
  // Brief pointer jitter is tolerated, but leaving cannot activate a hold.
  if(!inside){if(now-last_inside>std::chrono::milliseconds(150))cancel();return Action::Idle;}
  if(!fired&&now-armed_since>=quick_panel_delay){fired=true;return Action::QuickPanel;}
  return Action::Idle;
 }
 Action event(bool down,unsigned source,unsigned mouse_button,bool available,bool inside,Clock::time_point now,Clock::time_point received={}){
  if(down){
   if(available&&inside&&(!pressed||(source==device&&mouse_button==button))){pressed=true;fired=false;device=source;button=mouse_button;started=now;armed_since=received==Clock::time_point{}?now:received;last_inside=armed_since;}
   return Action::Idle;
  }
  // Steam may omit the device on release; known other devices still cannot release this hold.
  if(!pressed||(source!=device&&source!=~0u&&device!=~0u)||mouse_button!=button)return Action::Idle;
  auto action=Action::Idle;
  if(!fired&&available&&inside)action=now-started>=quick_panel_delay?Action::QuickPanel:Action::Launcher;
  cancel();return action;
 }
 // A confirmed physical release without a matching overlay release must not
 // become a long press. Prefer short-click behavior over inventing its duration.
 Action released(bool available,bool inside){
  const auto action=pressed&&!fired&&available&&inside?Action::Launcher:Action::Idle;
  cancel();return action;
 }
 float progress(Clock::time_point now)const{
  if(!pressed||now-armed_since<feedback_delay)return 0.f;
  const float elapsed=std::chrono::duration<float>(now-armed_since-feedback_delay).count();
  const float remaining=std::chrono::duration<float>(quick_panel_delay-feedback_delay).count();
  // A tiny nonzero value reveals the track at 100ms without jumping to 1/6 full.
  return std::clamp(elapsed/remaining,.001f,1.f);
 }
};

inline void entry_progress_border(std::vector<uint8_t>& pixels,float progress){
 if(progress<=0||pixels.size()!=128*128*4)return;
 constexpr float pi=3.14159265358979323846f,half=61.5f,radius=7.1f;
 constexpr float extent=half-radius,arc=pi*.5f*radius,perimeter=8*extent+4*arc;
 for(int y=0;y<128;y++)for(int x=0;x<128;x++){
  float dx=x-63.5f,dy=y-63.5f,qx=std::abs(dx)-extent,qy=std::abs(dy)-extent;
  const float distance=std::hypot(std::max(qx,0.f),std::max(qy,0.f))+std::min(std::max(qx,qy),0.f)-radius;
  const float coverage=std::clamp(2.f-std::abs(distance),0.f,1.f);
  if(coverage<=0)continue;
  // Arc length along the inset rounded rectangle, starting at top center.
  float along;
  if(dx>=extent&&dy<=-extent)along=extent+(std::atan2(dy+extent,dx-extent)+pi*.5f)*radius;
  else if(dx>=extent&&dy>=extent)along=3*extent+arc+std::atan2(dy-extent,dx-extent)*radius;
  else if(dx<=-extent&&dy>=extent)along=5*extent+2*arc+(std::atan2(dy-extent,dx+extent)-pi*.5f)*radius;
  else if(dx<=-extent&&dy<=-extent)along=7*extent+3*arc+(std::atan2(dy+extent,dx+extent)+pi)*radius;
  else if(dy<-extent)along=dx>=0?dx:perimeter+dx;
  else if(dx>extent)along=2*extent+arc+dy;
  else if(dy>extent)along=4*extent+2*arc-dx;
  else along=6*extent+3*arc-dy;
  const float alpha=coverage*(along<=progress*perimeter?1.f:.12f);
  const int blue[3]={147,197,237};const int i=(y*128+x)*4;
  for(int c=0;c<3;c++)pixels[i+c]=uint8_t(pixels[i+c]+(blue[c]-pixels[i+c])*alpha);
 }
}
