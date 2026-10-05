#include "render_timing.h"
#include <cassert>
#include <limits>
int main(){
 assert(browser_frame_rate(72)==60);assert(browser_frame_rate(90)==60);assert(browser_frame_rate(120)==60);
 assert(browser_frame_rate(0)==60);assert(browser_frame_rate(-1)==60);assert(browser_frame_rate(std::numeric_limits<float>::quiet_NaN())==60);
 assert(browser_frame_rate(30)==60);assert(browser_frame_rate(1000)==60);
}
