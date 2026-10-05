#include "launcher_pointer.h"
#include <cassert>
int main(){auto now=std::chrono::steady_clock::now();LauncherPointer pointer;int x=0,y=0;
 pointer.begin(100,200,now);x=122;y=211;assert(pointer.constrain(x,y,now+std::chrono::milliseconds(180)));assert(x==100&&y==200);
 // A 600 ms long press remains stable across many different jitter samples.
 for(int ms=100;ms<=900;ms+=100){x=100+(ms%35);y=190;assert(pointer.constrain(x,y,now+std::chrono::milliseconds(ms)));assert(x==100&&y==200);}
 x=126;y=215;assert(pointer.constrain(x,y,now+std::chrono::milliseconds(1100)));pointer.clear();assert(!pointer.holding(now));
 pointer.begin(100,200,now);x=160;y=200;assert(!pointer.constrain(x,y,now+std::chrono::milliseconds(100)));assert(x==160);x=100;assert(!pointer.constrain(x,y,now+std::chrono::milliseconds(200)));
 pointer.begin(100,200,now);x=100;y=200;assert(!pointer.constrain(x,y,now+std::chrono::seconds(3)));pointer.clear();assert(!pointer.constrain(x,y,now));
}
