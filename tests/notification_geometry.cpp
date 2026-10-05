#include "notification_geometry.h"
#include <cassert>

int main(){
 // A side-offset toast must face the eye, including tilted/rotated tracking frames.
 for(float yaw:{-.7f,0.f,.8f})for(float tilt:{-.6f,0.f,.5f})for(float radius:{.6f,1.3f,2.f}){
  float cy=std::cos(yaw),sy=std::sin(yaw),ct=std::cos(tilt),st=std::sin(tilt);
  vr::HmdMatrix34_t head{{{cy,sy*st,sy*ct,.6f},{0,ct,-st,.8f},{-sy,cy*st,cy*ct,-.7f}}};
  NotificationPlacement toast{};assert(notification_placement(head,radius,toast));
  auto& m=toast.transform;
  vr::HmdVector3_t eye{},right{{m.m[0][0],m.m[1][0],m.m[2][0]}},up{{m.m[0][1],m.m[1][1],m.m[2][1]}};
  float distance=0;
  for(int r=0;r<3;r++){
   eye.v[r]=head.m[r][3]-m.m[r][3];distance+=eye.v[r]*eye.v[r];
   float expected=head.m[r][3]+head.m[r][0]*.24f+head.m[r][1]*.12f-head.m[r][2]*1.f;
   assert(std::abs(expected-m.m[r][3])<1e-6f);
  }
  assert(normalize_dock_vector(eye));auto normal=cross_dock_vector(right,up);
  for(int r=0;r<3;r++){assert(std::abs(eye.v[r]-m.m[r][2])<1e-6f);assert(std::abs(normal.v[r]-m.m[r][2])<1e-6f);}
  assert(toast.radius==radius&&toast.width==.32f);
  // Both ends of the compact cylindrical surface remain front-facing to the eye.
  for(float side:{-1.f,1.f}){
   float angle=side*toast.width*.5f/radius,dot=0;
   for(int r=0;r<3;r++){
    float point=m.m[r][3]+right.v[r]*radius*std::sin(angle)+normal.v[r]*radius*(1-std::cos(angle));
    float front=normal.v[r]*std::cos(angle)-right.v[r]*std::sin(angle);
    dot+=front*(head.m[r][3]-point);
   }
   assert(dot>0);
  }
 }
 NotificationPlacement invalid{};vr::HmdMatrix34_t zero{};assert(!notification_placement(zero,1.3f,invalid));
 vr::HmdMatrix34_t identity{{{1,0,0,0},{0,1,0,0},{0,0,1,0}}};
 assert(!notification_placement(identity,0,invalid));assert(!notification_placement(identity,NAN,invalid));
 // A card painted at the top of the UI must hit at the top, not below it.
 std::vector<uint8_t> card(600*360*4,0);
 for(int y=360-8-150;y<360-8;y++)for(int x=8;x<592;x++)card[(y*600+x)*4+3]=255;
 auto top=notification_bounds(card,600,360,1);
 assert(top.x==8&&top.y==8&&top.width==584&&top.height==150);
 NotificationClick click;auto now=std::chrono::steady_clock::now();int x=110,y=106;
 click.begin(100,100,now);assert(click.hold(x,y,now+std::chrono::milliseconds(80)));
 assert(click.release(x,y,now+std::chrono::milliseconds(180)));assert(x==100&&y==100&&!click.active);
 click.begin(100,100,now);assert(!click.hold(140,100,now));x=101;y=100;assert(!click.release(x,y,now));assert(x==101);
 click.begin(100,100,now);x=y=100;assert(!click.release(x,y,now+std::chrono::seconds(1)));
 click.begin(100,100,now);click.clear();assert(!click.release(x,y,now));
 std::vector<uint8_t> pixels(12*8*4,0);
 assert(!notification_bounds(pixels,12,8,2).visible());
 // RGB left over in a transparent frame must not capture input.
 pixels[0]=255;
 assert(!notification_bounds(pixels,12,8,2).visible());
 pixels[(1*12+2)*4+3]=255;
 pixels[(4*12+9)*4+3]=1;
 auto bounds=notification_bounds(pixels,12,8,2);
 assert(bounds.visible());
 assert(bounds.x==1&&bounds.y==1.5f&&bounds.width==4&&bounds.height==2);
 // Dismissing/expiring the final toast yields an empty frame again.
 pixels[(1*12+2)*4+3]=0;pixels[(4*12+9)*4+3]=0;
 assert(!notification_bounds(pixels,12,8,2).visible());
 pixels.back()=255;
 bounds=notification_bounds(pixels,12,8,2);
 assert(bounds.x==5.5f&&bounds.y==0&&bounds.width==.5f&&bounds.height==.5f);
 assert(!notification_bounds(pixels,12,8,0).visible());
 assert(!notification_bounds({},12,8,2).visible());
}
