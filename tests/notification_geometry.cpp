#include "notification_geometry.h"
#include <cassert>

int main(){
 std::vector<uint8_t> pixels(12*8*4,0);
 assert(!notification_bounds(pixels,12,8,2).visible());
 // RGB left over in a transparent frame must not capture input.
 pixels[0]=255;
 assert(!notification_bounds(pixels,12,8,2).visible());
 pixels[(1*12+2)*4+3]=255;
 pixels[(4*12+9)*4+3]=1;
 auto bounds=notification_bounds(pixels,12,8,2);
 assert(bounds.visible());
 assert(bounds.x==1&&bounds.y==.5f&&bounds.width==4&&bounds.height==2);
 // Dismissing/expiring the final toast yields an empty frame again.
 pixels[(1*12+2)*4+3]=0;pixels[(4*12+9)*4+3]=0;
 assert(!notification_bounds(pixels,12,8,2).visible());
 pixels.back()=255;
 bounds=notification_bounds(pixels,12,8,2);
 assert(bounds.x==5.5f&&bounds.y==3.5f&&bounds.width==.5f&&bounds.height==.5f);
 assert(!notification_bounds(pixels,12,8,0).visible());
 assert(!notification_bounds({},12,8,2).visible());
}
