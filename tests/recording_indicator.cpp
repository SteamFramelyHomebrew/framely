#include "openvr.h"
#include "recording_indicator.h"
#include "launcher_input.h"
#include <cassert>
int main(){
 RecordingIndicatorClick click;
 assert(!click.update(true,true));assert(!click.update(false,true));
 assert(!click.update(true,false));assert(!click.update(false,true));
 assert(!click.update(true,true));assert(click.update(false,true));assert(!click.update(false,true));
 assert(!click.update(true,true));assert(!click.update(false,true,false));assert(!click.update(false,true));
 EntryAnalogTrigger trigger;RecordingIndicatorClick analog_click;
 assert(!analog_click.update(trigger.update(0.f),true));
 assert(!analog_click.update(trigger.update(.6f),true));
 assert(!analog_click.update(trigger.update(.46f),true));
 assert(analog_click.update(trigger.update(.44f),true));
 for(auto seconds:{0ULL,59ULL,60ULL,3600ULL,9999999ULL}){
  auto normal=recording_indicator_pixels(seconds,false),hover=recording_indicator_pixels(seconds,true);
  assert(normal.size()==320*80*4&&hover.size()==normal.size());
  assert(normal!=hover);assert(normal[3]==0);
  auto red=(40*320+32)*4;assert(normal[red]>200&&normal[red+1]<100);
  auto stop=(40*320+280)*4;assert(normal[stop]==245&&normal[stop+3]==255);
 }
}
