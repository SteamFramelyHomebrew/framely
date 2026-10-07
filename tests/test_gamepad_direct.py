"""Exercise the production Android guard with deterministic evdev-shaped data."""
import os
import pathlib
import platform
import shutil
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]

@unittest.skipUnless(platform.machine() == 'aarch64' and shutil.which('gcc'), 'AArch64 syscall guard')
class DirectGuard(unittest.TestCase):
    def test_passthrough_gating_expiry_and_rumble_cancellation(self):
        with tempfile.TemporaryDirectory(prefix='framely-direct-', dir='/tmp') as tmp:
            work = pathlib.Path(tmp)
            paths = {"FRAMELY_EVENT_PATH": work/'event', "FRAMELY_CONTROL_PATH": work/'control', "FRAMELY_READY_PATH": work/'ready'}
            lib = work/'guard.so'
            subprocess.run(['gcc', '-shared', '-fPIC', '-nostdlib', '-fno-stack-protector', '-mno-outline-atomics', '-Wl,--hash-style=both', *[f'-D{k}="{v}"' for k,v in paths.items()], str(ROOT/'native/gamepad_grab.c'), '-o',str(lib)],check=True)
            source=work/'test.c'
            source.write_text(r'''
#define _GNU_SOURCE
#include <errno.h>
#include <assert.h>
#include <fcntl.h>
#include <linux/input.h>
#include <time.h>
#include <unistd.h>
#include <stdio.h>
#include "gamepad_direct_policy.h"
int *__errno(void){return &errno;}
static uint64_t now(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return (uint64_t)t.tv_sec*1000000000ULL+t.tv_nsec;}
int main(int argc,char**argv){
 int control=open(argv[2],O_RDWR|O_CREAT,0644);assert(control>=0);
 struct FramelyDirectControl c={now()+2000000000ULL,1,1};assert(pwrite(control,&c,sizeof c,0)==sizeof c);
 int seed=open(argv[1],O_RDWR|O_CREAT,0644);assert(seed>=0);struct input_event e={0};e.type=EV_KEY;e.code=BTN_A;e.value=1;
 assert(pwrite(seed,&e,sizeof e,0)==sizeof e);close(seed);
 int fd=open(argv[1],O_RDWR);assert(fd>=0);struct input_event events[64];
 assert(read(fd,events,sizeof events)==sizeof e);assert(events[0].code==BTN_A&&events[0].value==1);
 // Disabling returns a neutral snapshot once, then drains subsequent input.
 c.enabled=0;assert(pwrite(control,&c,sizeof c,0)==sizeof c);lseek(fd,0,SEEK_SET);
 assert(read(fd,events,sizeof events)==21*sizeof e);for(int i=0;i<21;i++)assert(events[i].value==0);
 lseek(fd,0,SEEK_SET);assert(read(fd,events,sizeof events)==-1&&errno==EAGAIN);
 c.enabled=1;c.expires_ns=now()+2000000000ULL;assert(pwrite(control,&c,sizeof c,0)==sizeof c);lseek(fd,0,SEEK_SET);
 assert(read(fd,events,sizeof events)==sizeof e&&events[0].value==1);
 // A missing session lease must fail closed.
 c.expires_ns=now()-1;assert(pwrite(control,&c,sizeof c,0)==sizeof c);lseek(fd,0,SEEK_SET);
 assert(read(fd,events,sizeof events)==21*sizeof e);
 c.expires_ns=now()+2000000000ULL;c.rumble=0;assert(pwrite(control,&c,sizeof c,0)==sizeof c);
 e.type=EV_FF;e.code=3;e.value=1;lseek(fd,0,SEEK_SET);assert(write(fd,&e,sizeof e)==sizeof e);
 assert(pread(fd,&e,sizeof e,0)==sizeof e&&e.value==0);
 c.rumble=1;assert(pwrite(control,&c,sizeof c,0)==sizeof c);e.value=1;lseek(fd,0,SEEK_SET);assert(write(fd,&e,sizeof e)==sizeof e);
 c.enabled=0;assert(pwrite(control,&c,sizeof c,0)==sizeof c);usleep(80000);
 assert(pread(fd,&e,sizeof e,sizeof e)==sizeof e&&e.type==EV_FF&&e.code==3&&e.value==0);
 close(fd);close(control);return 0;
}
''')
            executable=work/'test'
            subprocess.run(['gcc','-rdynamic','-I'+str(ROOT/'native'),str(source),'-o',str(executable)],check=True)
            subprocess.run([str(executable),str(paths['FRAMELY_EVENT_PATH']),str(paths['FRAMELY_CONTROL_PATH'])],env=dict(os.environ,LD_PRELOAD=str(lib)),check=True,timeout=10)
            self.assertEqual(paths['FRAMELY_READY_PATH'].read_text(),'ready\n')

if __name__=='__main__':
    unittest.main()
