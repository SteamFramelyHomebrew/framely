#pragma once
#include <chrono>
#include <cmath>
#include "vendor/openvr/openvr.h"
inline bool launcher_gaze_ray(const vr::VREyeTrackingData_t& eye,vr::VROverlayIntersectionParams_t& ray){
 if(!eye.bActive||!eye.bValid||!eye.bTracked)return false;
 double length=0;for(int i=0;i<3;i++){if(!std::isfinite(eye.vGazeOrigin.v[i])||!std::isfinite(eye.vGazeTarget.v[i]))return false;ray.vSource.v[i]=eye.vGazeOrigin.v[i];ray.vDirection.v[i]=eye.vGazeTarget.v[i]-eye.vGazeOrigin.v[i];length+=double(ray.vDirection.v[i])*ray.vDirection.v[i];}
 if(!std::isfinite(length)||length<1e-8)return false;
 for(float& v:ray.vDirection.v)v/=float(std::sqrt(length));ray.eOrigin=vr::TrackingUniverseStanding;return true;
}
// Release-to-arm prevents the entry press, overlapping buttons, disconnects,
// or a mode switch while held from becoming a fresh gaze click.
struct LauncherGazePress {
 bool ready=false,held=false;int owner=-1;
 enum Edge {None,Down,Up,Cancel};
 Edge update(bool enabled,const bool (&connected)[2],const bool (&buttons)[2]){
  if(!enabled|| (held&&(owner<0||!connected[owner]))){auto edge=held?Cancel:None;ready=false;held=false;owner=-1;return edge;}
  if(held){if(buttons[owner])return None;held=false;owner=-1;ready=!buttons[0]&&!buttons[1];return Up;}
  if(!buttons[0]&&!buttons[1]){ready=true;return None;}
  if(!ready)return None;ready=false;for(int hand=0;hand<2;hand++)if(connected[hand]&&buttons[hand]){owner=hand;held=true;return Down;}return None;
 }
};
