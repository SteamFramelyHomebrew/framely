#include "cast_window_geometry.h"
#include <cassert>
#include <iostream>
#include <limits>
struct Overlay {
 float width;bool fail=false;int reads=0;
 int GetOverlayWidthInMeters(int,float* value){++reads;if(fail)return 1;*value=width;return 0;}
};
static void near(float actual,float expected){assert(std::abs(actual-expected)<.00001f);}
int main(){
 for(auto size:{std::pair{1920,1080},std::pair{498,1080},std::pair{640,240}})
  near(cast_window_initial_width(size.first,size.second)*size.second/size.first,1.8f);
 Overlay first{cast_window_initial_width(1920,1080)},second{cast_window_initial_width(498,1080)};
 // Preserve a user's scaled height through repeated rotations.
 first.width*=.6f;
 for(int n=0;n<5;++n){
  auto portrait=cast_window_resized_width(&first,1,1920,1080,1080,1920);assert(portrait);
  first.width=*portrait;near(first.width*1920/1080,1.08f);
  auto landscape=cast_window_resized_width(&first,1,1080,1920,1920,1080);assert(landscape);
  first.width=*landscape;near(first.width*1080/1920,1.08f);
 }
 near(second.width*1080/498,1.8f); // Independent windows.
 assert(!cast_window_aspect_changed(1920,1080,1280,720));
 assert(!cast_window_aspect_changed(498,1080,498,1080));
 assert(cast_window_aspect_changed(1920,1080,1080,1920));
 float saved=first.width;first.fail=true;
 assert(!cast_window_resized_width(&first,1,1920,1080,1080,1920));near(first.width,saved);
 first.fail=false;
 assert(cast_window_resized_width(&first,1,1920,1080,1080,1920));
 for(float invalid:{0.f,-1.f,std::numeric_limits<float>::infinity(),std::numeric_limits<float>::quiet_NaN()}){
  first.width=invalid;assert(!cast_window_resized_width(&first,1,1920,1080,1080,1920));
 }
 std::cout<<"PASS: initial cast height, user scaling, repeated rotation, independent windows and read failure recovery\n";
}
