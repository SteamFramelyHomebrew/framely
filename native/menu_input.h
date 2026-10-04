#pragma once
#include <chrono>
struct MenuButtonInput {
 bool pressed=false;
 std::chrono::steady_clock::time_point last_toggle{},opened_at{};
 bool event(bool down,bool left,bool available,std::chrono::steady_clock::time_point now){
  if(!left)return false;
  if(down){pressed=available;return false;}
  bool matched=pressed;pressed=false;
  if(!available||!matched||now-last_toggle<std::chrono::milliseconds(180))return false;
  last_toggle=now;return true;
 }
 void opened(std::chrono::steady_clock::time_point now){opened_at=now;}
 bool can_cancel(std::chrono::steady_clock::time_point now)const{return now-opened_at>=std::chrono::milliseconds(300);}
};
