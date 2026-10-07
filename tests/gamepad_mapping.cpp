#include "gamepad_mapping.h"
#include <cassert>
#include <fstream>
#include <sstream>
#include <map>
#include <string>
int main(){
 // Android Vendor_045e_Product_028e.kl scan codes, independent of Linux aliases.
 const int androidXbox[]={304,305,307,308,310,311,314,315,317,318};
 static_assert(sizeof(keys)==sizeof(androidXbox));
 for(int i=0;i<10;i++)assert(keys[i]==androidXbox[i]);
 // The custom host identity must retain Android's expected ABXY/axis layout.
 std::ifstream file("native/input/gamepad/Vendor_0001_Product_f001.kl");assert(file);std::map<int,std::string> entries;std::string line;
 while(std::getline(file,line)){std::istringstream row(line);std::string kind,name;int code;if(row>>kind>>code>>name&&kind=="key")entries[code]=name;}
 const char* names[]={"BUTTON_A","BUTTON_B","BUTTON_X","BUTTON_Y","BUTTON_L1","BUTTON_R1","BUTTON_SELECT","BUTTON_START","BUTTON_THUMBL","BUTTON_THUMBR"};
 for(int i=0;i<10;i++)assert(entries[keys[i]]==names[i]);
 assert(trigger_keys[0]==312&&trigger_keys[1]==313);
 assert(entries[trigger_keys[0]]=="BUTTON_L2"&&entries[trigger_keys[1]]=="BUTTON_R2");
 GamepadTriggers triggers;
 assert(!triggers.update(0,0));assert(!triggers.update(0,203));
 assert(triggers.update(0,204));assert(triggers.update(0,192));
 assert(!triggers.down[1]);assert(triggers.update(1,255));
 assert(!triggers.update(0,191));assert(triggers.down[1]);
 assert(!triggers.update(1,0));assert(!triggers.down[0]&&!triggers.down[1]);
 assert(triggers.configure(50));assert(!triggers.update(0,127));assert(triggers.update(0,128));
 assert(triggers.update(0,115));assert(!triggers.update(0,114));
 // Changing the setting recalculates a held trigger without axis movement.
 assert(triggers.update(0,180));assert(triggers.configure(80));assert(!triggers.update(0,180));
 assert(triggers.configure(60));assert(triggers.update(0,180));assert(!triggers.update(0,0));
 assert(triggers.configure(1));assert(!triggers.update(0,2));assert(triggers.update(0,3));assert(!triggers.update(0,0));
 assert(triggers.configure(100));assert(!triggers.update(1,254));assert(triggers.update(1,255));assert(!triggers.update(1,242));
 assert(!triggers.configure(0));assert(!triggers.configure(101));assert(triggers.press==255);
}
