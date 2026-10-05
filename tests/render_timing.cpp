#include "render_timing.h"
#include <cassert>
#include <limits>
int main(){
 assert(browser_frame_rate(72)==72);assert(browser_frame_rate(90)==90);assert(browser_frame_rate(120)==120);
 assert(browser_frame_rate(0)==90);assert(browser_frame_rate(-1)==90);assert(browser_frame_rate(std::numeric_limits<float>::quiet_NaN())==90);
 assert(browser_frame_rate(30)==60);assert(browser_frame_rate(1000)==144);
}
