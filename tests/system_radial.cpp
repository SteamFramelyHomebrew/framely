#include "system_radial.h"
#include <cassert>
#include <iostream>
int main(){
 using Clock=SystemRadial::Clock;
 auto start=Clock::now();
 SystemRadial menu;
 assert(menu.update(0,true,true,0,0,start)==SystemRadialEvent::Begin);
 assert(menu.update(0,true,false,0,0,start+std::chrono::milliseconds(120))==SystemRadialEvent::Cancel);
 assert(menu.update(1,true,true,0,0,start+std::chrono::seconds(2))==SystemRadialEvent::Begin);
 assert(menu.update(1,true,true,0,0,start+std::chrono::seconds(3))==SystemRadialEvent::Open);
 assert(menu.update(1,true,true,0,.15f,start+std::chrono::milliseconds(3100))==SystemRadialEvent::Change);
 assert(menu.direction==SystemRadialDirection::Up);
 assert(menu.update(1,true,false,0,.15f,start+std::chrono::milliseconds(3200))==SystemRadialEvent::Commit);
 assert(SystemRadial::select(.2f,0)==SystemRadialDirection::Right);
 assert(SystemRadial::select(-.2f,0)==SystemRadialDirection::Left);
 assert(SystemRadial::select(0,-.2f)==SystemRadialDirection::Down);
 assert(SystemRadial::select(.03f,.01f)==SystemRadialDirection::Center);
 assert(SystemRadial::hit(475,319)==SystemRadialDirection::Right);
 assert(SystemRadial::hit(599,401)==SystemRadialDirection::Right);
 assert(SystemRadial::hit(360,183)==SystemRadialDirection::Up);
 assert(SystemRadial::hit(183,360)==SystemRadialDirection::Left);
 assert(SystemRadial::hit(360,537)==SystemRadialDirection::Down);
 assert(!SystemRadial::hit(470,315));
 SystemRadial pointer;
 assert(pointer.update(0,true,true,0,0,start)==SystemRadialEvent::Begin);
 assert(pointer.update(0,true,true,0,0,start+std::chrono::seconds(1))==SystemRadialEvent::Open);
 assert(pointer.update(0,true,true,0,0,start+std::chrono::milliseconds(1050),SystemRadial::hit(475,319))==SystemRadialEvent::Change);
 assert(pointer.direction==SystemRadialDirection::Right);
 std::cout<<"system radial: passed\n";
}
