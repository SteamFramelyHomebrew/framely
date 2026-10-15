#include "framing_lifecycle.h"
#include <cassert>
#include <iostream>
int main(){
 using nlohmann::json;
 assert(!framing_editor_should_close(nullptr,"current"));
 assert(!framing_editor_should_close(json::object(),"current"));
 assert(!framing_editor_should_close({{"id","current"},{"open",true}},"current"));
 assert(framing_editor_should_close({{"id","current"},{"open",false}},"current"));
 assert(!framing_editor_should_close({{"id","old"},{"open",false}},"current"));
 assert(framing_editor_should_close({{"id","new"},{"open",true}},"current"));
 std::cout<<"PASS: persistent framing state repairs lost closes and ignores stale acknowledgements\n";
}
