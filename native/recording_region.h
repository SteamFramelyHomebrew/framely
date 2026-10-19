#pragma once
#include <cmath>
#include <algorithm>
#include <vector>
#include <array>
#include <chrono>
#include <utility>
#include "vendor/json.hpp"
using RegionJson=nlohmann::json;
struct RecordingRegionPlane {vr::HmdMatrix34_t transform{};float width=0;};
inline void recording_region_foreground(vr::IVROverlay* overlays,vr::VROverlayHandle_t handle){
 overlays->SetOverlayFlag(handle,vr::VROverlayFlags_SortWithNonSceneOverlays,true);
 overlays->SetOverlaySortOrder(handle,1000);
}
inline bool recording_region_plane(const RegionJson& view,double tan,double cx,double cy,double aspect,RecordingRegionPlane& plane,double distance=2.){
 try{
  if(!std::isfinite(tan)||tan<=0||!std::isfinite(cx)||!std::isfinite(cy)||!std::isfinite(aspect)||aspect<=0||!std::isfinite(distance)||distance<=0)return false;
  const auto& basis=view.at("captureToHead");
  for(int i=0;i<3;i++){
   double translation=basis.at(i).at(3).get<double>();
   for(int j=0;j<3;j++){
    double value=basis.at(i).at(j).get<double>();if(!std::isfinite(value))return false;
    plane.transform.m[i][j]=value;
    translation+=distance*value*(j==0?cx:j==1?-cy:-1.);
   }
   if(!std::isfinite(translation))return false;plane.transform.m[i][3]=translation;
  }
  plane.width=2*tan*distance;return std::isfinite(plane.width)&&plane.width>0;
 }catch(...){return false;}
}
inline std::array<unsigned char,3> recording_region_color(){
 static std::array<unsigned char,3> cached{0,255,255};static auto checked=std::chrono::steady_clock::time_point{};
 auto now=std::chrono::steady_clock::now();if(now-checked<std::chrono::seconds(1))return cached;checked=now;
 std::array<unsigned char,3> color{0,255,255};auto* settings=vr::VRSettings();
 if(!settings)return cached;
 const char* keys[]={vr::k_pch_CollisionBounds_ColorGammaR_Int32,vr::k_pch_CollisionBounds_ColorGammaG_Int32,vr::k_pch_CollisionBounds_ColorGammaB_Int32};
 for(int i=0;i<3;i++){vr::EVRSettingsError error{};auto value=settings->GetInt32(vr::k_pch_CollisionBounds_Section,keys[i],&error);if(error!=vr::VRSettingsError_None||value<0||value>255)return cached;color[i]=static_cast<unsigned char>(value);}
 cached=color;return cached;
}
inline std::vector<unsigned char> recording_region_pixels(int w,int h,const std::array<unsigned char,3>& color={0,255,255}){
 std::vector<unsigned char> pixels(w*h*4);
 for(int y=0;y<h;y++)for(int x=0;x<w;x++){
  bool edge=x<2||x>=w-2||y<2||y>=h-2;
  if(edge&&((x<2||x>=w-2?y:x)/14)%2==0){auto i=(y*w+x)*4;for(int c=0;c<3;c++)pixels[i+c]=color[c];pixels[i+3]=72;}
 }
 return pixels;
}
class RecordingRegionOverlay {
 vr::IVROverlay* overlays=nullptr;vr::VROverlayHandle_t handle=0;std::string last,last_pixels;
 std::string key;
public:
 explicit RecordingRegionOverlay(std::string overlay_key="framely.recording.region"):key(std::move(overlay_key)){}
 void destroy(){if(handle&&overlays)overlays->DestroyOverlay(handle);handle=0;last.clear();last_pixels.clear();}
 void tick(vr::IVROverlay* api,const RegionJson& state,double distance=2.,const RegionJson* live_view=nullptr){
  overlays=api;
  if(!state.value("visible",false)){destroy();return;}
  auto color=recording_region_color();auto serialized=state.dump()+std::string(color.begin(),color.end())+std::to_string(distance)+(live_view?live_view->dump():"");if(serialized==last)return;
  try{
   const auto& view=live_view?*live_view:state.at("view");double aspect=view.at("aspect");
   RecordingRegionPlane plane;if(!recording_region_plane(view,view.at("tanHalfHorizontal"),view.at("centerTanX"),view.at("centerTanY"),aspect,plane,distance)){destroy();return;}
   if(!handle){
    if(overlays->CreateOverlay(key.c_str(),"Framely capture region",&handle))return;
    overlays->SetOverlayInputMethod(handle,vr::VROverlayInputMethod_None);
    overlays->SetOverlayFlag(handle,vr::VROverlayFlags_MakeOverlaysInteractiveIfVisible,false);
    overlays->SetOverlayFlag(handle,vr::VROverlayFlags_HideLaserIntersection,true);
    overlays->SetOverlayFlag(handle,vr::VROverlayFlags_VisibleInDashboard,true);
    recording_region_foreground(overlays,handle);
   }
   int width=state.at("settings").at("width"),height=state.at("settings").at("height");
   if(width<160||height<160||width>4096||height>4096||std::abs(double(width)/height-aspect)>1e-6){destroy();return;}
   // The INNER edge is the capture boundary. Keep the guide outside it so
   // it does not cover pixels inside the intended recorded image.
   // Bound raw-overlay traffic independently of recording resolution.
   // Square texels are explicitly stretched to the exact capture aspect.
   width=height=512;plane.width*=512.f/508.f;
   auto error=overlays->SetOverlayWidthInMeters(handle,plane.width);
   if(!error)error=overlays->SetOverlayTexelAspect(handle,aspect);
   if(!error)error=overlays->SetOverlayTransformTrackedDeviceRelative(handle,vr::k_unTrackedDeviceIndex_Hmd,&plane.transform);
   const auto pixel_key=std::string(color.begin(),color.end());
   if(!error&&(last_pixels.empty()||pixel_key!=last_pixels)){
    auto pixels=recording_region_pixels(width,height,color);
    error=overlays->SetOverlayRaw(handle,pixels.data(),width,height,4);
    if(!error)last_pixels=pixel_key;
   }
   if(error){destroy();return;}overlays->ShowOverlay(handle);last=serialized;
  }catch(...){destroy();}
 }
};
