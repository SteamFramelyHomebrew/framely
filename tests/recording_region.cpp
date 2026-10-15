#include "openvr.h"
#include "recording_region.h"
#include <cassert>
#include <iostream>
int main(){
 for(double roll:{-.2,0.,.2})for(double yaw:{-.3,0.,.3})for(double aspect:{4./3,16./9,9./16,1.}){
  double c=cos(roll),s=sin(roll),cy=cos(yaw),sy=sin(yaw);
  RegionJson view={{"captureToHead",{{c*cy,-s, c*sy,-.032},{s*cy,c,s*sy,.003},{-sy,0,cy,.01}}}};
  const double tan=.8,cx=.15,center_y=-.08;
  RecordingRegionPlane plane;assert(recording_region_plane(view,tan,cx,center_y,aspect,plane));
  // Undo the exported head basis: each overlay corner must be exactly the
  // tangent ray consumed at the corresponding output-LUT corner.
  for(double u:{0.,.25,1.})for(double v:{0.,.75,1.}){
   double world[3];for(int i=0;i<3;i++)world[i]=plane.transform.m[i][3]+plane.transform.m[i][0]*(u-.5)*plane.width+plane.transform.m[i][1]*(.5-v)*plane.width/aspect-view["captureToHead"][i][3].get<double>();
   double ray[3]{};for(int j=0;j<3;j++)for(int i=0;i<3;i++)ray[j]+=view["captureToHead"][i][j].get<double>()*world[i]/2.;
   assert(std::abs(ray[0]-(cx+(2*u-1)*tan))<1e-6);
   assert(std::abs(ray[1]-(-center_y+(1-2*v)*tan/aspect))<1e-6);
   assert(std::abs(ray[2]+1.)<1e-6);
  }
 }
 auto pixels=recording_region_pixels(100,80);assert(pixels[(40*100+50)*4+3]==0);assert(pixels[3]>0&&pixels[3]<100);
 auto colored=recording_region_pixels(100,80,{12,34,56});assert(colored[0]==12&&colored[1]==34&&colored[2]==56&&colored[3]==72);
 RecordingRegionPlane bad;assert(!recording_region_plane(RegionJson::object(),1,0,0,1,bad));
 std::cout<<"PASS: recording border projection matches capture rays, including roll/yaw/eye offset and aspect ratios\n";
}
