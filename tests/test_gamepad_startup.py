"""Steam mode must not register an OpenVR action application."""
import os
import pathlib
import platform
import shutil
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


@unittest.skipUnless(platform.machine() == 'aarch64' and os.geteuid() != 0 and shutil.which('g++'), 'ARM64 native bridge requires a non-root user and g++')
class GamepadStartup(unittest.TestCase):
    def test_steam_skips_openvr_but_frame_direct_initializes_it(self):
        with tempfile.TemporaryDirectory(dir=ROOT / 'target', prefix='gamepad-startup-') as directory:
            work = pathlib.Path(directory)
            vr = work / 'vr.cpp'
            symbols = ['VR_InitInternal', 'VR_InitInternal2', 'VR_ShutdownInternal', 'VR_GetGenericInterface', 'VR_GetInitToken', 'VR_IsInterfaceVersionValid']
            vr.write_text('#include <cstdio>\n#include <cstdlib>\n' + '\n'.join('extern "C" void ' + name + '(){fputs("OPENVR_USED\\n",stderr);exit(91);}' for name in symbols))
            subprocess.run(['g++', '-shared', '-fPIC', str(vr), '-o', str(work / 'libopenvr_api.so')], check=True)
            sdl = work / 'sdl.cpp'
            sdl.write_text('''#include <cstdio>
#include <cstdint>
extern "C" {
bool SDL_Init(uint32_t){fputs("SDL_USED\\n",stderr);return true;}
void SDL_Quit(){} void SDL_UpdateGamepads(){}
void SDL_PumpEvents(){fputs("SDL_PUMPED\\n",stderr);}
void SDL_FlushEvents(uint32_t,uint32_t){fputs("SDL_FLUSHED\\n",stderr);}
uint32_t* SDL_GetGamepads(int* n){*n=0;return nullptr;}
void SDL_free(void*){} void* SDL_OpenGamepad(uint32_t){return nullptr;}
void SDL_CloseGamepad(void*){} const char* SDL_GetGamepadPathForID(uint32_t){return nullptr;}
bool SDL_GetGamepadButton(void*,int){return false;} int16_t SDL_GetGamepadAxis(void*,int){return 0;}
bool SDL_RumbleGamepad(void*,uint16_t,uint16_t,uint32_t){return true;}
}
''')
            subprocess.run(['g++', '-shared', '-fPIC', str(sdl), '-o', str(work / 'libsdl-test.so')], check=True)
            loop = work / 'loop.cpp'
            loop.write_text('#include "gamepad_steam.h"\nint main(int argc,char** argv){SteamGamepad pad;if(!pad.start(argv[1],argv[2]))return 1;pad.read(0,false);pad.read(501,false);}')
            subprocess.run(['g++', '-std=c++17', '-I'+str(ROOT/'native'), str(loop), '-ldl', '-o', str(work/'loop')], check=True)
            result = subprocess.run([str(work/'loop'), str(work/'libsdl-test.so'), str(work/'info')], capture_output=True, text=True, check=True)
            self.assertEqual(result.stderr.count('SDL_PUMPED'), 2)
            self.assertEqual(result.stderr.count('SDL_FLUSHED'), 2)
            helper = work / 'bridge'
            subprocess.run(['g++', '-std=c++17', '-O2', '-I'+str(ROOT/'native/vendor/openvr'), str(ROOT/'native/gamepad.cpp'), '-L'+str(ROOT/'native/vendor/openvr/lib/linuxarm64'), '-lopenvr_api', '-ldl', '-pthread', '-o', str(helper)], check=True)
            env = dict(os.environ, LD_LIBRARY_PATH=str(work), FRAMELY_STEAM_SDL_LIBRARY=str(work/'libsdl-test.so'), FRAMELY_STEAM_GAMEPAD_INFO=str(work/'info'))
            args = [str(helper), '/nonexistent/actions.json', '/nonexistent/app.vrmanifest', 'framely.test']
            steam = subprocess.run(args, env=dict(env, FRAMELY_GAMEPAD_SOURCE='steam'), input='', capture_output=True, text=True, timeout=10)
            self.assertIn('SDL_USED', steam.stderr)
            self.assertNotIn('OPENVR_USED', steam.stderr)
            self.assertNotIn('Invalid gamepad action', steam.stderr)
            direct = subprocess.run(args, env=dict(env, FRAMELY_GAMEPAD_SOURCE='steam-direct', FRAMELY_GAMEPAD_READY=str(work)), input='', capture_output=True, text=True, timeout=10)
            self.assertIn('No writable Steam Input output', direct.stderr)
            self.assertNotIn('OPENVR_USED', direct.stderr)
            self.assertNotIn('SDL_USED', direct.stderr)
            frame = subprocess.run(args, env=dict(env, FRAMELY_GAMEPAD_SOURCE='frame'), input='', capture_output=True, text=True, timeout=10)
            self.assertIn('OPENVR_USED', frame.stderr)
            self.assertNotIn('SDL_USED', frame.stderr)


if __name__ == '__main__':
    unittest.main()
