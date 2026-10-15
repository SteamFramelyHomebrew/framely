#include "recording_region_depth.h"
#include <cassert>
#include <iostream>
#include <limits>
using namespace recording_region_depth_detail;
static vr::HmdMatrix34_t pose(double yaw=0,double pitch=0,double roll=0,Point t={0,0,0}){
 const double cy=cos(yaw),sy=sin(yaw),cp=cos(pitch),sp=sin(pitch),cr=cos(roll),sr=sin(roll);
 vr::HmdMatrix34_t m{{
  {float(cy*cr+sy*sp*sr),float(-cy*sr+sy*sp*cr),float(sy*cp),float(t[0])},
  {float(cp*sr),float(cp*cr),float(-sp),float(t[1])},
  {float(-sy*cr+cy*sp*sr),float(sy*sr+cy*sp*cr),float(cy*cp),float(t[2])}}};return m;
}
static vr::HmdMatrix34_t multiply(const vr::HmdMatrix34_t& a,const vr::HmdMatrix34_t& b){
 vr::HmdMatrix34_t m{};for(int i=0;i<3;i++){
  for(int j=0;j<3;j++)for(int k=0;k<3;k++)m.m[i][j]+=a.m[i][k]*b.m[k][j];
  m.m[i][3]=a.m[i][3];for(int k=0;k<3;k++)m.m[i][3]+=a.m[i][k]*b.m[k][3];
 }return m;
}
static void near(double a,double b,double tolerance=1e-5){assert(std::abs(a-b)<tolerance);}
int main(){
 for(double ipd:{0.055,0.064,0.075})for(auto eye:{vr::Eye_Left,vr::Eye_Right}){
  const auto left=pose(0,0,0,{-ipd/2,0,0.012}),right=pose(0,0,0,{ipd/2,0,0.012});
  auto capture=pose(0.12,-0.08,0.04,{0,0,0});const auto before=capture;
  assert(recording_region_refresh_eye_origin(eye,left,right,capture));
  for(int i=0;i<3;i++)for(int j=0;j<3;j++)assert(capture.m[i][j]==before.m[i][j]);
  const auto selected=eye==vr::Eye_Left?left:right;
  for(int i=0;i<3;i++)near(capture.m[i][3],selected.m[i][3]);
  for(double depth:{0.5,1.,2.,4.})for(double yaw:{-0.35,0.,0.35})for(double pitch:{-0.25,0.,0.25}){
   const auto hmd=pose(0.4,-0.1,0.05,{0.5,1.6,-0.2});
   const double cx=0.16,cy=-0.09;
   const auto origin=add(translation(hmd),rotate(hmd,translation(capture)));
   const auto direction=rotate(hmd,rotate(capture,{cx,-cy,-1.}));
   const Point intersection={origin[0]+depth*direction[0],origin[1]+depth*direction[1],origin[2]+depth*direction[2]};
   auto window=multiply(hmd,pose(yaw,pitch,0.07));for(int i=0;i<3;i++)window.m[i][3]=float(intersection[i]);
   const auto result=recording_region_window_depth(hmd,window,capture,cx,cy);
   assert(result);near(*result,depth);
   const vr::HmdVector3_t hit{{float(intersection[0]),float(intersection[1]),float(intersection[2])}};
   const auto hit_depth=recording_region_hit_depth(hmd,capture,cx,cy,hit);
   assert(hit_depth);near(*hit_depth,depth);
  }
  // Any finite plane distance preserves selected-eye ray bounds. The other
  // eye's shift is baseline/D, symmetric for left and right recordings.
  for(double depth:{0.5,1.,2.,4.})for(double tx:{-0.7,0.,0.7}){
   auto parallel=pose(0,0,0,{0,0,0});assert(recording_region_refresh_eye_origin(eye,left,right,parallel));
   const auto origin=translation(parallel),other=translation(eye==vr::Eye_Left?right:left);
   const Point point={origin[0]+depth*tx,origin[1],origin[2]-depth};
   near((point[0]-origin[0])/(origin[2]-point[2]),tx);
   const double baseline=origin[0]-other[0];
   const double actual_other=(point[0]-other[0])/(other[2]-point[2]);
   near(actual_other,tx+baseline/depth);
   const double old_other=tx+baseline/2.;
   near(actual_other-old_other,baseline*(1./depth-0.5));
   if(depth!=2.)assert(std::abs(actual_other-old_other)>0.001);
  }
 }
 // Changing IPD refreshes a stale exported origin without altering ray basis.
 auto stale=pose(0,0,0,{0.032,0,0});
 assert(recording_region_refresh_eye_origin(vr::Eye_Right,pose(0,0,0,{-0.0375,0,0}),pose(0,0,0,{0.0375,0,0}),stale));
 near(stale.m[0][3],0.0375);
 const auto identity=pose(),left=pose(0,0,0,{-0.032,0,0}),right=pose(0,0,0,{0.032,0,0});
 auto unchanged=identity;
 assert(!recording_region_refresh_eye_origin(vr::Eye_Right,pose(0,0,0,{-0.1,0,0}),pose(0,0,0,{0.1,0,0}),unchanged));
 assert(!recording_region_refresh_eye_origin(vr::Eye_Right,right,left,unchanged));
 assert(unchanged.m[0][3]==0);
 auto invalid=identity;invalid.m[0][0]=std::numeric_limits<float>::quiet_NaN();
 assert(!recording_region_refresh_eye_origin(vr::Eye_Right,invalid,right,unchanged));
 assert(!recording_region_window_depth(invalid,pose(0,0,0,{0,0,-2}),identity,0,0));
 assert(!recording_region_window_depth(vr::HmdMatrix34_t{},pose(0,0,0,{0,0,-2}),identity,0,0));
 assert(!recording_region_window_depth(identity,pose(acos(-1.)/2,0,0,{0,0,-2}),identity,0,0));
 for(double z:{-0.1,-8.,1.})assert(!recording_region_window_depth(identity,pose(0,0,0,{0,0,z}),identity,0,0));
 assert(!recording_region_window_depth(identity,pose(0,0,0,{0,0,-2}),identity,std::numeric_limits<double>::infinity(),0));
 // Eye Z matters: head-relative depth alone would return 2, but D=2.012.
 const auto zdepth=recording_region_window_depth(identity,pose(0,0,0,{0,0,-2}),right,0,0);
 assert(zdepth);near(*zdepth,2.0); // This fixture's right eye has Z=0.
 const auto elevated=pose(0,0,0,{0.032,0,0.012});
 const auto exact=recording_region_window_depth(identity,pose(0,0,0,{0,0,-2}),elevated,0,0);
 assert(exact);near(*exact,2.012);
 for(double depth:{0.25,0.5,1.,2.,4.,6.}){
  const vr::HmdVector3_t hit{{0,0,float(-depth)}};
  const auto result=recording_region_hit_depth(identity,identity,0,0,hit);assert(result);near(*result,depth);
 }
 for(const auto& hit:std::array<vr::HmdVector3_t,4>{{{{0.02f,0,-2}},{{0,0,2}},{{0,0,-8}},{{std::numeric_limits<float>::quiet_NaN(),0,-2}}}})
  assert(!recording_region_hit_depth(identity,identity,0,0,hit));
 assert(!recording_region_hit_depth(invalid,identity,0,0,vr::HmdVector3_t{{0,0,-2}}));
 std::cout<<"PASS: current eye origin, exact center-ray UI depth, both-eye parallax and invalid-pose guards\n";
}
