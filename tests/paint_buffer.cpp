#include "paint_buffer.h"
#include <cassert>
struct Rect{int x,y,width,height;};
int main(){
 std::vector<uint8_t> source(4*3*4,80),normal,flipped;Rect full{0,0,4,3};
 merge_paint(normal,source.data(),4,3,1,&full);merge_paint(flipped,source.data(),4,3,1,&full,true);
 // A partial repaint may contain unrelated pixels outside the dirty area.
 std::fill(source.begin(),source.end(),0);source[(1*4+2)*4]=200;Rect dirty{2,1,1,1};
 merge_paint(normal,source.data(),4,3,1,&dirty);merge_paint(flipped,source.data(),4,3,1,&dirty,true);
 assert(normal[0]==80&&normal[(1*4+2)*4]==200);assert(flipped[0]==80&&flipped[(1*4+2)*4]==200);
 source[(0*4+1)*4]=150;Rect top{1,0,1,1};merge_paint(flipped,source.data(),4,3,1,&top,true);assert(flipped[(2*4+1)*4]==150);assert(flipped[1*4]==80);
 Rect clipped{-1,-1,2,2};merge_paint(normal,source.data(),4,3,1,&clipped);assert(normal[0]==0&&normal[4]==80);
 // Simulate alternating GPU textures: a texture must catch up on all paints
 // since its last upload, including damage already sent to the other texture.
 PaintDamage pending[2];std::vector<uint8_t> gpu[2];
 source.assign(4*3*4,80);flipped.clear();
 auto paint=[&](Rect r){merge_paint(flipped,source.data(),4,3,1,&r,true);for(auto& d:pending)d.add(4,3,1,&r);};
 auto upload=[&](int slot){auto& d=pending[slot];if(gpu[slot].empty()){gpu[slot].resize(flipped.size());d.include(0,0,4,3);}for(int y=d.y;y<d.y+d.height;y++)std::copy_n(flipped.data()+(y*4+d.x)*4,d.width*4,gpu[slot].data()+(y*4+d.x)*4);d.clear();assert(gpu[slot]==flipped);};
 paint(full);upload(0);upload(1);
 source[(0*4+1)*4]=150;paint(top);assert(pending[0].x==1&&pending[0].y==2&&pending[0].width==1&&pending[0].height==1);upload(0);
 source[(1*4+2)*4]=200;paint(dirty);upload(1);upload(0);
 // Multiple CEF callbacks before a submission, clipping and missing dirty lists.
 source[0]=25;paint(clipped);source[(2*4+3)*4]=35;paint(Rect{3,2,1,1});upload(0);upload(1);
 PaintDamage bounds;bounds.add(4,3,0,static_cast<Rect*>(nullptr));assert(bounds.width==4&&bounds.height==3);bounds.clear();Rect outside{9,9,3,3};bounds.add(4,3,1,&outside);assert(bounds.empty());
}
