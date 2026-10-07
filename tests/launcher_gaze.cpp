#include "launcher_gaze.h"
#include <cassert>
#include <limits>
int main(){
 vr::VREyeTrackingData_t e{};vr::VROverlayIntersectionParams_t ray{};assert(!launcher_gaze_ray(e,ray));
 e.bActive=e.bValid=e.bTracked=true;e.vGazeOrigin={{1,2,3}};e.vGazeTarget={{1,2,1}};assert(launcher_gaze_ray(e,ray));assert(ray.vDirection.v[2]==-1&&ray.vSource.v[0]==1);
 e.vGazeTarget=e.vGazeOrigin;assert(!launcher_gaze_ray(e,ray));e.vGazeTarget.v[0]=std::numeric_limits<float>::quiet_NaN();assert(!launcher_gaze_ray(e,ray));
 LauncherGazePress p;bool connected[2]{true,true},buttons[2]{true,false};
 assert(p.update(true,connected,buttons)==LauncherGazePress::None); // entry held
 buttons[0]=false;assert(p.update(true,connected,buttons)==LauncherGazePress::None);
 buttons[0]=true;assert(p.update(true,connected,buttons)==LauncherGazePress::Down);
 buttons[1]=true;assert(p.update(true,connected,buttons)==LauncherGazePress::None);
 buttons[0]=false;assert(p.update(true,connected,buttons)==LauncherGazePress::Up);
 assert(p.update(true,connected,buttons)==LauncherGazePress::None); // overlapping button doesn't re-click
 buttons[1]=false;p.update(true,connected,buttons);buttons[1]=true;assert(p.update(true,connected,buttons)==LauncherGazePress::Down);
 connected[1]=false;assert(p.update(true,connected,buttons)==LauncherGazePress::Cancel);
 connected[1]=true;assert(p.update(true,connected,buttons)==LauncherGazePress::None);
 buttons[1]=false;p.update(true,connected,buttons);buttons[0]=true;assert(p.update(true,connected,buttons)==LauncherGazePress::Down);
 assert(p.update(false,connected,buttons)==LauncherGazePress::Cancel);
 assert(p.update(true,connected,buttons)==LauncherGazePress::None);
 // A laser aimed at the Dock entry owns this press even if gaze previously
 // focused a launcher icon. Moving away while held must not become gaze-down.
 for(int hand=0;hand<2;hand++){
  LauncherGazePress dock;bool online[2]{true,true},pressed[2]{false,false};
  dock.update(true,online,pressed);pressed[hand]=true;
  assert(dock.update(false,online,pressed)==LauncherGazePress::None);
  assert(dock.update(true,online,pressed)==LauncherGazePress::None);
  pressed[hand]=false;assert(dock.update(true,online,pressed)==LauncherGazePress::None);
  pressed[hand]=true;assert(dock.update(true,online,pressed)==LauncherGazePress::Down);
  // Dock mouse-down cancels an already-held gaze target instead of releasing
  // it (release would click). Same-frame down/up remains disarmed while held.
  assert(dock.cancel()==LauncherGazePress::Cancel);
  assert(!dock.ready&&!dock.held&&dock.owner==-1);
  assert(dock.update(true,online,pressed)==LauncherGazePress::None);
  pressed[hand]=false;assert(dock.update(true,online,pressed)==LauncherGazePress::None);
  pressed[hand]=true;assert(dock.update(true,online,pressed)==LauncherGazePress::Down);
  pressed[hand]=false;assert(dock.update(true,online,pressed)==LauncherGazePress::Up);
 }
}
