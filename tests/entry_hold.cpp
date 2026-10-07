#include "entry_hold.h"
#include <cassert>
#include <iostream>

int main(){
 using A=EntryHold::Action;
 using namespace std::chrono;
 const auto start=EntryHold::Clock::time_point{}+seconds(10);
 // All mouse buttons and runtime-supplied input (without a tracked device).
 for(unsigned device:{1u,2u,~0u})for(unsigned button:{1u,2u,4u}){
  EntryHold hold;
  assert(hold.event(true,device,button,true,true,start)==A::Idle);
  assert(hold.event(false,device,button,true,true,start+milliseconds(100))==A::Launcher);
  assert(hold.event(false,device,button,true,true,start+milliseconds(110))==A::Idle);
 }
 EntryHold hold;
 hold.event(true,1,1,true,true,start);
 assert(hold.progress(start+milliseconds(99))==0);
 assert(std::abs(hold.progress(start+milliseconds(100))-1.f/6)<.001f);
 assert(std::abs(hold.progress(start+milliseconds(300))-.5f)<.001f);
 assert(hold.tick(start+milliseconds(599),true,true)==A::Idle);
 assert(hold.tick(start+milliseconds(600),true,true)==A::QuickPanel);
 assert(hold.tick(start+milliseconds(900),true,true)==A::Idle);
 assert(hold.event(false,1,1,true,true,start+seconds(1))==A::Idle);
 assert(hold.progress(start+seconds(1))==0);
 // Crossing the threshold between frames still resolves exclusively as a hold.
 hold.event(true,1,1,true,true,start);
 assert(hold.event(false,1,1,true,true,start+milliseconds(600))==A::QuickPanel);
 hold.event(true,1,1,true,true,start);
 assert(hold.event(false,2,1,true,true,start+milliseconds(100))==A::Idle);
 assert(hold.event(false,1,2,true,true,start+milliseconds(110))==A::Idle);
 assert(hold.event(false,1,1,true,true,start+milliseconds(120))==A::Launcher);
 // Small jitter does not restart the timer; a sustained leave or loss cancels.
 hold.event(true,1,1,true,true,start);
 hold.tick(start+milliseconds(480),true,true);
 assert(hold.tick(start+milliseconds(550),true,false)==A::Idle);
 assert(hold.tick(start+milliseconds(610),true,true)==A::QuickPanel);
 hold.cancel();hold.event(true,1,1,true,true,start);
 hold.tick(start+milliseconds(151),true,false);
 assert(!hold.pressed);
 assert(hold.event(false,1,1,true,true,start+seconds(1))==A::Idle);
 for(bool inside:{false,true}){
  hold.event(true,1,1,true,true,start);
  assert(hold.tick(start+milliseconds(600),false,inside)==A::Idle);
  assert(!hold.pressed);
 }
 hold.event(true,1,1,true,false,start);
 assert(hold.tick(start+seconds(1),true,true)==A::Idle);
 hold.event(true,1,1,true,true,start);
 assert(hold.event(false,1,1,true,false,start+milliseconds(100))==A::Idle);
 // The ring never alters the central logo or texture alpha; only its perimeter.
 std::vector<uint8_t> pixels(128*128*4,23),original=pixels;
 entry_progress_ring(pixels,0);assert(pixels==original);
 entry_progress_ring(pixels,.5f);
 assert(pixels!=original);
 for(int y=0;y<128;y++)for(int x=0;x<128;x++){
  int i=(y*128+x)*4;
  assert(pixels[i+3]==original[i+3]);
  if(std::hypot(x-63.5f,y-63.5f)<55.f)for(int c=0;c<4;c++)assert(pixels[i+c]==original[i+c]);
 }
 // A white line is blended, not opaque, and is wider than the previous ring.
 const int top=(5*128+64)*4;
 assert(pixels[top]>original[top]&&pixels[top]<255);
 assert(pixels[top]==pixels[top+1]&&pixels[top+1]==pixels[top+2]);
 assert(pixels[(8*128+64)*4]>original[(8*128+64)*4]);
 auto partial=pixels;entry_progress_ring(pixels,1);
 assert(pixels!=partial);
 std::cout<<"entry_hold: passed\n";
}
