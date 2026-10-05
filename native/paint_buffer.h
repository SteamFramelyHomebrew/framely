#pragma once
#include <algorithm>
#include <cstdint>
#include <vector>
// Each alternating GPU texture retains its own pending damage until uploaded.
// Coordinates match the vertically flipped CPU buffer, not CEF's top-left view.
struct PaintDamage {
 int x=0,y=0,width=0,height=0;
 bool empty() const{return width<=0||height<=0;}
 void clear(){*this={};}
 void include(int left,int top,int right,int bottom){
  if(right<=left||bottom<=top)return;
  if(!empty()){right=std::max(right,x+width);bottom=std::max(bottom,y+height);left=std::min(left,x);top=std::min(top,y);}
  x=left;y=top;width=right-left;height=bottom-top;
 }
 template<class Rect> void add(int w,int h,size_t count,const Rect* rects){
  if(!count||!rects){include(0,0,w,h);return;}
  for(size_t i=0;i<count;i++){
   const auto& r=rects[i];const int left=std::clamp(r.x,0,w),top=std::clamp(r.y,0,h);
   const int right=int(std::clamp<long long>(static_cast<long long>(r.x)+r.width,0,w)),bottom=int(std::clamp<long long>(static_cast<long long>(r.y)+r.height,0,h));
   if(right>left&&bottom>top)include(left,h-bottom,right,h-top);
  }
 }
};
// CEF dirty rectangles use physical pixels. Preserve unchanged pixels between
// paints, and flip only the copied rows for OpenVR's bottom-left texture origin.
template<class Rect> inline void merge_paint(std::vector<uint8_t>& pixels,const void* buffer,int width,int height,size_t count,const Rect* dirty,bool flip=false){
 const size_t bytes=size_t(width)*height*4;const auto* source=static_cast<const uint8_t*>(buffer);
 auto copy=[&](int left,int top,int right,int bottom){for(int y=top;y<bottom;y++)std::copy_n(source+(size_t(y)*width+left)*4,size_t(right-left)*4,pixels.data()+(size_t(flip?height-1-y:y)*width+left)*4);};
 if(pixels.size()!=bytes||!count||!dirty){pixels.resize(bytes);copy(0,0,width,height);return;}
 for(size_t i=0;i<count;i++){const auto& r=dirty[i];int left=std::clamp(r.x,0,width),top=std::clamp(r.y,0,height),right=int(std::clamp<long long>(static_cast<long long>(r.x)+r.width,0,width)),bottom=int(std::clamp<long long>(static_cast<long long>(r.y)+r.height,0,height));if(right>left&&bottom>top)copy(left,top,right,bottom);}
}
