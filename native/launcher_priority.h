#pragma once
#include <algorithm>
#include <cerrno>
#include <chrono>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <map>
#include <sstream>
#include <sys/resource.h>
#include <sys/stat.h>
#include <unistd.h>

// Only the native renderer and its CEF children participate. Android, terminal
// shells and desktop applications belong to the session agent, not this tree.
class LauncherPriority {
 struct Task { unsigned long long born; int nice; };
 struct Process { int parent; unsigned long long born; };
 std::map<int,Task> saved;
 bool active=false,warned=false;
 int baseline=getpriority(PRIO_PROCESS,0);
 std::chrono::steady_clock::time_point scanned{};
 static bool stat(int id,Process& result){
  std::ifstream f("/proc/"+std::to_string(id)+"/stat");std::string line;std::getline(f,line);
  auto end=line.rfind(')');if(end==std::string::npos)return false;
  std::istringstream s(line.substr(end+2));std::string value;
  for(int field=3;field<=22;field++){if(!(s>>value))return false;try{if(field==4)result.parent=std::stoi(value);if(field==22)result.born=std::stoull(value);}catch(...){return false;}}
  return true;
 }
 public:
 static int target(int original){return std::min(original,-5);}
 void update(bool enabled){
  auto now=std::chrono::steady_clock::now();if(enabled==active&&(!enabled||now-scanned<std::chrono::seconds(1)))return;
  const bool continuing=active;
  // Include threads born since the last scan before restoring their inherited priority.
  if(!enabled&&active){scanned={};update(true);}
  active=enabled;scanned=now;
  if(!enabled){for(auto [id,task]:saved){Process p{};if(stat(id,p)&&p.born==task.born)setpriority(PRIO_PROCESS,id,task.nice);}saved.clear();return;}
  try {
  std::error_code ec;auto exe=std::filesystem::read_symlink("/proc/self/exe",ec);if(ec)return;
  std::map<int,Process> processes;
  for(auto& entry:std::filesystem::directory_iterator("/proc",ec)){
   const auto name=entry.path().filename().string();if(name.empty()||name.find_first_not_of("0123456789")!=std::string::npos)continue;
   struct ::stat info{};if(::stat(entry.path().c_str(),&info)||info.st_uid!=geteuid())continue;
   if(std::filesystem::read_symlink(entry.path()/"exe",ec)!=exe){ec.clear();continue;}
   Process p{};int id=std::stoi(name);if(stat(id,p))processes[id]=p;
  }
  for(auto [pid,process]:processes){
   int parent=pid;for(int i=0;i<32&&parent!=getpid();i++){auto p=processes.find(parent);if(p==processes.end()){parent=0;break;}parent=p->second.parent;}
   if(parent!=getpid())continue;
   for(auto& entry:std::filesystem::directory_iterator("/proc/"+std::to_string(pid)+"/task",ec)){
    int id;try{id=std::stoi(entry.path().filename().string());}catch(...){continue;}
    Process p{};if(!stat(id,p))continue;
    auto old=saved.find(id);if(old!=saved.end()&&old->second.born!=p.born){saved.erase(old);old=saved.end();}
    if(old==saved.end()){errno=0;int nice=getpriority(PRIO_PROCESS,id);if(errno)continue;
     // CEF threads created during a boost inherit it; restore the baseline.
     if(continuing&&nice==target(baseline))nice=baseline;
     old=saved.emplace(id,Task{p.born,nice}).first;
    }
    if(setpriority(PRIO_PROCESS,id,target(old->second.nice))&&!warned&&errno!=ESRCH){warned=true;std::cerr<<"Launcher CPU priority unavailable (requires service LimitNICE=25)\n";}
   }
  }
  for(auto it=saved.begin();it!=saved.end();){Process p{};if(!stat(it->first,p)||p.born!=it->second.born)it=saved.erase(it);else++it;}
  } catch(const std::filesystem::filesystem_error&) { /* Child may exit during enumeration; retry next tick. */ }
 }
 ~LauncherPriority(){update(false);}
};
