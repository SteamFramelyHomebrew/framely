"""Exercise cached audio with a fake SDL, without speakers/Steam/CEF."""
import pathlib, subprocess, tempfile
root = pathlib.Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory() as tmp:
    tmp = pathlib.Path(tmp)
    (tmp / 'steamrtarm64').mkdir()
    fake = tmp / 'fake.cpp'
    fake.write_text(r'''
#include <cstdint>
#include <cstdlib>
#include <cassert>
#include <cstring>
static int loads=0, writes=0, streams=0, cleared=0;
extern "C" {
bool SDL_InitSubSystem(uint32_t flags){assert(flags==0x10);return true;}
void SDL_QuitSubSystem(uint32_t flags){assert(flags==0x10);assert(streams==0);}
bool SDL_LoadWAV(const char* path,void* spec,uint8_t** data,uint32_t* len){assert(strstr(path,"/steamui/sounds/deck_ui_typing.wav"));++loads;*data=(uint8_t*)malloc(4);*len=4;return true;}
void* SDL_OpenAudioDeviceStream(uint32_t id,const void*,void* callback,void* userdata){assert(id==0xffffffffu&&!callback&&!userdata);++streams;return malloc(1);}
bool SDL_SetAudioStreamGain(void*,float gain){assert(gain==.30f||gain==.45f);return true;}
bool SDL_ResumeAudioStreamDevice(void*){return true;}
bool SDL_PutAudioStreamData(void*,const void*,int len){assert(loads==2&&len==4);++writes;return true;}
bool SDL_ClearAudioStream(void*){++cleared;return true;}
void SDL_DestroyAudioStream(void* stream){--streams;free(stream);}
void SDL_free(void* data){free(data);}
int counts(int which){return which==0?loads:which==1?writes:cleared;}
}
''')
    library=tmp/'steamrtarm64/libSDL3.so.0'
    subprocess.run(['g++','-shared','-fPIC',str(fake),'-o',str(library)],check=True)
    test=tmp/'test.cpp'
    test.write_text(r'''
#include "ui_feedback.h"
#include <cassert>
int main(int argc,char** argv){
 SteamUiSounds sounds;assert(!sounds.prepare("/missing"));sounds.play();sounds.silence();
 assert(sounds.prepare(argv[1]));
 auto lib=dlopen((std::string(argv[1])+"/steamrtarm64/libSDL3.so.0").c_str(),RTLD_NOW);
 auto counts=reinterpret_cast<int(*)(int)>(dlsym(lib,"counts"));
 sounds.play();sounds.play(true);sounds.silence();
 assert(counts(0)==2&&counts(1)==2&&counts(2)==6);
 sounds.shutdown();sounds.play();assert(counts(1)==2);dlclose(lib);
}
''')
    subprocess.run(['g++','-std=c++17','-I'+str(root/'native'),str(test),'-ldl','-o',str(tmp/'test')],check=True)
    subprocess.run([str(tmp/'test'),str(tmp)],check=True)
print('Steam UI cached sound playback: ok')
