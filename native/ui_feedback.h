#pragma once
#include <cstdint>
#include <cstdlib>
#include <dlfcn.h>
#include <string>
#include <iostream>

// Load Steam's installed SDL/audio assets; no Steam sounds are redistributed.
// Only the native UI thread owns these streams. Playback queues cached PCM and
// never spawns a process or reads files on a hover/click.
class SteamUiSounds {
 struct Spec { int format, channels, freq; };
 struct Clip { void* stream=nullptr; uint8_t* data=nullptr; uint32_t size=0; } clips[2];
 void* library=nullptr;
 bool initialized=false;
 bool (*init)(uint32_t)=nullptr;
 void (*quit)(uint32_t)=nullptr;
 bool (*load)(const char*,Spec*,uint8_t**,uint32_t*)=nullptr;
 void* (*open)(uint32_t,const Spec*,void*,void*)=nullptr;
 bool (*resume)(void*)=nullptr;
 bool (*put)(void*,const void*,int)=nullptr;
 bool (*clear)(void*)=nullptr;
 bool (*gain)(void*,float)=nullptr;
 void (*destroy)(void*)=nullptr;
 void (*release)(void*)=nullptr;
 template<class T> bool bind(T& fn,const char* name){fn=reinterpret_cast<T>(dlsym(library,name));return fn!=nullptr;}
 public:
 SteamUiSounds()=default;
 SteamUiSounds(const SteamUiSounds&)=delete;
 ~SteamUiSounds(){shutdown();}
 void shutdown(){
  for(auto& clip:clips){if(clip.stream&&destroy)destroy(clip.stream);if(clip.data&&release)release(clip.data);clip={};}
  if(initialized&&quit)quit(0x10);initialized=false;
  if(library)dlclose(library);library=nullptr;
 }
 bool prepare(const std::string& root){
  if(root.empty())return false;
  for(const auto* name:{"steamrtarm64/libSDL3.so.0","steamrt64/libSDL3.so.0"}){library=dlopen((root+"/"+name).c_str(),RTLD_NOW|RTLD_LOCAL);if(library)break;}
  if(!library)return false;
  if(!(bind(init,"SDL_InitSubSystem")&&bind(quit,"SDL_QuitSubSystem")&&bind(load,"SDL_LoadWAV")&&bind(open,"SDL_OpenAudioDeviceStream")&&bind(resume,"SDL_ResumeAudioStreamDevice")&&bind(put,"SDL_PutAudioStreamData")&&bind(clear,"SDL_ClearAudioStream")&&bind(gain,"SDL_SetAudioStreamGain")&&bind(destroy,"SDL_DestroyAudioStream")&&bind(release,"SDL_free"))){shutdown();return false;}
  if(!init(0x10)){shutdown();return false;}initialized=true;
  const char* names[]={"deck_ui_typing.wav","deck_ui_typing.wav"};
  for(int i=0;i<2;++i){Spec spec{};auto& clip=clips[i];
   if(!load((root+"/steamui/sounds/"+names[i]).c_str(),&spec,&clip.data,&clip.size)||!clip.size||clip.size>1024*1024){shutdown();return false;}
   clip.stream=open(0xffffffffu,&spec,nullptr,nullptr);
   if(!clip.stream||!gain(clip.stream,i==0?.30f:.45f)||!resume(clip.stream)){shutdown();return false;}
  }
  return true;
 }
 void silence(){if(!initialized)return;for(auto& clip:clips)clear(clip.stream);}
 void play(bool activation=false){if(!initialized)return;silence();auto& clip=clips[activation?1:0];put(clip.stream,clip.data,int(clip.size));}
};
