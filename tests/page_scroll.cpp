#include "page_scroll.h"
#include <cassert>
int main(){
 auto t=std::chrono::steady_clock::now();PageScrollStick s;
 assert(!s.update(0,.1f,true,t).y);assert(!s.update(0,1,false,t).y);
 int sixty=0;for(int i=1;i<=60;i++)sixty+=s.update(0,1,true,t+std::chrono::microseconds(i*1000000/60)).y;
 s.clear();int ninety=0;for(int i=1;i<=90;i++)ninety+=s.update(0,1,true,t+std::chrono::microseconds(i*1000000/90)).y;
 assert(std::abs(sixty-ninety)<10);auto d=s.update(.8f,.9f,true,t+std::chrono::seconds(2));assert(d.x==0&&d.y>0&&d.y<=55);
 d=s.update(1,.2f,true,t+std::chrono::milliseconds(2017));assert(d.x<0&&d.y==0);
 assert(!s.update(NAN,1,true,t).y);assert(!s.update(0,0,true,t).y);s.clear();assert(s.last.time_since_epoch().count()==0);
}
