#include "notification_badge.h"
#include <cassert>

int main(){
 std::vector<uint8_t> original(128*128*4,40),pixels=original;
 notification_badge(pixels,0);assert(pixels==original);
 notification_badge(pixels,1);assert(pixels!=original);
 auto one=pixels;pixels=original;notification_badge(pixels,12);assert(pixels!=one);
 pixels=original;notification_badge(pixels,99);auto ninety_nine=pixels;
 pixels=original;notification_badge(pixels,100);assert(pixels!=ninety_nine);
 auto maximum=pixels;pixels=original;notification_badge(pixels,256);assert(pixels==maximum);
 for(int y=0;y<128;y++)for(int x=0;x<128;x++)if(y>=33||x<74)
  for(int c=0;c<4;c++)assert(pixels[(y*128+x)*4+c]==40);
 std::vector<uint8_t> invalid(12,0);notification_badge(invalid,1);assert(invalid==std::vector<uint8_t>(12,0));
}
