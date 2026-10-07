#pragma once
// Steam's already-configured evdev output, without SDL or a second uinput pad.
#include "gamepad_steam.h"
#include <filesystem>
#include <sys/stat.h>
#include <chrono>
#include <poll.h>
#include "gamepad_direct_policy.h"
inline int direct_slot(const input_id& id,const char* name,const std::vector<SteamPadInfo>& rows){
 if(id.vendor!=0x28de||id.product!=0x11ff)return -1;
 const char* at=std::strstr(name,"pad ");int slot=-1;char extra;
 if(!at||std::sscanf(at+4,"%d%c",&slot,&extra)!=1||!allowed_steam_slot(rows,slot))return -1;
 return slot;
}
inline int steam_direct(const char* infoPath,const char* directory){
 if(!infoPath||!directory)return 1;
 std::ifstream info(infoPath);auto rows=read_steam_pad_info(info);
 std::string node;int slot=64;struct stat identity{};
 std::error_code ec;
 for(auto& entry:std::filesystem::directory_iterator("/dev/input",ec)){
  const auto name=entry.path().filename().string();if(name.rfind("event",0)!=0)continue;
  int fd=open(entry.path().c_str(),O_RDWR|O_NONBLOCK|O_CLOEXEC);if(fd<0)continue;
  input_id id{};char label[128]{};int next=-1;struct stat st{};
  if(ioctl(fd,EVIOCGID,&id)==0&&ioctl(fd,EVIOCGNAME(sizeof label),label)>0&&fstat(fd,&st)==0&&S_ISCHR(st.st_mode))next=direct_slot(id,label,rows);
  close(fd);if(next>=0&&next<slot){node=entry.path();slot=next;identity=st;}
 }
 if(node.empty()){fprintf(stderr,"No writable Steam Input output is available; wake the controller and retry, or select the bridge.\n");return 1;}
 const auto source=*std::find_if(rows.begin(),rows.end(),[&](auto p){return p.slot==slot;});
 const auto control=std::string(directory)+"/control";
 int ctl=open(control.c_str(),O_WRONLY|O_CREAT|O_EXCL|O_CLOEXEC,0644);if(ctl<0)return 1;
 if(fchmod(ctl,0644)!=0){close(ctl);return 1;}
 auto now=[](){return uint64_t(std::chrono::duration_cast<std::chrono::nanoseconds>(std::chrono::steady_clock::now().time_since_epoch()).count());};
 bool sourceValid=true;unsigned missing=0;bool enabled=false,rumble=!getenv("FRAMELY_GAMEPAD_RUMBLE")||std::string(getenv("FRAMELY_GAMEPAD_RUMBLE"))!="0";
 auto publish=[&](){FramelyDirectControl c{now()+2000000000ULL,uint32_t(enabled&&sourceValid),uint32_t(rumble)};return pwrite(ctl,&c,sizeof c,0)==sizeof c;};
 if(!publish()){close(ctl);return 1;}
 printf("%s\n",node.c_str());fflush(stdout);
 std::string commands;const auto parent=getppid();uint64_t scanned=0;
 while(!stopped&&getppid()==parent){
  pollfd p{STDIN_FILENO,POLLIN,0};int result=poll(&p,1,50);if(result<0&&errno!=EINTR)break;
  if(result>0&&(p.revents&(POLLIN|POLLHUP|POLLERR))){char buffer[128];auto n=read(0,buffer,sizeof buffer);if(n<=0)break;commands.append(buffer,n);size_t at;
   while((at=commands.find('\n'))!=std::string::npos){auto cmd=commands.substr(0,at);commands.erase(0,at+1);if(cmd=="enable")enabled=true;else if(cmd=="disable")enabled=false;else if(cmd=="rumble-on")rumble=true;else if(cmd=="rumble-off")rumble=false;}
   if(commands.size()>128)break;
  }
  if(now()-scanned>=500000000ULL){scanned=now();struct stat current{};std::ifstream file(infoPath);auto latest=read_steam_pad_info(file);
   if(stat(node.c_str(),&current)!=0||current.st_ino!=identity.st_ino||current.st_rdev!=identity.st_rdev){
    fprintf(stderr,"Steam Input output changed/disconnected; restart this container to reconnect.\n");break;
   }
   sourceValid=std::any_of(latest.begin(),latest.end(),[&](auto p){return p.slot==source.slot&&p.identified&&p.vendor==source.vendor&&p.product==source.product;});
   if(sourceValid)missing=0;else if(++missing>=6){fprintf(stderr,"Steam Input controller ownership changed; restart this container to reconnect.\n");break;}
  }
  if(!publish())break;
 }
 enabled=false;rumble=false;publish();close(ctl);return 0;
}
