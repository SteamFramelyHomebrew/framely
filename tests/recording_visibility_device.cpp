// Isolated, opt-in wearer diagnostic. Never part of the installed app.
// Usage: helper geometry.json ready-file stop-file
#include "openvr.h"
#include "recording_region.h"
#include <fstream>
#include <thread>
#include <chrono>
#include <filesystem>
#include <iostream>
#include <stdexcept>

namespace {
constexpr int texture_size=512;
constexpr double aspect=4./3.;
constexpr int fovs[]{80,75,70,65,60};
constexpr std::array<unsigned char,3> colors[]{
 {255,48,48},{255,230,32},{32,255,255},{48,255,64},{255,255,255}
};
// Five columns, seven rows; each row's most significant bit is the left pixel.
constexpr unsigned char digits[10][7]{
 {14,17,19,21,25,17,14},{4,12,4,4,4,4,14},
 {14,17,1,2,4,8,31},{30,1,1,14,1,1,30},
 {2,6,10,18,31,2,2},{31,16,16,30,1,1,30},
 {14,16,16,30,17,17,14},{31,1,2,4,8,8,8},
 {14,17,17,14,17,17,14},{14,17,17,15,1,1,14}
};

std::vector<unsigned char> pixels(int fov,const std::array<unsigned char,3>& color){
 std::vector<unsigned char> image(texture_size*texture_size*4);
 auto put=[&](int x,int y){
  auto index=(y*texture_size+x)*4;
  for(int c=0;c<3;c++)image[index+c]=color[c];
  image[index+3]=255;
 };
 constexpr int stroke=3;
 for(int y=0;y<texture_size;y++)for(int x=0;x<texture_size;x++){
  if(x<stroke||x>=texture_size-stroke||y<stroke||y>=texture_size-stroke)put(x,y);
 }
 // Center the FOV number just inside the top edge. Large block digits remain
 // readable without CEF or a font installation; all other pixels are clear.
 // Keep the label shorter than the angular gap to the next rectangle, so
 // an inner horizontal border cannot strike through the outer FOV number.
 constexpr int scale=2,digit_width=5*scale,gap=2*scale;
 constexpr int start_x=(texture_size-(2*digit_width+gap))/2,start_y=5;
 for(int digit=0;digit<2;digit++){
  int value=digit==0?fov/10:fov%10;
  for(int row=0;row<7;row++)for(int column=0;column<5;column++){
   if(!(digits[value][row]&(1<<(4-column))))continue;
   for(int dy=0;dy<scale;dy++)for(int dx=0;dx<scale;dx++)
    put(start_x+digit*(digit_width+gap)+column*scale+dx,start_y+row*scale+dy);
  }
 }
 return image;
}

void check(vr::IVROverlay* api,vr::EVROverlayError error,const char* operation){
 if(error!=vr::VROverlayError_None)
  throw std::runtime_error(std::string(operation)+": "+api->GetOverlayErrorNameFromEnum(error));
}

struct Cleanup {
 vr::IVROverlay* api=nullptr;
 std::vector<vr::VROverlayHandle_t> handles;
 bool initialized=false;
 ~Cleanup(){
  for(auto handle:handles)if(api){
   auto hidden=api->HideOverlay(handle),destroyed=api->DestroyOverlay(handle);
   if(hidden||destroyed)std::cerr<<"Diagnostic overlay cleanup error: "<<hidden<<", "<<destroyed<<"\n";
  }
  if(initialized)vr::VR_Shutdown();
 }
};
}

int main(int argc,char** argv){
 if(argc!=4){std::cerr<<"Usage: recording-visibility-device geometry.json ready-file stop-file\n";return 2;}
 Cleanup cleanup;
 try{
  RegionJson view;std::ifstream input(argv[1]);if(!input)throw std::runtime_error("Cannot open geometry");input>>view;
  const double cx=view.at("centerTanX"),cy=view.at("centerTanY");
  vr::EVRInitError error{};vr::VR_Init(&error,vr::VRApplication_Overlay);
  if(error)throw std::runtime_error(vr::VR_GetVRInitErrorAsEnglishDescription(error));
  cleanup.initialized=true;cleanup.api=vr::VROverlay();
  if(!cleanup.api)throw std::runtime_error("OpenVR overlay interface unavailable");
  auto* api=cleanup.api;
  for(int i=0;i<5;i++){
   RecordingRegionPlane plane;
   const double tangent=std::tan(fovs[i]*std::acos(-1.)/360.);
   if(!recording_region_plane(view,tangent,cx,cy,aspect,plane))throw std::runtime_error("Invalid capture geometry");
   vr::VROverlayHandle_t handle=0;
   const std::string key="framely.recording.visibility.fixture."+std::to_string(fovs[i]);
   check(api,api->CreateOverlay(key.c_str(),("Recording visibility "+std::to_string(fovs[i])).c_str(),&handle),"CreateOverlay");
   cleanup.handles.push_back(handle);
   check(api,api->SetOverlayInputMethod(handle,vr::VROverlayInputMethod_None),"SetOverlayInputMethod");
   check(api,api->SetOverlayFlag(handle,vr::VROverlayFlags_MakeOverlaysInteractiveIfVisible,false),"Disable interactive overlay");
   check(api,api->SetOverlayFlag(handle,vr::VROverlayFlags_HideLaserIntersection,true),"Hide laser intersection");
   check(api,api->SetOverlayFlag(handle,vr::VROverlayFlags_VisibleInDashboard,true),"VisibleInDashboard");
   check(api,api->SetOverlayFlag(handle,vr::VROverlayFlags_SortWithNonSceneOverlays,true),"Foreground sort flag");
   check(api,api->SetOverlaySortOrder(handle,1000+i),"SetOverlaySortOrder");
   check(api,api->SetOverlayWidthInMeters(handle,plane.width),"SetOverlayWidthInMeters");
   check(api,api->SetOverlayTexelAspect(handle,aspect),"SetOverlayTexelAspect");
   check(api,api->SetOverlayTransformTrackedDeviceRelative(handle,vr::k_unTrackedDeviceIndex_Hmd,&plane.transform),"SetOverlayTransformTrackedDeviceRelative");
   auto image=pixels(fovs[i],colors[i]);
   check(api,api->SetOverlayRaw(handle,image.data(),texture_size,texture_size,4),"SetOverlayRaw");
   check(api,api->ShowOverlay(handle),"ShowOverlay");
  }
  std::ofstream ready(argv[2]);if(!ready)throw std::runtime_error("Cannot open ready file");
  ready<<"ready\n";ready.close();if(!ready)throw std::runtime_error("Cannot write ready file");
  std::cout<<"Visibility diagnostic ready: eye="<<view.at("eye")<<", center="<<cx<<","<<cy
   <<", 4:3 FOVs red80 yellow75 cyan70 green65 white60; auto-hide after 90 seconds\n"<<std::flush;
  const auto until=std::chrono::steady_clock::now()+std::chrono::seconds(90);
  while(std::chrono::steady_clock::now()<until&&!std::filesystem::exists(argv[3]))
   std::this_thread::sleep_for(std::chrono::milliseconds(20));
  return 0;
 }catch(const std::exception& error){std::cerr<<"Recording visibility diagnostic: "<<error.what()<<"\n";return 1;}
}
