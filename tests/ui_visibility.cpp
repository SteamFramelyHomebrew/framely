#include "ui_visibility.h"
#include <cassert>
int main(){
 assert(!capture_view_visible(false,true,true,false,true));
 assert(!capture_view_visible(false,false,true,true,true));
 assert(!capture_view_visible(false,true,true,true,false));
 assert(capture_view_visible(false,true,true,true,true));
 assert(capture_view_visible(false,true,false,false,false));
 assert(!capture_view_visible(true,true,false,false,false));
}
