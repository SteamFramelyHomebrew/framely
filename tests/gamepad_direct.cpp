#include <signal.h>
static volatile sig_atomic_t stopped=0;
#include "gamepad_direct.h"
#include <cassert>
#include <sstream>
int main(){
 std::istringstream info("[slot 0]\nVID=0x28de\nPID=0x11e0\n[slot 1]\nVID=0x0001\nPID=0xf001\n[slot 2]\nVID=0x045e\nPID=0x028e\n");auto rows=read_steam_pad_info(info);
 input_id id{BUS_USB,0x28de,0x11ff,1};
 assert(direct_slot(id,"Microsoft X-Box 360 pad 0",rows)==0);
 assert(direct_slot(id,"Microsoft X-Box 360 pad 1",rows)==-1);
 assert(direct_slot(id,"Microsoft X-Box 360 pad 2",rows)==2);
 assert(direct_slot(id,"Microsoft X-Box 360 pad 0 other",rows)==-1);
 id.vendor=0x045e;assert(direct_slot(id,"Microsoft X-Box 360 pad 0",rows)==-1);
 FramelyDirectControl c{200,1,1};assert(framely_direct_enabled(c,100));assert(framely_direct_rumble(c,100));
 c.rumble=0;assert(framely_direct_enabled(c,100));assert(!framely_direct_rumble(c,100));
 c.rumble=1;c.enabled=0;assert(!framely_direct_rumble(c,100));
 c.enabled=1;assert(!framely_direct_enabled(c,200));assert(!framely_direct_rumble(c,201));
}
