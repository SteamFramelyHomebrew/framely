#include "launcher_input.h"
#include <cassert>
#include <cmath>
int main(){
 LauncherStick stick;assert(!stick.update(1,0,true));assert(!stick.update(0,0,true));assert(stick.update(.8f,.75f,true)==1);assert(!stick.update(.4f,.9f,true));assert(!stick.update(0,0,true));assert(stick.update(-.7f,-.95f,true)==-2);assert(!stick.update(0,0,false));assert(!stick.update(0,1,true));assert(!stick.update(0,.2f,true));assert(stick.update(0,1,true)==2);assert(!stick.update(NAN,0,true));
vr::RenderModel_ComponentState_t joystick_rest{},joystick_x{},joystick_y{},joystick_current{};
 joystick_rest.mTrackingToComponentRenderModel={{{1,0,0,0},{0,1,0,0},{0,0,1,0}}};joystick_x=joystick_y=joystick_current=joystick_rest;
 auto turn=[](vr::RenderModel_ComponentState_t& value,float x,float y){auto& m=value.mTrackingToComponentRenderModel;float cx=std::cos(x),sx=std::sin(x),cy=std::cos(y),sy=std::sin(y);m={{{cy,sy*sx,sy*cx,0},{0,cx,-sx,0},{-sy,cy*sx,cy*cx,0}}};};
 turn(joystick_x,0,.35f);turn(joystick_y,.35f,0);turn(joystick_current,-.28f,.14f);float joystick_dx=0,joystick_dy=0;assert(launcher_stick_axes(joystick_rest,joystick_x,joystick_y,joystick_current,joystick_dx,joystick_dy));assert(std::abs(joystick_dx-.4f)<.03f);assert(std::abs(joystick_dy+.8f)<.03f);assert(!launcher_stick_axes(joystick_rest,joystick_rest,joystick_rest,joystick_current,joystick_dx,joystick_dy));
 auto now=std::chrono::steady_clock::now();EntryTrigger t;EntryPointer pointer;
 pointer.move(1,320,64);assert(pointer.hit(1));assert(!pointer.hit(2));pointer.move(2,20,64);assert(!pointer.hit(2));pointer.move(2,320,64);assert(pointer.hit(2));pointer.leave();assert(!pointer.hit(2));
 vr::HmdMatrix34_t tracked{{{0,0,1,1},{0,1,0,2},{-1,0,0,3}}},tip{{{1,0,0,.1f},{0,1,0,.2f},{0,0,1,.3f}}};auto composed=entry_tip_pose(tracked,tip);assert(std::abs(composed.m[0][3]-1.3f)<1e-6f);assert(std::abs(composed.m[1][3]-2.2f)<1e-6f);assert(std::abs(composed.m[2][3]-2.9f)<1e-6f);assert(composed.m[2][0]==-1);
 vr::RenderModel_ComponentState_t rest{},full{},partial{};rest.mTrackingToComponentRenderModel={{{1,0,0,0},{0,1,0,0},{0,0,1,0}}};full=partial=rest;
 auto rotate=[](vr::RenderModel_ComponentState_t& state,float angle){auto& m=state.mTrackingToComponentRenderModel;m.m[0][0]=m.m[2][2]=std::cos(angle);m.m[0][2]=std::sin(angle);m.m[2][0]=-std::sin(angle);};
 rotate(full,.22f);rotate(partial,.121f);float pull=0;assert(entry_trigger_pull(rest,full,partial,pull));assert(std::abs(pull-.55f)<.001f);assert(!(partial.uProperties&vr::VRComponentProperty_IsPressed));EntryAnalogTrigger analog;assert(analog.update(pull));assert(analog.update(.47f));assert(!analog.update(.44f));assert(!analog.update(.49f));assert(analog.update(.51f));assert(!entry_trigger_pull(rest,rest,partial,pull));assert(!analog.update(NAN));
 EntryAnalogTrigger sampledPull;EntryTrigger sampledClick;int activations=0,frame=0;
 for(float sample:{0.f,.213733f,.378395f,.513260f,.718133f,.501608f,.345020f,.188268f,.0107336f,0.f}){bool opened=sampledClick.update(true,sampledPull.update(sample),true,now+std::chrono::milliseconds(25*frame));activations+=opened;if(frame==3)assert(opened);if(frame!=3)assert(!opened);frame++;}assert(activations==1);
 EntryTrigger left,upper;assert(!left.update(true,false,true,now));assert(!upper.update(true,false,true,now));assert(left.update(true,true,true,now));assert(!left.update(true,true,true,now));assert(!left.update(true,false,true,now));assert(upper.update(true,true,true,now));assert(!upper.update(true,false,true,now));
 // Dragging a held trigger into the target does not count as a new press.
 assert(!t.update(true,false,false,now));assert(!t.update(true,true,false,now));assert(!t.update(true,true,true,now));assert(!t.update(true,false,true,now));
 assert(t.update(true,true,true,now));assert(!t.update(true,true,true,now));assert(!t.update(true,false,true,now));
 // Disconnect/reconnect while held cannot produce a stale activation.
 now+=std::chrono::milliseconds(200);assert(!t.update(false,false,true,now));assert(!t.update(true,true,true,now));assert(!t.update(true,false,true,now));assert(t.update(true,true,true,now));assert(!t.update(true,false,true,now));
 assert(!t.update(true,true,true,now));assert(!t.update(true,false,true,now));now+=std::chrono::milliseconds(200);assert(t.update(true,true,true,now));
 // Reopening after a plugin quick page must rearm the modal guard each time.
 LauncherDismissGuard guard;guard.opened(now);
 assert(!guard.allows(now,true));assert(!guard.allows(now+std::chrono::milliseconds(400),true));
 assert(guard.allows(now+std::chrono::milliseconds(400),false));assert(guard.allows(now+std::chrono::milliseconds(500),true));
 now+=std::chrono::seconds(2);guard.opened(now);
 assert(!guard.allows(now+std::chrono::milliseconds(20),false));
 assert(!guard.allows(now+std::chrono::milliseconds(500),false,1.f));
 assert(guard.allows(now+std::chrono::milliseconds(500),false,.1f));
 // Releasing the opening trigger rearms input once, even without mouse events.
 guard.opened(now);guard.observe(true);guard.observe(false);
 assert(guard.allows(now+std::chrono::milliseconds(700),true));
 // Dock dragging and delayed cancel keep the launcher; later background
 // clicks become dismissible again, and reopening clears the prior gesture.
 LauncherDockGuard dock;assert(!dock.holds(now));dock.observe(true,true,now);
 assert(dock.holds(now+std::chrono::milliseconds(60)));
 dock.observe(false,true,now+std::chrono::seconds(2));
 assert(dock.holds(now+std::chrono::seconds(2)));
 dock.observe(false,false,now+std::chrono::seconds(3));
 assert(dock.holds(now+std::chrono::milliseconds(3200)));
 assert(!dock.holds(now+std::chrono::milliseconds(3250)));
 dock.clear();assert(!dock.holds(now+std::chrono::seconds(3)));

 // A Dock depth gesture consumes stick input; a still-deflected stick must
 // not navigate after the guard expires until it has returned to neutral.
 LauncherStick depthStick;assert(!depthStick.update(0,0,true));
 assert(!depthStick.update(0,.9f,false));assert(!depthStick.update(0,.9f,true));
 assert(!depthStick.update(0,0,true));assert(depthStick.update(0,.9f,true)==2);

}
