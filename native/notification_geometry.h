#pragma once
#include <cstdint>
#include <cstddef>
#include <vector>

struct NotificationBounds {
 float x=0,y=0,width=0,height=0;
 bool visible() const {return width>0&&height>0;}
};

// The submitted BGRA buffer is flipped in on_paint to OpenVR's bottom-left origin.
inline NotificationBounds notification_bounds(const std::vector<uint8_t>& pixels,int width,int height,int scale){
 if(width<=0||height<=0||scale<=0||pixels.size()!=size_t(width)*height*4)return {};
 int left=width,low=height,right=-1,high=-1;
 for(int y=0;y<height;++y)for(int x=0;x<width;++x){
  if(!pixels[(size_t(y)*width+x)*4+3])continue;
  if(x<left)left=x;if(x>right)right=x;if(y<low)low=y;if(y>high)high=y;
 }
 if(right<left)return {};
 return {float(left)/scale,float(low)/scale,float(right-left+1)/scale,float(high-low+1)/scale};
}
