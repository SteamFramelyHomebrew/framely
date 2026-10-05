#pragma once
#include <algorithm>
#include <cstdint>
#include <string>
#include <vector>

// Rasterize the unread count directly onto the Dock's 128px icon texture.
// The zero state leaves the original icon byte-for-byte unchanged.
inline void notification_badge(std::vector<uint8_t>& rgba, unsigned count) {
 if (!count || rgba.size()!=128*128*4) return;
 const std::string label=count>99?"99+":std::to_string(count);
 const int text_width=int(label.size())*12-3,width=text_width+14,left=124-width,top=4,height=29;
 for(int y=top;y<top+height;y++)for(int x=left;x<124;x++){
  const int dx=std::max({left+6-x,0,x-117}),dy=std::max({top+6-y,0,y-(top+height-7)});
  if(dx*dx+dy*dy>36)continue;
  auto i=(y*128+x)*4;rgba[i]=147;rgba[i+1]=197;rgba[i+2]=237;rgba[i+3]=255;
 }
 // Compact 3x5 numerals retain a readable count at the Frame Dock's display scale.
 constexpr uint16_t digits[]={0b111101101101111,0b010110010010111,0b111001111100111,0b111001111001111,0b101101111001001,0b111100111001111,0b111100111101111,0b111001001001001,0b111101111101111,0b111101111001111};
 for(size_t n=0;n<label.size();n++){
  const uint16_t bits=label[n]=='+'?0b000010111010000:digits[label[n]-'0'];
  for(int y=0;y<5;y++)for(int x=0;x<3;x++)if(bits&(1u<<(14-y*3-x)))
   for(int sy=0;sy<3;sy++)for(int sx=0;sx<3;sx++){
    auto i=((top+7+y*3+sy)*128+left+7+int(n)*12+x*3+sx)*4;
    rgba[i]=23;rgba[i+1]=25;rgba[i+2]=28;rgba[i+3]=255;
   }
 }
}
