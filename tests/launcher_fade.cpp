#include "launcher_fade.h"
#include <cassert>
#include <cmath>
int main(){
 using namespace std::chrono;auto now=steady_clock::now();LauncherFade fade;
 fade.open(now);assert(fade.value(now)==0);assert(std::abs(fade.value(now+milliseconds(290))-.5f)<.001f);assert(fade.value(now+milliseconds(580))==1);
 // Closing during entry is continuous; reopening starts a new entry.
 auto interrupted=now+milliseconds(290);fade.close(interrupted);assert(std::abs(fade.value(interrupted)-.5f)<.001f);assert(std::abs(fade.value(interrupted+milliseconds(220))-.25f)<.001f);assert(fade.value(interrupted+milliseconds(440))==0);
 fade.open(interrupted);assert(fade.value(interrupted)==0);fade.reduced=true;assert(fade.value(interrupted)==1);fade.close(interrupted);assert(fade.value(interrupted)==0);
}
