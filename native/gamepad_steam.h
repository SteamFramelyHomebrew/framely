#pragma once
#include <dlfcn.h>
#include <cstdint>
#include <linux/input.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/ioctl.h>
#include <array>
#include <vector>
#include <fstream>
#include <string>
#include <algorithm>
#include <cstdlib>
#include <cstring>
#include <cstdio>
#include <cmath>
#include <limits>
#include <sstream>
inline constexpr unsigned framely_pad_vendor=0x0001,framely_pad_product=0xf001;
struct SteamPadInfo {int slot=-1;unsigned vendor=0,product=0;bool identified=false;};
inline std::vector<SteamPadInfo> read_steam_pad_info(std::istream& stream){
 std::vector<SteamPadInfo> rows;std::string line;size_t bytes=0;
 while(std::getline(stream,line)){bytes+=line.size()+1;if(bytes>65536)return {};int slot=-1;if(std::sscanf(line.c_str(),"[slot %d]",&slot)==1){if(slot<0||slot>63)return {};rows.push_back({slot});continue;}if(rows.empty())continue;auto& p=rows.back();unsigned value=0;if(std::sscanf(line.c_str(),"VID=%x",&value)==1)p.vendor=value;else if(std::sscanf(line.c_str(),"PID=%x",&value)==1){p.product=value;p.identified=true;}}
 return rows;
}
inline bool allowed_steam_slot(const std::vector<SteamPadInfo>& rows,int slot){return std::any_of(rows.begin(),rows.end(),[&](auto p){return p.slot==slot&&p.identified&&p.vendor!=0&&!(p.vendor==framely_pad_vendor&&p.product==framely_pad_product);});}
inline std::array<int,18> steam_pad_state(const std::array<bool,15>& buttons,const std::array<int,6>& axes){
 std::array<int,18> state{};const int indices[]={0,1,2,3,9,10,4,6,7,8};for(int k=0;k<10;k++)state[k]=buttons[indices[k]];
 for(int k=0;k<4;k++)state[10+k]=std::clamp(axes[k],-32768,32767);
 for(int k=0;k<2;k++)state[14+k]=std::clamp(axes[4+k],0,32767)*255/32767;
 state[16]=int(buttons[14])-int(buttons[13]);state[17]=int(buttons[12])-int(buttons[11]);return state;
}
class SteamGamepad {
 void* library=nullptr;
 using Pad=void*;
 bool (*init)(uint32_t)=nullptr;void (*quit)()=nullptr;void (*update)()=nullptr;
 uint32_t* (*get)(int*)=nullptr;void (*freeMem)(void*)=nullptr;Pad (*openPad)(uint32_t)=nullptr;void (*closePad)(Pad)=nullptr;
 const char* (*path)(uint32_t)=nullptr;bool (*button)(Pad,int)=nullptr;int16_t (*axis)(Pad,int)=nullptr;
 bool (*rumblePad)(Pad,uint16_t,uint16_t,uint32_t)=nullptr;
 struct Entry {uint32_t id;Pad pad;std::array<int,18> previous{};};std::vector<Entry> pads;
 std::string infoPath;uint32_t selected=0,vibrating=0;uint64_t scanned=0;bool scanning=false,initialized=false;
 template<class T>bool symbol(T& target,const char* name){target=reinterpret_cast<T>(dlsym(library,name));return target!=nullptr;}
 bool valid(uint32_t id,const std::vector<SteamPadInfo>& info){
  const char* p=path(id);if(!p||std::string(p).rfind("/dev/input/event",0)!=0)return false;
  const std::string suffix=std::string(p).substr(16);if(suffix.empty()||!std::all_of(suffix.begin(),suffix.end(),[](char c){return c>='0'&&c<='9';}))return false;
  int fd=::open(p,O_RDONLY|O_NONBLOCK|O_CLOEXEC);if(fd<0)return false;input_id device{};char name[128]{};const bool ok=ioctl(fd,EVIOCGID,&device)==0&&device.vendor==0x28de&&device.product==0x11ff&&ioctl(fd,EVIOCGNAME(sizeof name),name)>0;::close(fd);if(!ok)return false;
  const char* at=std::strstr(name,"pad ");int slot=-1;return at&&std::sscanf(at+4,"%d",&slot)==1&&allowed_steam_slot(info,slot);
 }
 void scan(uint64_t now){
  if(scanning&&now-scanned<500)return;scanning=true;scanned=now;std::ifstream f(infoPath);auto info=read_steam_pad_info(f);
  int count=0;auto* ids=get(&count);std::vector<uint32_t> validIds;
  if(ids&&count>=0&&count<=64)for(int k=0;k<count;k++)if(valid(ids[k],info))validIds.push_back(ids[k]);freeMem(ids);
  for(auto it=pads.begin();it!=pads.end();){if(std::find(validIds.begin(),validIds.end(),it->id)==validIds.end()){if(vibrating==it->id){rumblePad(it->pad,0,0,0);vibrating=0;}if(selected==it->id)selected=0;closePad(it->pad);it=pads.erase(it);}else ++it;}
  for(auto id:validIds)if(std::none_of(pads.begin(),pads.end(),[&](auto p){return p.id==id;})){auto p=openPad(id);if(p)pads.push_back({id,p});}
 }
public:
 uint32_t source()const{return selected;}
 bool start(const char* lib,const char* info){
  if(!lib||!info)return false;infoPath=info;library=dlopen(lib,RTLD_NOW|RTLD_LOCAL);if(!library)return false;
  bool ok=symbol(init,"SDL_Init")&&symbol(quit,"SDL_Quit")&&symbol(update,"SDL_UpdateGamepads")&&symbol(get,"SDL_GetGamepads")&&symbol(freeMem,"SDL_free")&&symbol(openPad,"SDL_OpenGamepad")&&symbol(closePad,"SDL_CloseGamepad")&&symbol(path,"SDL_GetGamepadPathForID")&&symbol(button,"SDL_GetGamepadButton")&&symbol(axis,"SDL_GetGamepadAxis")&&symbol(rumblePad,"SDL_RumbleGamepad");if(!ok)return false;
  setenv("SDL_GAMECONTROLLER_ALLOW_STEAM_VIRTUAL_GAMEPAD","1",1);setenv("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS","1",1);setenv("SteamVirtualGamepadInfo",info,1);
  // Only Steam's post-configuration evdev pads are accepted. HIDAPI must not
  // additionally open the physical devices or bypass Steam's mappings.
  setenv("SDL_JOYSTICK_HIDAPI","0",1);setenv("SDL_GAMECONTROLLER_IGNORE_DEVICES","0x0001/0xf001",1);
  initialized=init(0x2000);return initialized;
 }
 std::array<int,18> read(uint64_t now,bool enabled){
  std::array<int,18> result{};if(!initialized)return result;update();scan(now);uint32_t next=selected;
  for(auto& p:pads){std::array<bool,15> buttons{};std::array<int,6> axes{};for(int k=0;k<15;k++)buttons[k]=button(p.pad,k);for(int k=0;k<6;k++)axes[k]=axis(p.pad,k);auto state=steam_pad_state(buttons,axes);
   bool activity=false;for(int k=0;k<10;k++)activity|=state[k]&&!p.previous[k];for(int k=10;k<16;k++)activity|=std::abs(state[k])>(k<14?8000:40)&&std::abs(state[k]-p.previous[k])>(k<14?1500:15);for(int k=16;k<18;k++)activity|=state[k]!=0&&state[k]!=p.previous[k];p.previous=state;
   if(enabled&&activity)next=p.id;
  }
  if(!next&&!pads.empty())next=pads.front().id;
  if(next!=selected){stop();selected=next;return result;}
  if(enabled)for(auto& p:pads)if(p.id==selected)result=p.previous;return result;
 }
 bool rumble(uint16_t low,uint16_t high){if(!low&&!high){stop();return true;}for(auto& p:pads)if(p.id==selected){const bool ok=rumblePad(p.pad,low,high,250);if(ok)vibrating=p.id;return ok;}return false;}
 // Cancel only feedback issued by this bridge, never unrelated Steam rumble.
 void stop(){for(auto& p:pads)if(p.id==vibrating)rumblePad(p.pad,0,0,0);vibrating=0;}
 ~SteamGamepad(){if(initialized){stop();for(auto& p:pads)closePad(p.pad);quit();}if(library)dlclose(library);}
};
