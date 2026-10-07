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
}
