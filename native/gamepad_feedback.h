#pragma once
#include <linux/input.h>
#include <array>
#include <algorithm>
#include <cerrno>
#include <cstdint>
// Kernel-owned effect IDs. Keep upload/erase acknowledgements independent of
// focus so Android's InputReader cannot block while the game is in background.
class GamepadFeedback {
 struct Slot { ff_effect effect{}; bool uploaded=false,playing=false; uint64_t start=0,end=0,delay=0,period=0; };
 std::array<Slot,32> slots{};
public:
 static constexpr int capacity=32;
 int upload(const ff_effect& effect) {
  if(effect.id<0||effect.id>=capacity||effect.type!=FF_RUMBLE)return -EINVAL;
  auto& s=slots[effect.id];s.effect=effect;s.uploaded=true;return 0;
 }
 int erase(int id){if(id<0||id>=capacity)return -EINVAL;slots[id]={};return 0;}
 void play(int id,int count,uint64_t now){if(id<0||id>=capacity)return;auto& s=slots[id];s.playing=count>0&&s.uploaded;if(!s.playing)return;s.start=now;s.delay=std::min<unsigned>(s.effect.replay.delay,5000);s.period=s.delay+std::min<unsigned>(s.effect.replay.length,5000);s.end=now+std::min<uint64_t>(10000,s.period*std::min(count,32));}
 void stop(){for(auto& s:slots)s.playing=false;}
 std::array<uint16_t,2> value(uint64_t now,bool allowed){
  std::array<uint16_t,2> v{};if(!allowed){stop();return v;}
  for(auto& s:slots){if(!s.playing)continue;if(now>=s.end){s.playing=false;continue;}if(now<s.start||!s.period||(now-s.start)%s.period<s.delay)continue;v[0]=std::max<uint16_t>(v[0],std::min<unsigned>(s.effect.u.rumble.strong_magnitude,32768));v[1]=std::max<uint16_t>(v[1],std::min<unsigned>(s.effect.u.rumble.weak_magnitude,32768));}return v;
 }
};
