#include "entry_hold.h"
#include "paint_buffer.h"
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
 assert(hold.progress(start+milliseconds(199))==0);
 assert(hold.tick(start+milliseconds(200),true,true)==A::Idle);
 assert(hold.progress(start+milliseconds(200))>0&&hold.progress(start+milliseconds(200))<.01f);
 assert(std::abs(hold.progress(start+milliseconds(400))-.5f)<.001f);
 assert(hold.tick(start+milliseconds(599),true,true)==A::Idle);
 assert(hold.progress(start+milliseconds(599))<1);
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
 // A release with no tracked device must not leave an old progress ring held.
 hold.event(true,1,1,true,true,start);
 assert(hold.event(false,~0u,1,true,true,start+milliseconds(80))==A::Launcher);
 assert(!hold.pressed&&hold.progress(start+milliseconds(100))==0);
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
 // A short click with a late release is judged by event time, not dequeue time.
 const auto received=start+seconds(1);
 hold.event(true,1,1,true,true,entry_event_time(received,1.f),received);
 assert(hold.tick(received,true,true)==A::Idle);
 assert(hold.progress(received+milliseconds(199))==0);
 assert(hold.event(false,1,1,true,true,entry_event_time(received,.92f),received)==A::Launcher);
 // A missing overlay release cannot leave a delayed quick-panel activation.
 hold.event(true,1,1,true,true,start);
 assert(hold.released(true,true)==A::Launcher);
 assert(hold.tick(start+seconds(1),true,true)==A::Idle);
 // A fresh down after a lost release starts a new gesture, never the old timer.
 hold.event(true,1,1,true,true,start);
 hold.event(true,1,1,true,true,start+seconds(1));
 assert(hold.progress(start+seconds(1)+milliseconds(80))==0);
 assert(hold.event(false,1,1,true,true,start+seconds(1)+milliseconds(80))==A::Launcher);
 // A real long hold remains a single long action, including delayed dequeue.
 hold.event(true,1,1,true,true,entry_event_time(received,1.f),received);
 assert(hold.event(false,1,1,true,true,entry_event_time(received,.3f),received)==A::QuickPanel);
 assert(entry_event_time(received,-1.f)==received);
 assert(entry_event_time(received,NAN)==received);
 // The progress follows the rounded square's border, leaving the logo intact.
 std::vector<uint8_t> pixels(128*128*4,23),original=pixels;
 entry_progress_border(pixels,0);assert(pixels==original);
 entry_progress_border(pixels,.5f);assert(pixels!=original);
 for(int y=0;y<128;y++)for(int x=0;x<128;x++){
  int i=(y*128+x)*4;assert(pixels[i+3]==original[i+3]);
  if(std::abs(x-63.5f)<58&&std::abs(y-63.5f)<58)for(int c=0;c<4;c++)assert(pixels[i+c]==original[i+c]);
 }
 const int top=(2*128+64)*4;
 assert(pixels[top]==147&&pixels[top+1]==197&&pixels[top+2]==237);
 assert(pixels[(64*128+125)*4]>original[(64*128+125)*4]);
 assert(pixels[(4*128+123)*4]>original[(4*128+123)*4]); // rounded corner
 assert(pixels[(5*128+64)*4]==original[(5*128+64)*4]); // no circular ring
 struct Rect{int x,y,width,height;};std::vector<uint8_t> upload;
 merge_paint(upload,pixels.data(),128,128,0,static_cast<const Rect*>(nullptr),true);
 assert(upload[((127-2)*128+64)*4]==pixels[top]);
 assert(pixels[top]>pixels[(2*128+63)*4]);
 auto partial=pixels;entry_progress_border(pixels,1);assert(pixels!=partial);
 std::cout<<"entry_hold: passed\n";
}
