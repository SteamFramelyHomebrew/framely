// Isolated device fixture: visible fiducials, never part of the installed app.
#include "openvr.h"
#include "recording_region.h"
#include <fstream>
#include <thread>
#include <chrono>
#include <filesystem>
#include <iostream>
int main(int argc,char** argv){
 if(argc!=4)return 2;RegionJson view;std::ifstream(argv[1])>>view;
 vr::EVRInitError error{};vr::VR_Init(&error,vr::VRApplication_Overlay);if(error)return 3;
 auto* overlays=vr::VROverlay();vr::VROverlayHandle_t handle=0;
 if(overlays->CreateOverlay("framely.region.alignment.fixture","Recording alignment fixture",&handle))return 4;
 RecordingRegionPlane plane;if(!recording_region_plane(view,view.at("tanHalfHorizontal"),view.at("centerTanX"),view.at("centerTanY"),view.at("aspect"),plane))return 5;
 const int w=view.at("width"),h=view.at("height"),texture=512;std::vector<unsigned char> pixels(texture*texture*4);
 const int positions[5][2]={{16,16},{w-16,16},{16,h-16},{w-16,h-16},{w/2,h/2}};
 const int colors[5][3]={{255,0,0},{0,255,0},{0,0,255},{255,0,255},{255,255,255}};
 for(int y=0;y<texture;y++)for(int x=0;x<texture;x++)for(int p=0;p<5;p++){
  double ox=(x+.5)*w/texture,oy=(y+.5)*h/texture;
  if(ox<positions[p][0]-16||ox>=positions[p][0]+16||oy<positions[p][1]-16||oy>=positions[p][1]+16)continue;
  auto i=(y*texture+x)*4;for(int c=0;c<3;c++)pixels[i+c]=colors[p][c];pixels[i+3]=255;
 }
 overlays->SetOverlayInputMethod(handle,vr::VROverlayInputMethod_None);
 overlays->SetOverlayFlag(handle,vr::VROverlayFlags_MakeOverlaysInteractiveIfVisible,false);
 overlays->SetOverlayFlag(handle,vr::VROverlayFlags_VisibleInDashboard,true);
 recording_region_foreground(overlays,handle);
 overlays->SetOverlayWidthInMeters(handle,plane.width);
 overlays->SetOverlayTexelAspect(handle,view.at("aspect"));
 overlays->SetOverlayTransformTrackedDeviceRelative(handle,vr::k_unTrackedDeviceIndex_Hmd,&plane.transform);
 overlays->SetOverlayRaw(handle,pixels.data(),texture,texture,4);overlays->ShowOverlay(handle);
 std::ofstream(argv[2])<<"ready";
 auto until=std::chrono::steady_clock::now()+std::chrono::seconds(40);
 while(std::chrono::steady_clock::now()<until&&!std::filesystem::exists(argv[3])){
  std::this_thread::sleep_for(std::chrono::milliseconds(5));
 }
 overlays->DestroyOverlay(handle);vr::VR_Shutdown();return 0;
}
