#pragma once
#include <algorithm>
#include <chrono>
#include <cmath>
#include <optional>

enum class SystemRadialDirection {Center,Up,UpRight,Right,DownRight,Down,DownLeft,Left,UpLeft};
enum class SystemRadialEvent {Idle,Begin,Open,Change,Cancel,Commit};

struct SystemRadial {
 using Clock=std::chrono::steady_clock;
 bool active=false,open=false;
 int hand=-1;
 Clock::time_point began{};
 SystemRadialDirection direction=SystemRadialDirection::Center;
 static constexpr auto hold=std::chrono::seconds(1);
 static constexpr float deadzone=.09f;
 float progress(Clock::time_point now) const {
  return active?std::clamp(std::chrono::duration<float>(now-began).count(),0.f,1.f):0.f;
 }
 SystemRadialEvent update(int input_hand,bool valid,bool down,float right,float up,Clock::time_point now,std::optional<SystemRadialDirection> hover=std::nullopt) {
  if(!active) {
   if(!valid||!down)return SystemRadialEvent::Idle;
   active=true;open=false;hand=input_hand;began=now;direction=SystemRadialDirection::Center;
   return SystemRadialEvent::Begin;
  }
  if(input_hand!=hand)return SystemRadialEvent::Idle;
  if(!valid) {active=false;open=false;hand=-1;return SystemRadialEvent::Cancel;}
  if(!down) {
   const bool commit=open||now-began>=hold;
   active=false;open=false;hand=-1;
   return commit?SystemRadialEvent::Commit:SystemRadialEvent::Cancel;
  }
  if(!open&&now-began>=hold){open=true;return SystemRadialEvent::Open;}
  if(open) {
   auto next=hover.value_or(select(right,up));
   if(next!=direction){direction=next;return SystemRadialEvent::Change;}
  }
  return SystemRadialEvent::Idle;
 }
 static SystemRadialDirection select(float right,float up) {
  if(right*right+up*up<deadzone*deadzone)return SystemRadialDirection::Center;
  constexpr float sector=.78539816339f;
  int index=(int)std::lround(std::atan2(right,up)/sector);
  if(index<0)index+=8;
  switch(index%8) {
   case 0:return SystemRadialDirection::Up;
   case 1:return SystemRadialDirection::UpRight;
   case 2:return SystemRadialDirection::Right;
   case 3:return SystemRadialDirection::DownRight;
   case 4:return SystemRadialDirection::Down;
   case 5:return SystemRadialDirection::DownLeft;
   case 6:return SystemRadialDirection::Left;
   default:return SystemRadialDirection::UpLeft;
  }
 }
 // Match the four visible 124x82 slots and 112px center in the 720px overlay.
 // A ray inside the painted button always selects it, including its border.
 static std::optional<SystemRadialDirection> hit(float x,float y) {
  const float dx=x-360.f,dy=y-360.f;
  if(dx*dx+dy*dy<=56.f*56.f)return SystemRadialDirection::Center;
  if(std::abs(dx)<=62.f&&std::abs(dy+177.f)<=41.f)return SystemRadialDirection::Up;
  if(std::abs(dx-177.f)<=62.f&&std::abs(dy)<=41.f)return SystemRadialDirection::Right;
  if(std::abs(dx)<=62.f&&std::abs(dy-177.f)<=41.f)return SystemRadialDirection::Down;
  if(std::abs(dx+177.f)<=62.f&&std::abs(dy)<=41.f)return SystemRadialDirection::Left;
  return std::nullopt;
 }
};
