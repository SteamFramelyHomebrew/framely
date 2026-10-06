// Bionic preload: exclusively claim only Framely's mounted virtual event node.
// Raw AArch64 syscalls avoid a glibc dependency inside Android.
#define _GNU_SOURCE
#include <stdarg.h>
#include <fcntl.h>
#include <linux/input.h>
extern int *__errno(void);
static long call(long nr,long a,long b,long c,long d){register long x0 __asm__("x0")=a;register long x1 __asm__("x1")=b;register long x2 __asm__("x2")=c;register long x3 __asm__("x3")=d;register long x8 __asm__("x8")=nr;__asm__ volatile("svc 0":"+r"(x0):"r"(x1),"r"(x2),"r"(x3),"r"(x8):"memory");return x0;}
static int same(const char*a,const char*b){while(*a&&*a==*b){a++;b++;}return *a==*b;}
static int opened(int dir,const char*p,int flags,int mode){long fd=call(56,dir,(long)p,flags,mode);if(fd<0){*__errno()=-fd;return -1;}if(same(p,"/dev/input/event250")){long r=call(29,fd,EVIOCGRAB,1,0);if(r==0){long ready=call(56,-100,(long)"/framely-gamepad-ready/ready",O_WRONLY|O_CREAT|O_TRUNC|O_CLOEXEC,0644);if(ready>=0){call(64,ready,(long)"ready\n",6,0);call(57,ready,0,0,0);}}}return fd;}
int open(const char*p,int flags,...){int mode=0;if((flags&O_CREAT)||(flags&O_TMPFILE)==O_TMPFILE){va_list ap;va_start(ap,flags);mode=va_arg(ap,int);va_end(ap);}return opened(-100,p,flags,mode);}
int open64(const char*p,int flags,...){int mode=0;if((flags&O_CREAT)||(flags&O_TMPFILE)==O_TMPFILE){va_list ap;va_start(ap,flags);mode=va_arg(ap,int);va_end(ap);}return opened(-100,p,flags,mode);}
int openat(int dir,const char*p,int flags,...){int mode=0;if((flags&O_CREAT)||(flags&O_TMPFILE)==O_TMPFILE){va_list ap;va_start(ap,flags);mode=va_arg(ap,int);va_end(ap);}return opened(dir,p,flags,mode);}
int __open_2(const char*p,int flags){return opened(-100,p,flags,0);}
int __openat_2(int dir,const char*p,int flags){return opened(dir,p,flags,0);}
