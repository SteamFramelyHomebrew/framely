// Session-owned Steam Input/OpenVR -> Android bridge with reverse rumble.
#include "openvr.h"
#include <linux/uinput.h>
#include <fcntl.h>
#include <unistd.h>
#include <poll.h>
#include <signal.h>
#include <filesystem>
#include <cstdio>
#include <cstring>
#include <cerrno>
#include <cmath>
#include <algorithm>
#include <string>
#include <chrono>
#include <thread>
static volatile sig_atomic_t stopped=0;
static void stop(int){stopped=1;}
#include "gamepad_mapping.h"
#include "gamepad_steam.h"
#include "gamepad_feedback.h"
static const int axes[]={ABS_X,ABS_Y,ABS_RX,ABS_RY,ABS_Z,ABS_RZ,ABS_HAT0X,ABS_HAT0Y};
static bool event(int fd,int type,int code,int value){input_event e{};e.type=type;e.code=code;e.value=value;return write(fd,&e,sizeof e)==sizeof e;}
static bool send(int fd,const int* state){static GamepadTriggers triggers;bool ok=true;for(int k=0;k<10;k++)ok=event(fd,EV_KEY,keys[k],state[k])&&ok;for(int k=0;k<2;k++)ok=event(fd,EV_KEY,trigger_keys[k],triggers.update(k,state[14+k]))&&ok;for(int k=0;k<8;k++)ok=event(fd,EV_ABS,axes[k],state[10+k])&&ok;return event(fd,EV_SYN,SYN_REPORT,0)&&ok;}
int main(int argc,char**argv){
 if((argc!=2&&argc!=4)||geteuid()==0){fprintf(stderr,"Run gamepad bridge as the Steam session user with an action manifest.\n");return 1;}
 // A session request runs on a short-lived worker thread. PR_SET_PDEATHSIG
 // follows that thread's lifetime, so it destroys the pad as soon as prepare
 // returns. The session-owned stdin pipe and parent process own this helper.
 const auto parent=getppid();
 const bool steamMode=std::getenv("FRAMELY_GAMEPAD_SOURCE")&&std::string(std::getenv("FRAMELY_GAMEPAD_SOURCE"))=="steam";
 SteamGamepad steam;
 if(steamMode&&!steam.start(std::getenv("FRAMELY_STEAM_SDL_LIBRARY"),std::getenv("FRAMELY_STEAM_GAMEPAD_INFO"))){fprintf(stderr,"Steam Input SDL3 runtime unavailable; select Frame direct input in APK settings.\n");return 1;}
 bool rumbleEnabled=!std::getenv("FRAMELY_GAMEPAD_RUMBLE")||std::string(std::getenv("FRAMELY_GAMEPAD_RUMBLE"))!="0";
 auto milliseconds=[](){return uint64_t(std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now().time_since_epoch()).count());};
 signal(SIGTERM,stop);signal(SIGINT,stop);signal(SIGPIPE,SIG_IGN);
 vr::EVRInitError error=vr::VRInitError_None;if(!steamMode)vr::VR_Init(&error,vr::VRApplication_Overlay);if(error){fprintf(stderr,"SteamVR gamepad input unavailable: %d\n",error);return 1;}
 // Different live containers must not share SteamVR's generated executable
 // identity. Otherwise a second helper replaces the first app's registered PID.
 bool registered=false;
 // Steam Input already represents Steam's configured output. Registering an
 // independent OpenVR action app here suppresses SDL's virtual-gamepad
 // enumeration in this process on Frame.
 auto shutdown=[&](){if(!steamMode){if(registered)vr::VRApplications()->RemoveApplicationManifest(argv[2]);vr::VR_Shutdown();}};
 if(!steamMode&&argc==4){
  if(vr::VRApplications()->AddApplicationManifest(argv[2],true)!=vr::VRApplicationError_None){fprintf(stderr,"Gamepad application registration failed.\n");shutdown();return 1;}
  registered=true;
  auto identity=vr::VRApplications()->IdentifyApplication(getpid(),argv[3]);if(identity!=vr::VRApplicationError_None){fprintf(stderr,"Gamepad application identity failed: %d (%s).\n",identity,vr::VRApplications()->GetApplicationsErrorNameFromEnum(identity));shutdown();return 1;}
 }
 auto*input=steamMode?nullptr:vr::VRInput();vr::VRActiveActionSet_t set{};
 bool valid=steamMode||(input->SetActionManifestPath(argv[1])==vr::VRInputError_None&&input->GetActionSetHandle("/actions/framely_gamepad",&set.ulActionSet)==vr::VRInputError_None);
 const char*names[]={"right_a","right_b","right_x","right_y","left_bumper","right_bumper","left_menu","right_menu","left_stickclick","right_stickclick","left_thumbstick","right_thumbstick","left_trigger","right_trigger","left_dpad_left","left_dpad_right","left_dpad_up","left_dpad_down"};
 vr::VRActionHandle_t handles[18]{};
 for(int k=0;!steamMode&&k<18;k++)valid= input->GetActionHandle((std::string("/actions/framely_gamepad/in/")+names[k]).c_str(),&handles[k])==vr::VRInputError_None&&valid;
 if(!valid){fprintf(stderr,"Invalid gamepad action bindings.\n");shutdown();return 1;}
 vr::VRActionHandle_t haptics[2]{};
 for(int k=0;!steamMode&&k<2;k++)input->GetActionHandle(k?"/actions/framely_gamepad/out/right_haptic":"/actions/framely_gamepad/out/left_haptic",&haptics[k]);
 int fd=open("/dev/uinput",O_RDWR|O_NONBLOCK|O_CLOEXEC);if(fd<0){perror("Gamepad /dev/uinput");shutdown();return 1;}
 bool ok=ioctl(fd,UI_SET_EVBIT,EV_KEY)==0&&ioctl(fd,UI_SET_EVBIT,EV_ABS)==0&&ioctl(fd,UI_SET_EVBIT,EV_FF)==0&&ioctl(fd,UI_SET_FFBIT,FF_RUMBLE)==0;
 for(auto key:keys)ok=ioctl(fd,UI_SET_KEYBIT,key)==0&&ok;
 for(auto key:trigger_keys)ok=ioctl(fd,UI_SET_KEYBIT,key)==0&&ok;
 for(int k=0;k<8;k++){ok=ioctl(fd,UI_SET_ABSBIT,axes[k])==0&&ok;uinput_abs_setup a{};a.code=axes[k];a.absinfo.minimum=k<4?-32768:(k<6?0:-1);a.absinfo.maximum=k<4?32767:(k<6?255:1);a.absinfo.flat=k<4?1024:0;ok=ioctl(fd,UI_ABS_SETUP,&a)==0&&ok;}
 uinput_setup device{};device.id={BUS_VIRTUAL,framely_pad_vendor,framely_pad_product,0x0001};device.ff_effects_max=GamepadFeedback::capacity;strcpy(device.name,"Framely Android gamepad");
 ok=ioctl(fd,UI_DEV_SETUP,&device)==0&&ioctl(fd,UI_DEV_CREATE)==0&&ok;
 char sysname[128]{};ok=ioctl(fd,UI_GET_SYSNAME(sizeof sysname),sysname)>=0&&ok;
 std::string node;
 for(int tries=0;ok&&tries<100&&node.empty();tries++){std::error_code ec;for(auto&entry:std::filesystem::directory_iterator(std::string("/sys/class/input/")+sysname,ec)){auto name=entry.path().filename().string();if(name.rfind("event",0)==0)node="/dev/input/"+name;}if(node.empty())std::this_thread::sleep_for(std::chrono::milliseconds(20));}
 if(!ok||node.empty()){fprintf(stderr,"Could not create the virtual gamepad.\n");ioctl(fd,UI_DEV_DESTROY);close(fd);shutdown();return 1;}
 int witness=-1;for(int tries=0;tries<100&&witness<0;tries++){witness=open(node.c_str(),O_RDONLY|O_NONBLOCK|O_CLOEXEC);if(witness<0)std::this_thread::sleep_for(std::chrono::milliseconds(20));}
 if(witness<0){perror("Gamepad event access");ioctl(fd,UI_DEV_DESTROY);close(fd);shutdown();return 1;}
 printf("%s\n",node.c_str());fflush(stdout);
 // Parent owns routing. Start neutral; EOF, disconnect or disabled actions release input.
 bool enabled=false;std::string commands;int previous[18]{};GamepadFeedback feedback;std::array<uint16_t,2> lastRumble{};uint64_t rumbleAt=0;uint32_t source=0;bool frameConnected[2]{};
 while(!stopped&&getppid()==parent){pollfd p{STDIN_FILENO,POLLIN,0};if(poll(&p,1,0)>0){char buffer[128];auto count=read(0,buffer,sizeof buffer);if(count<=0)break;commands.append(buffer,count);size_t at;while((at=commands.find('\n'))!=std::string::npos){auto command=commands.substr(0,at);commands.erase(0,at+1);if(command=="enable")enabled=true;else if(command=="disable")enabled=false;else if(command=="rumble-on")rumbleEnabled=true;else if(command=="rumble-off")rumbleEnabled=false;}if(commands.size()>128)break;}
  const auto now=milliseconds();
  // Android may upload effects even while unfocused. Always acknowledge the
  // kernel requests; only playback is gated by foreground ownership.
  for(int k=0;k<64;k++){input_event e{};if(read(fd,&e,sizeof e)!=sizeof e)break;
   if(e.type==EV_UINPUT&&e.code==UI_FF_UPLOAD){uinput_ff_upload q{};q.request_id=e.value;if(ioctl(fd,UI_BEGIN_FF_UPLOAD,&q)==0){q.retval=feedback.upload(q.effect);ioctl(fd,UI_END_FF_UPLOAD,&q);}}
   else if(e.type==EV_UINPUT&&e.code==UI_FF_ERASE){uinput_ff_erase q{};q.request_id=e.value;if(ioctl(fd,UI_BEGIN_FF_ERASE,&q)==0){q.retval=feedback.erase(q.effect_id);ioctl(fd,UI_END_FF_ERASE,&q);}}
   else if(e.type==EV_FF)feedback.play(e.code,e.value,now);
  }
  int state[18]{};bool inputAvailable=steamMode;
  // If Android releases its exclusive claim (e.g. InputReader restart),
  // pause until it claims the node again. Do not feed other host consumers.
  bool claimed=false;if(enabled){if(ioctl(witness,EVIOCGRAB,1)==0)ioctl(witness,EVIOCGRAB,0);else claimed=errno==EBUSY;}
  if(steamMode){auto next=steam.read(now,enabled&&claimed);std::copy(next.begin(),next.end(),state);if(source!=steam.source()){feedback.stop();source=steam.source();lastRumble={};}}
  if(!steamMode&&enabled&&claimed&&input->UpdateActionState(&set,sizeof set,1)==vr::VRInputError_None){
   const auto leftIndex=vr::VRSystem()->GetTrackedDeviceIndexForControllerRole(vr::TrackedControllerRole_LeftHand);
   const auto rightIndex=vr::VRSystem()->GetTrackedDeviceIndexForControllerRole(vr::TrackedControllerRole_RightHand);
   const bool left=leftIndex!=vr::k_unTrackedDeviceIndexInvalid&&vr::VRSystem()->IsTrackedDeviceConnected(leftIndex);
   const bool right=rightIndex!=vr::k_unTrackedDeviceIndexInvalid&&vr::VRSystem()->IsTrackedDeviceConnected(rightIndex);
   if((frameConnected[0]&&!left)||(frameConnected[1]&&!right))feedback.stop();frameConnected[0]=left;frameConnected[1]=right;inputAvailable=left||right;
   bool buttons[18]{};for(int k=0;k<18;k++){if(k>=10&&k<=13)continue;vr::InputDigitalActionData_t data{};if(input->GetDigitalActionData(handles[k],&data,sizeof data,vr::k_ulInvalidInputValueHandle)==vr::VRInputError_None)buttons[k]=(k==4||k==6||k==8||k>=14?left:right)&&data.bActive&&data.bState;}
   for(int k=0;k<10;k++)state[k]=buttons[k];
   for(int k=10;k<=13;k++){if(!(k==10||k==12?left:right))continue;vr::InputAnalogActionData_t data{};if(input->GetAnalogActionData(handles[k],&data,sizeof data,vr::k_ulInvalidInputValueHandle)!=vr::VRInputError_None||!data.bActive||!std::isfinite(data.x)||!std::isfinite(data.y))continue;if(k<=11){state[10+(k-10)*2]=std::lround(std::clamp(data.x,-1.f,1.f)*32767);state[11+(k-10)*2]=std::lround(std::clamp(-data.y,-1.f,1.f)*32767);}else state[14+k-12]=std::lround(std::clamp(data.x,0.f,1.f)*255);}
   state[16]=int(buttons[15])-int(buttons[14]);state[17]=int(buttons[17])-int(buttons[16]);
  }
  const auto vibration=feedback.value(now,enabled&&claimed&&rumbleEnabled&&inputAvailable);
  if(vibration!=lastRumble||((vibration[0]||vibration[1])&&now-rumbleAt>=100)){
   if(steamMode)steam.rumble(vibration[0],vibration[1]);
   lastRumble=vibration;rumbleAt=now;
  }
  if(!steamMode&&(vibration[0]||vibration[1]))for(int k=0;k<2;k++)if(haptics[k]!=vr::k_ulInvalidActionHandle&&vibration[k])input->TriggerHapticVibrationAction(haptics[k],0,.012f,100.f,float(vibration[k])/65535.f,vr::k_ulInvalidInputValueHandle);
  if(memcmp(previous,state,sizeof state)){if(!send(fd,state))break;memcpy(previous,state,sizeof state);}
  std::this_thread::sleep_for(std::chrono::milliseconds(8));
 }
 steam.stop();feedback.stop();int neutral[18]{};send(fd,neutral);close(witness);ioctl(fd,UI_DEV_DESTROY);close(fd);shutdown();return 0;
}
