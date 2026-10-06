#include "launcher_fade.h"
#include <cassert>
#include <cmath>
int main(){
 using namespace std::chrono;auto now=steady_clock::now();LauncherFade fade;
 // Activation/first paint may be late: the fade must not elapse offscreen.
 fade.open(now);assert(fade.value(now)==0);assert(fade.value(now+seconds(2))==0);
 const auto visible=now+seconds(2);fade.ready(visible);
 assert(fade.value(visible)==0);
 assert(std::abs(fade.value(visible+milliseconds(160))-.875f)<.001f);
 fade.ready(visible+milliseconds(160)); // Repeated visibility does not restart it.
 assert(fade.value(visible+milliseconds(320))==1);
 // Closing during entry remains continuous; interrupted close resets completely.
 auto interrupted=visible+milliseconds(160);fade.close(interrupted);
 assert(std::abs(fade.value(interrupted)-.875f)<.001f);
 assert(std::abs(fade.value(interrupted+milliseconds(120))-.4375f)<.001f);
 assert(fade.value(interrupted+milliseconds(240))==0);
 fade.reset();assert(fade.value(interrupted)==0);assert(!fade.waiting);
 fade.open(interrupted);assert(fade.value(interrupted)==0);
 fade.ready(interrupted);assert(fade.value(interrupted)==0);
 fade.reduced=true;assert(fade.value(interrupted)==1);
 fade.close(interrupted);assert(fade.value(interrupted)==0);
 // Cancelling before activation never produces a late fade or a visible flash.
 fade.open(now);fade.close(now+milliseconds(100));assert(fade.value(now+milliseconds(100))==0);
 fade.reset();fade.ready(now+seconds(3));assert(fade.value(now+seconds(3))==0);
}
