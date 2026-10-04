#pragma once
#include <chrono>
#include <cstdint>

// Global and owning-overlay queues contain the same character events.
constexpr bool keyboard_edit_source(uint64_t source,uint64_t target){return source&&source==target;}

// Keyboard dismissal can leave a queued modal-cancel event on the menu.
struct KeyboardMenuGuard {
 using Clock=std::chrono::steady_clock;
 bool active=false;
 Clock::time_point until{};
 void begin(bool menu){active=menu;until={};}
 void end(Clock::time_point now){if(active)until=now+std::chrono::milliseconds(350);active=false;}
 bool holds(Clock::time_point now)const{return active||now<until;}
};
