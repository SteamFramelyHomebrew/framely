#include "keyboard_state.h"
#include <cassert>
int main(){
 assert(!keyboard_edit_source(0,42));assert(!keyboard_edit_source(43,42));
 assert(keyboard_edit_source(42,42));assert(!keyboard_edit_source(0,0));
 KeyboardMenuGuard guard;auto now=KeyboardMenuGuard::Clock::now();
 assert(!guard.holds(now));guard.begin(true);assert(guard.holds(now+std::chrono::seconds(10)));
 guard.end(now);assert(guard.holds(now));assert(guard.holds(now+std::chrono::milliseconds(349)));
 assert(!guard.holds(now+std::chrono::milliseconds(351)));
 guard.begin(false);assert(!guard.holds(now));guard.end(now);assert(!guard.holds(now));
}
