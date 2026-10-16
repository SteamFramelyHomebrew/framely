#pragma once
#include <array>
#include <vector>
#include <string>
#include <cstdio>
#include <cmath>
#include <algorithm>

struct RecordingIndicatorClick {
 bool held=true,armed=false;
 bool update(bool down,bool inside,bool valid=true){
  if(!valid){held=true;armed=false;return false;}
  if(down&&!held)armed=inside;
  bool clicked=!down&&held&&armed&&inside;
  if(!down)armed=false;
  held=down;return clicked;
 }
};

inline std::vector<unsigned char> recording_indicator_pixels(uint64_t seconds,bool hover){
 constexpr int w=320,h=80;std::vector<unsigned char> pixels(w*h*4);
 auto point=[&](int x,int y,int r,int g,int b,int a=255){if(x<0||x>=w||y<0||y>=h)return;auto i=(y*w+x)*4;pixels[i]=r;pixels[i+1]=g;pixels[i+2]=b;pixels[i+3]=a;};
 for(int y=0;y<h;y++)for(int x=0;x<w;x++){
  int dx=std::max(12-x,std::max(x-(w-13),0)),dy=std::max(12-y,std::max(y-(h-13),0));
  if(dx*dx+dy*dy<=144)point(x,y,hover?65:27,hover?45:27,hover?45:29,235);
  if((x-32)*(x-32)+(y-40)*(y-40)<=11*11)point(x,y,242,61,75);
 }
 static constexpr unsigned digits[]={0x7b6f,0x2492,0x73e7,0x73cf,0x5bc9,0x79cf,0x79ef,0x7249,0x7bef,0x7bcf};
 char text[32];seconds=std::min<uint64_t>(seconds,359999);std::snprintf(text,sizeof text,"%02llu:%02llu:%02llu",(unsigned long long)(seconds/3600),(unsigned long long)(seconds/60%60),(unsigned long long)(seconds%60));
 int x=70;for(char c:std::string(text)){
  unsigned glyph=c==':'?0x0410:digits[c-'0'];
  for(int row=0;row<5;row++)for(int col=0;col<3;col++)if(glyph&(1u<<(14-row*3-col)))for(int dy=0;dy<4;dy++)for(int dx=0;dx<4;dx++)point(x+col*4+dx,30+row*4+dy,245,245,245);
  x+=20;
 }
 for(int y=29;y<51;y++)for(int x=272;x<294;x++)point(x,y,245,245,245);
 return pixels;
}

class RecordingIndicator {
 vr::IVROverlay* overlay=nullptr;vr::VROverlayHandle_t handle=0;
 std::string id;uint64_t rendered=~uint64_t(0);bool last_hover=false;std::array<RecordingIndicatorClick,2> clicks{};std::array<bool,2> hover{};
public:
 void destroy(){if(handle&&overlay)overlay->DestroyOverlay(handle);handle=0;id.clear();cancel_input();}
 bool active()const{return handle&&!id.empty();}
 void cancel_input(){clicks={};hover={};}
 bool hovered(int hand)const{return hand>=0&&hand<2&&hover[hand];}
 std::string pointer(int hand,const vr::HmdMatrix34_t& pose,bool valid,bool down){
  if(!active()||hand<0||hand>=2)return {};
  vr::VROverlayIntersectionParams_t ray{};ray.eOrigin=vr::TrackingUniverseStanding;
  for(int k=0;k<3;k++){ray.vSource.v[k]=pose.m[k][3];ray.vDirection.v[k]=-pose.m[k][2];}
  vr::VROverlayIntersectionResults_t hit{};
  hover[hand]=valid&&overlay->ComputeOverlayIntersection(handle,&ray,&hit)&&hit.vUVs.v[0]>=0&&hit.vUVs.v[0]<=1&&hit.vUVs.v[1]>=0&&hit.vUVs.v[1]<=1;
  return clicks[hand].update(down,hover[hand],valid)?id:std::string{};
 }
 void tick(vr::IVROverlay* overlays,const std::string& next,uint64_t seconds){
  overlay=overlays;if(next.empty()){destroy();return;}
  if(!handle){
   if(overlay->CreateOverlay("framely.recording","Framely recording — click to stop",&handle))return;
   overlay->SetOverlayWidthInMeters(handle,.32f);
   vr::HmdMatrix34_t transform={{{1,0,0,.50f},{0,1,0,.35f},{0,0,1,-1.f}}};
   overlay->SetOverlayTransformTrackedDeviceRelative(handle,vr::k_unTrackedDeviceIndex_Hmd,&transform);
   // Stay passive: never take input focus from a game or activate global lasers.
   overlay->SetOverlayInputMethod(handle,vr::VROverlayInputMethod_None);
   overlay->SetOverlayFlag(handle,vr::VROverlayFlags_MakeOverlaysInteractiveIfVisible,false);
   overlay->SetOverlayFlag(handle,vr::VROverlayFlags_HideLaserIntersection,true);
   overlay->SetOverlayFlag(handle,vr::VROverlayFlags_VisibleInDashboard,true);
   rendered=~uint64_t(0);
  }
  if(id!=next){id=next;clicks={};hover={};}
  bool highlighted=hover[0]||hover[1];
  if(seconds!=rendered||highlighted!=last_hover){auto pixels=recording_indicator_pixels(seconds,highlighted);if(!overlay->SetOverlayRaw(handle,pixels.data(),320,80,4)){overlay->ShowOverlay(handle);rendered=seconds;last_hover=highlighted;}}
 }
};
