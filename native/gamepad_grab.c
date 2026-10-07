// Bionic preload: isolated virtual pad, or guarded Steam Input evdev passthrough.
// Raw AArch64 syscalls avoid a glibc dependency inside Android.
#define _GNU_SOURCE
#include <stdarg.h>
#include <fcntl.h>
#include <linux/input.h>
#include <time.h>
#include <stddef.h>
#include "gamepad_direct_policy.h"
extern int *__errno(void);
extern int pthread_create(unsigned long*,const void*,void*(*)(void*),void*);
extern int pthread_detach(unsigned long);
static long call(long nr,long a,long b,long c,long d){register long x0 __asm__("x0")=a;register long x1 __asm__("x1")=b;register long x2 __asm__("x2")=c;register long x3 __asm__("x3")=d;register long x8 __asm__("x8")=nr;__asm__ volatile("svc 0":"+r"(x0):"r"(x1),"r"(x2),"r"(x3),"r"(x8):"memory");return x0;}
static long checked(long r){if(r<0){*__errno()=-r;return -1;}return r;}
static int same(const char*a,const char*b){while(*a&&*a==*b){a++;b++;}return *a==*b;}
#ifndef FRAMELY_EVENT_PATH
#define FRAMELY_EVENT_PATH "/dev/input/event250"
#define FRAMELY_CONTROL_PATH "/framely-gamepad-control"
#define FRAMELY_READY_PATH "/framely-gamepad-ready/ready"
#endif
#define MAX_FDS 4096
static unsigned char direct_fds[MAX_FDS],was_enabled[MAX_FDS];
static uint64_t playing[MAX_FDS];
static int control=-1,lock=0,started=0;
static void enter(void){while(__atomic_exchange_n(&lock,1,__ATOMIC_ACQUIRE))call(124,0,0,0,0);}
static void leave(void){__atomic_store_n(&lock,0,__ATOMIC_RELEASE);}
static struct FramelyDirectControl policy(uint64_t* now){
 struct timespec t;struct FramelyDirectControl c={0};*now=0;
 if(call(113,CLOCK_MONOTONIC,(long)&t,0,0)<0)return c;
 *now=(uint64_t)t.tv_sec*1000000000ULL+t.tv_nsec;
 if(control<0||call(67,control,(long)&c,sizeof c,0)!=sizeof c){c.expires_ns=0;c.enabled=0;c.rumble=0;}return c;
}
static void stop_effects(int fd){uint64_t mask=playing[fd];playing[fd]=0;for(int i=0;i<64;i++)if(mask&(1ULL<<i)){struct input_event e={0};e.type=EV_FF;e.code=i;call(64,fd,(long)&e,sizeof e,0);}}
static void* watch(void* unused){(void)unused;for(;;){enter();uint64_t now;struct FramelyDirectControl c=policy(&now);if(!framely_direct_rumble(c,now))for(int fd=0;fd<MAX_FDS;fd++)if(direct_fds[fd]&&playing[fd])stop_effects(fd);leave();struct timespec delay={0,20000000};call(101,(long)&delay,0,0,0);}return 0;}
static int opened(int dir,const char*p,int flags,int mode){
 long fd=call(56,dir,(long)p,flags,mode);if(fd<0)return checked(fd);
 if(same(p,FRAMELY_EVENT_PATH)){
  enter();if(control<0)control=call(56,-100,(long)FRAMELY_CONTROL_PATH,O_RDONLY|O_CLOEXEC,0);
  if(control<0&&control!=-2){int error=-control;leave();call(57,fd,0,0,0);*__errno()=error;return -1;}
  int direct=control>=0;
  if(direct){if(fd>=MAX_FDS){leave();call(57,fd,0,0,0);*__errno()=24;return -1;}__atomic_store_n(&direct_fds[fd],1,__ATOMIC_RELEASE);was_enabled[fd]=1;playing[fd]=0;
   if(!started){unsigned long thread;started=pthread_create(&thread,0,watch,0)==0;if(started)pthread_detach(thread);}
   // Without the feedback watchdog, fail closed rather than leave rumble active.
   if(!started){__atomic_store_n(&direct_fds[fd],0,__ATOMIC_RELEASE);leave();call(57,fd,0,0,0);*__errno()=5;return -1;}
  }
  leave();long r=direct?0:call(29,fd,EVIOCGRAB,1,0);
  if(r==0){long ready=call(56,-100,(long)FRAMELY_READY_PATH,O_WRONLY|O_CREAT|O_TRUNC|O_CLOEXEC,0644);if(ready>=0){call(64,ready,(long)"ready\n",6,0);call(57,ready,0,0,0);}}
 }
 return fd;
}
int open(const char*p,int flags,...){int mode=0;if((flags&O_CREAT)||(flags&O_TMPFILE)==O_TMPFILE){va_list ap;va_start(ap,flags);mode=va_arg(ap,int);va_end(ap);}return opened(-100,p,flags,mode);}
int open64(const char*p,int flags,...){int mode=0;if((flags&O_CREAT)||(flags&O_TMPFILE)==O_TMPFILE){va_list ap;va_start(ap,flags);mode=va_arg(ap,int);va_end(ap);}return opened(-100,p,flags,mode);}
int openat(int dir,const char*p,int flags,...){int mode=0;if((flags&O_CREAT)||(flags&O_TMPFILE)==O_TMPFILE){va_list ap;va_start(ap,flags);mode=va_arg(ap,int);va_end(ap);}return opened(dir,p,flags,mode);}
int __open_2(const char*p,int flags){return opened(-100,p,flags,0);}
int __openat_2(int dir,const char*p,int flags){return opened(dir,p,flags,0);}
long read(int fd,void* buffer,size_t bytes){
 long n=call(63,fd,(long)buffer,bytes,0);if(n<=0)return checked(n);
 if(fd<0||fd>=MAX_FDS||!__atomic_load_n(&direct_fds[fd],__ATOMIC_ACQUIRE))return n;
 enter();if(direct_fds[fd]){uint64_t now;struct FramelyDirectControl c=policy(&now);int enabled=framely_direct_enabled(c,now);
  if(!enabled){
   // Drain the original events. Release all Android button/axis state once,
   // then report EAGAIN, so background input is never replayed on resumption.
   const int keys[]={304,305,307,308,310,311,312,313,314,315,317,318};const int axes[]={0,1,3,4,2,5,16,17};
   if(was_enabled[fd]&&bytes>=21*sizeof(struct input_event)){struct input_event* e=buffer;int count=0;for(int i=0;i<12;i++){e[count]=(struct input_event){0};e[count].type=EV_KEY;e[count++].code=keys[i];}for(int i=0;i<8;i++){e[count]=(struct input_event){0};e[count].type=EV_ABS;e[count++].code=axes[i];}e[count]=(struct input_event){0};e[count].type=EV_SYN;e[count++].code=SYN_REPORT;n=count*sizeof *e;was_enabled[fd]=0;}else n=-11;
  }else was_enabled[fd]=1;
 }leave();return checked(n);
}
long write(int fd,const void* buffer,size_t bytes){
 if(fd<0||fd>=MAX_FDS||!__atomic_load_n(&direct_fds[fd],__ATOMIC_ACQUIRE))return checked(call(64,fd,(long)buffer,bytes,0));
 enter();long n;
 if(direct_fds[fd]){
  uint64_t now;struct FramelyDirectControl c=policy(&now);int allowed=framely_direct_rumble(c,now);const struct input_event* events=buffer;
  if(bytes%sizeof *events){leave();*__errno()=22;return -1;}
  n=0;for(size_t i=0;i<bytes/sizeof *events;i++){
   struct input_event e=events[i];if(e.type==EV_FF&&e.value>0&&!allowed)e.value=0;
   long r=call(64,fd,(long)&e,sizeof e,0);if(r<0){if(n==0)n=r;break;}n+=r;
   if(e.type==EV_FF&&e.code<64){if(e.value>0)playing[fd]|=1ULL<<e.code;else playing[fd]&=~(1ULL<<e.code);}
  }
 }else n=call(64,fd,(long)buffer,bytes,0);
 leave();return checked(n);
}
int close(int fd){if(fd<0||fd>=MAX_FDS||!__atomic_load_n(&direct_fds[fd],__ATOMIC_ACQUIRE))return checked(call(57,fd,0,0,0));enter();if(fd>=0&&fd<MAX_FDS&&direct_fds[fd]){stop_effects(fd);__atomic_store_n(&direct_fds[fd],0,__ATOMIC_RELEASE);}long result=call(57,fd,0,0,0);leave();return checked(result);}
int ioctl(int fd,unsigned long request,...){
 va_list ap;va_start(ap,request);void* argument=va_arg(ap,void*);va_end(ap);
 if(fd<0||fd>=MAX_FDS||!__atomic_load_n(&direct_fds[fd],__ATOMIC_ACQUIRE))return checked(call(29,fd,request,(long)argument,0));
 enter();long result=call(29,fd,request,(long)argument,0);
 if(result>=0&&direct_fds[fd]){
  uint64_t now;struct FramelyDirectControl c=policy(&now);
  if(!framely_direct_enabled(c,now)&&_IOC_TYPE(request)=='E'&&(_IOC_DIR(request)&_IOC_READ)){
   if(_IOC_NR(request)==0x18){unsigned char* bytes=argument;for(unsigned i=0;i<_IOC_SIZE(request);i++)bytes[i]=0;}
   else if(_IOC_NR(request)>=0x40&&_IOC_NR(request)<=0x7f&&_IOC_SIZE(request)==sizeof(struct input_absinfo))((struct input_absinfo*)argument)->value=0;
  }
  if(request==EVIOCRMFF&&(unsigned long)argument<64)playing[fd]&=~(1ULL<<(unsigned long)argument);
 }
 leave();return checked(result);
}
// Bionic's fortified entry points must use the same foreground/feedback gate.
long __read_chk(int fd,void* buffer,size_t bytes,size_t capacity){if(bytes>capacity){*__errno()=14;return -1;}return read(fd,buffer,bytes);}
long __write_chk(int fd,const void* buffer,size_t bytes,size_t capacity){if(bytes>capacity){*__errno()=14;return -1;}return write(fd,buffer,bytes);}
