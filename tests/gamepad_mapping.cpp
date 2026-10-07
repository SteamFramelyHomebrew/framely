#include "gamepad_mapping.h"
#include <cassert>
int main(){
 // Android Vendor_045e_Product_028e.kl scan codes, independent of Linux aliases.
 const int androidXbox[]={304,305,307,308,310,311,314,315,317,318};
 static_assert(sizeof(keys)==sizeof(androidXbox));
 for(int i=0;i<10;i++)assert(keys[i]==androidXbox[i]);
}
