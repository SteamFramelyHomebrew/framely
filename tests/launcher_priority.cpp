#include "launcher_priority.h"
#include <cassert>
#include <thread>

int main(){
 assert(LauncherPriority::target(0)==-5);
 assert(LauncherPriority::target(19)==-5);
 assert(LauncherPriority::target(-10)==-10);
 const int original=getpriority(PRIO_PROCESS,0);
 {
  LauncherPriority priority;priority.update(true);
  // Without RLIMIT_NICE the boost fails safely; with it, verify the bounded boost.
  int boosted=getpriority(PRIO_PROCESS,0);
  assert(boosted==original||boosted==LauncherPriority::target(original));
  std::thread newborn([]{std::this_thread::sleep_for(std::chrono::milliseconds(150));});
  priority.update(false);
  assert(getpriority(PRIO_PROCESS,0)==original);
  newborn.join();
 }
 assert(getpriority(PRIO_PROCESS,0)==original);
}
