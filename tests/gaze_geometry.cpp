#include "gaze_geometry.h"
#include <cassert>
#include <limits>
int main(){
 const double correction[6]{.93,.01,.045,-.02,1.06,-.035};
 vr::HmdVector3_t target{{-.7f,1.8f,-1.4f}};
 for(int n=0;n<100;n++){
  float yaw=(n-50)*.012f,pitch=std::sin(n*.15f)*.2f,roll=std::cos(n*.12f)*.15f;
  float cy=std::cos(yaw),sy=std::sin(yaw),cp=std::cos(pitch),sp=std::sin(pitch),cr=std::cos(roll),sr=std::sin(roll);
  vr::HmdMatrix34_t head{{{cy*cr+sy*sp*sr,-cy*sr+sy*sp*cr,sy*cp,std::sin(n*.1f)*.2f},{cp*sr,cp*cr,-sp,1.65f},{-sy*cr+cy*sp*sr,sy*sr+cy*sp*cr,cy*cp,std::cos(n*.1f)*.15f}}};
  vr::HmdVector3_t origin{{head.m[0][3]+.015f,head.m[1][3]-.02f,head.m[2][3]}};GazeAngles expected{};
  assert(gaze_angles_to_point(head,origin,target,expected));
  double ex=expected.yaw-correction[2],ey=expected.pitch-correction[5],det=correction[0]*correction[4]-correction[1]*correction[3];
  GazeAngles raw{float((correction[4]*ex-correction[1]*ey)/det),float((-correction[3]*ex+correction[0]*ey)/det)};
  auto ray=gaze_world_ray(head,origin,gaze_correct(raw,correction));GazeAngles round{};assert(gaze_head_angles(head,ray.vDirection,round));assert(std::hypot(round.yaw-expected.yaw,round.pitch-expected.pitch)<1e-5f);
  float distance=0;for(int i=0;i<3;i++){assert(ray.vSource.v[i]==origin.v[i]);distance+=std::pow(target.v[i]-origin.v[i],2);}distance=std::sqrt(distance);
  for(int i=0;i<3;i++)assert(std::abs(origin.v[i]+distance*ray.vDirection.v[i]-target.v[i])<1e-5f);
 }
 // Runtime UV inversion, including curvature coupling and a displaced seed.
 auto curved=[](GazeAngles a,float& u,float& v){u=.5f+std::sin(a.yaw);v=.5f+a.pitch+.18f*a.yaw*a.yaw;return u>=0&&u<=1&&v>=0&&v<=1;};
 for(float u:{.08f,.5f,.92f})for(float v:{.08f,.5f,.92f}){GazeAngles out;assert(gaze_target_inverse({},u,v,curved,out));float x,y;assert(curved(out,x,y));assert(std::hypot(x-u,y-v)<.0003f);}
 GazeAngles out;assert(!gaze_target_inverse({},.3f,.3f,[](GazeAngles,float&,float&){return false;},out));assert(!gaze_target_inverse({},.3f,.3f,[](GazeAngles,float& u,float& v){u=v=.5f;return true;},out));
 vr::HmdMatrix34_t invalid{};invalid.m[0][0]=std::numeric_limits<float>::quiet_NaN();assert(!gaze_head_angles(invalid,target,out));
}
