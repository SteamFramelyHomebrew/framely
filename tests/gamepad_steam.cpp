#include "gamepad_steam.h"
#include "gamepad_feedback.h"
#include <cassert>
#include <sstream>
int main(){
 std::istringstream info("[slot 0]\nname=Steam Frame Controllers\nVID=0x28de\nPID=0x11e0\n[slot 1]\nVID=0x0001\nPID=0xf001\n[slot 2]\nVID=0x045e\nPID=0x028e\n");
 auto rows=read_steam_pad_info(info);assert(allowed_steam_slot(rows,0));assert(!allowed_steam_slot(rows,1));assert(allowed_steam_slot(rows,2));assert(!allowed_steam_slot(rows,3));
 std::istringstream missing("[slot 0]\nVID=0x28de\n");assert(!allowed_steam_slot(read_steam_pad_info(missing),0));
 std::istringstream invalid("[slot 99]\nVID=0x28de\nPID=0x11e0\n");assert(read_steam_pad_info(invalid).empty());
 std::array<bool,15> b{};std::array<int,6>a{};for(int k=0;k<4;k++){b[k]=true;auto s=steam_pad_state(b,a);assert(s[k]==1);b[k]=false;}
 b[4]=b[6]=b[7]=b[8]=b[9]=b[10]=true;b[11]=b[14]=true;a={-32768,32767,-15000,8000,32767,16384};auto s=steam_pad_state(b,a);for(int k=4;k<10;k++)assert(s[k]);assert(s[10]==-32768&&s[11]==32767&&s[12]==-15000&&s[13]==8000);assert(s[14]==255&&s[15]==127);assert(s[16]==1&&s[17]==-1);
 GamepadFeedback f;ff_effect e{};e.id=0;e.type=FF_RUMBLE;e.replay.delay=50;e.replay.length=100;e.u.rumble.strong_magnitude=60000;e.u.rumble.weak_magnitude=12000;assert(f.upload(e)==0);f.play(0,1,1000);assert(f.value(1049,true)[0]==0);assert(f.value(1050,true)[0]==32768);assert(f.value(1149,true)[1]==12000);assert(f.value(1150,true)[0]==0);
 f.play(0,2,1000);assert(f.value(1150,true)[0]==0);assert(f.value(1200,true)[0]==32768);assert(f.value(1300,true)[0]==0);
 f.play(0,1,2000);f.value(2050,false);assert(f.value(2051,true)[0]==0);f.play(0,1,3000);f.erase(0);assert(f.value(3050,true)[0]==0);
 e.id=32;assert(f.upload(e)==-EINVAL);e.id=0;e.type=FF_PERIODIC;assert(f.upload(e)==-EINVAL);e.type=FF_RUMBLE;e.replay.delay=0;e.replay.length=65535;f.upload(e);f.play(0,100,0);assert(f.value(9999,true)[0]>0&&f.value(10000,true)[0]==0);
}
