#include "menu_geometry.h"
#include "menu_input.h"
#include "overlay_placement.h"
#include <cassert>
#include <cstdio>
int main(){
 // Tracking must update between expensive probes, including scaled OpenVR frames.
 for(float yaw:{-.7f,0.f,.8f})for(float tilt:{-.6f,0.f,.5f}){
  float cy=std::cos(yaw),sy=std::sin(yaw),ct=std::cos(tilt),st=std::sin(tilt);
  vr::HmdMatrix34_t before{{{1,0,0,.2f},{0,1,0,.4f},{0,0,1,-1.f}}};
  vr::HmdMatrix34_t after{{{cy,sy*st,sy*ct,.6f},{0,ct,-st,.8f},{-sy,cy*st,cy*ct,-.7f}}};
  auto scaled=after;for(int r=0;r<3;r++)for(int c=0;c<3;c++)scaled.m[r][c]*=.369f;
  vr::HmdMatrix34_t frame{};assert(dock_rigid_frame(scaled,frame));
  auto pose=before;pose.m[0][3]+=.7f;pose.m[1][3]+=.3f;
  auto moved=dock_follow_pose(pose,before,frame);
  for(int r=0;r<3;r++){
   assert(std::abs(moved.m[r][3]-after.m[r][3]-.7f*after.m[r][0]-.3f*after.m[r][1])<1e-6f);
   for(int c=0;c<3;c++)assert(std::abs(moved.m[r][c]-after.m[r][c])<1e-6f);
  }
  auto restored=dock_follow_pose(moved,frame,before);
  for(int r=0;r<3;r++)for(int c=0;c<4;c++)assert(std::abs(restored.m[r][c]-pose.m[r][c])<1e-6f);
 }
 {vr::HmdMatrix34_t bad{},frame{};assert(!dock_rigid_frame(bad,frame));}
 // Captured from Frame: the Dock row lies horizontally while its normals are pitched.
 {vr::HmdVector3_t p{{.280766f,.551717f,-.954413f}},q{{.270991f,.551716f,-.957210f}},r{{.201858f,.551716f,-.974077f}},center{},axis{};float radius=0;
 assert(dock_row_circle(p,q,r,center,axis,radius));assert(std::abs(radius-1.0318f)<.005f);assert(std::abs(axis.v[1])>.999f);}
 // Reproduce Steam's pitched fan: normals have a constant axial component.
 for(float yaw:{-.7f,0.f,.8f})for(float tilt:{-.6f,0.f,.5f})for(float pitch:{-.7f,0.f}){
  float cy=std::cos(yaw),sy=std::sin(yaw),ct=std::cos(tilt),st=std::sin(tilt),R=1.03f;
  vr::HmdVector3_t t{{cy,0,-sy}},up{{sy*st,ct,cy*st}},n{{sy*ct,-st,cy*ct}};
  auto point=[&](float angle){vr::HmdVector3_t p{};for(int r=0;r<3;r++)p.v[r]=.1f-r*.2f-R*(n.v[r]*std::cos(angle)-t.v[r]*std::sin(angle));return p;};
  auto p=point(.3f),q=point(.18f),r=point(.06f);vr::HmdVector3_t center{},axis{};float radius=0;
  assert(dock_row_circle(p,q,r,center,axis,radius));assert(std::abs(radius-R)<.00005f);
  for(int j=0;j<3;j++)assert(std::abs(center.v[j]-(.1f-j*.2f))<.00005f);
  vr::HmdMatrix34_t button{};for(int j=0;j<3;j++){button.m[j][0]=t.v[j];button.m[j][1]=up.v[j];button.m[j][2]=n.v[j];button.m[j][3]=.1f-j*.2f-R*n.v[j];}
  MenuPlacement menu{};assert(dock_menu_placement(button,.068f,R,pitch,menu));
  float h=.034f,rt=R-h*std::sin(pitch),a=h/R;
  for(int j=0;j<3;j++){
   float circle=menu.transform.m[j][3]+menu.transform.m[j][2]*rt-menu.transform.m[j][1]*(.224f+.012f+h*std::cos(pitch));
   assert(std::abs(circle-(.1f-j*.2f))<1e-6);
   float edge=menu.transform.m[j][3]+menu.transform.m[j][0]*rt*std::sin(.16f/rt)+menu.transform.m[j][2]*rt*(1-std::cos(.16f/rt))-up.v[j]*.224f;
   float expected=.1f-j*.2f-rt*(n.v[j]*std::cos(a)-t.v[j]*std::sin(a))+up.v[j]*(h*std::cos(pitch)+.012f);
   assert(std::abs(edge-expected)<1e-6);
  }
 }
 for(float yaw:{-.7f,0.f,.8f})for(float pitch:{-.9f,0.f,.6f}){
  float c=std::cos(yaw),s=std::sin(yaw),cp=std::cos(pitch),sp=std::sin(pitch);
  vr::HmdMatrix34_t pose{{{c,s*sp,s*cp,.2f},{0,cp,-sp,.6f},{-s,c*sp,c*cp,-1.f}}};auto initial=pose;float radius=1.35f;
  advance_dock_curve(pose,radius,.045f);
  for(int r=0;r<3;r++)assert(std::abs(pose.m[r][3]+radius*pose.m[r][2]-initial.m[r][3]-radius*initial.m[r][2])<1e-6);
 }

 {vr::HmdMatrix34_t surface{{{.369f,0,0,.1f},{0,.28267f,.237189f,.55f},{0,-.237189f,.28267f,-1.f}}};vr::HmdVector3_t origin{};
 assert(dock_probe_origin(surface,origin));float distance=0,front=0;
 for(int r=0;r<3;r++){float d=origin.v[r]-surface.m[r][3];distance+=d*d;front+=d*surface.m[r][2];}
 assert(std::abs(distance-1.f)<1e-5f);assert(front>0);surface={};assert(!dock_probe_origin(surface,origin));}
for(float pitch:{-.8f,0.f,.7f})for(float yaw:{-.7f,0.f,.6f}){
 vr::HmdMatrix34_t button{};button.m[0][0]=std::cos(yaw);button.m[1][0]=0;button.m[2][0]=-std::sin(yaw);button.m[0][1]=std::sin(yaw)*std::sin(pitch);button.m[1][1]=std::cos(pitch);button.m[2][1]=std::cos(yaw)*std::sin(pitch);button.m[0][3]=.7f;button.m[1][3]=.8f;button.m[2][3]=-1.8f;MenuPlacement popup{};assert(menu_placement(button,.068f,popup));auto& m=popup.transform;assert(m.m[0][1]==0&&m.m[1][1]==1&&m.m[2][1]==0);float determinant=m.m[0][0]*m.m[2][2]-m.m[2][0]*m.m[0][2];assert(std::abs(determinant-1)<1e-5);assert(popup.width==.32f);
 for(int r=0;r<3;r++){float bottom_right=m.m[r][3]+m.m[r][0]*popup.width*.5f-m.m[r][1]*popup.width*840.f/600.f*.5f;float dock_top_right=button.m[r][3]+(button.m[r][0]+button.m[r][1])*.034f;assert(std::abs(bottom_right-dock_top_right-(r==1?.012f:0.f))<1e-6);}}
 for(float radius:{.6f,1.3f,2.f})for(float pitch:{-.7f,-.3f,0.f}){
  vr::VROverlayIntersectionResults_t a{},b{};float theta=.12f;
  a.vPoint={{0,radius*std::sin(pitch),-radius*std::cos(pitch)}};a.vNormal={{0,-std::sin(pitch),std::cos(pitch)}};
  b.vPoint={{-radius*std::sin(theta),radius*std::cos(theta)*std::sin(pitch),-radius*std::cos(theta)*std::cos(pitch)}};b.vNormal={{std::sin(theta),-std::cos(theta)*std::sin(pitch),std::cos(theta)*std::cos(pitch)}};
  assert(std::abs(dock_surface_radius(a,b)-radius)<.0001f);
  vr::HmdMatrix34_t button{{{1,0,0,0},{0,1,0,0},{0,0,1,-radius}}};
  vr::HmdMatrix34_t main{{{1,0,0,0},{0,std::cos(pitch),-std::sin(pitch),0},{0,std::sin(pitch),std::cos(pitch),0}}};
  MenuPlacement menu{};assert(menu_placement(button,.068f,menu,radius,&main));float angle=.16f/radius;
  for(int r=0;r<3;r++){
   assert(std::abs(menu.transform.m[r][1]-main.m[r][1])<1e-6);
   // The right edge tangent continues the Dock; a standalone centered arc fails this.
   float edge_tangent=menu.transform.m[r][0]*std::cos(angle)+menu.transform.m[r][2]*std::sin(angle);
   assert(std::abs(edge_tangent-button.m[r][0])<1e-6);
   float shared_center=menu.transform.m[r][3]+menu.transform.m[r][2]*radius-menu.transform.m[r][1]*(.224f+.012f);
   float endpoint_normal=-menu.transform.m[r][0]*std::sin(angle)+menu.transform.m[r][2]*std::cos(angle);
   float endpoint_center=button.m[r][3]+button.m[r][1]*.034f+button.m[r][0]*radius*std::sin(.034f/radius)+button.m[r][2]*radius*(1-std::cos(.034f/radius))+endpoint_normal*radius;
   assert(std::abs(shared_center-endpoint_center)<1e-5);
   float edge=menu.transform.m[r][3]+menu.transform.m[r][0]*radius*std::sin(angle)+menu.transform.m[r][2]*radius*(1-std::cos(angle))-menu.transform.m[r][1]*.224f;
   float expected=button.m[r][3]+button.m[r][1]*.034f+button.m[r][0]*radius*std::sin(.034f/radius)+button.m[r][2]*radius*(1-std::cos(.034f/radius))+main.m[r][1]*.012f;
   assert(std::abs(edge-expected)<1e-5);
  }
 }
 vr::HmdMatrix34_t invalid{};MenuPlacement out{};assert(!menu_placement(invalid,.06f,out));
 MenuButtonInput input;auto t=std::chrono::steady_clock::now();assert(!input.event(false,true,true,t));assert(!input.event(true,true,true,t));assert(input.event(false,true,true,t));input.opened(t);assert(!input.can_cancel(t+std::chrono::milliseconds(299)));assert(input.can_cancel(t+std::chrono::milliseconds(300)));assert(!input.event(false,true,true,t));assert(!input.event(true,true,true,t));assert(!input.event(false,true,true,t+std::chrono::milliseconds(20)));assert(!input.event(true,true,true,t));assert(input.event(false,true,true,t+std::chrono::milliseconds(200)));assert(!input.event(true,false,true,t));assert(!input.event(false,false,true,t));
 struct FakeOverlay{int widths=0,transforms=0;vr::EVROverlayError SetOverlayWidthInMeters(vr::VROverlayHandle_t,float){++widths;return vr::VROverlayError_None;}vr::EVROverlayError SetOverlayTransformAbsolute(vr::VROverlayHandle_t,vr::ETrackingUniverseOrigin,const vr::HmdMatrix34_t*){++transforms;return vr::VROverlayError_None;}} overlay;
 OverlayPlacement placement;vr::HmdMatrix34_t m{};m.m[0][0]=m.m[1][1]=m.m[2][2]=1;assert(placement.apply(&overlay,1,m,.068f));for(int i=0;i<120;i++){auto jitter=m;jitter.m[0][3]=.0001f*std::sin(float(i));assert(!placement.apply(&overlay,1,jitter,.068f));}assert(overlay.widths==1&&overlay.transforms==1);m.m[0][3]=.01f;assert(placement.apply(&overlay,1,m,.068f));assert(overlay.widths==1&&overlay.transforms==2);assert(placement.apply(&overlay,1,m,.08f));assert(overlay.widths==2&&overlay.transforms==3);
 std::puts("PASS Dock-bound popup, matched click activation, cancel guard and stable geometry submission");}
