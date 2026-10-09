#pragma once
#include <gst/gst.h>
#include <gst/app/gstappsink.h>
#include <gst/video/video.h>
#include <sys/file.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <fcntl.h>
#include <unistd.h>
#include <cstring>
#include <string>
#include <algorithm>
#include <vector>
#include <atomic>
// A frame file is written under an advisory lock. Readers never see a partial frame.
struct FrameSink {
 std::string directory,id,protocol; std::atomic<bool> announced{false}; std::atomic<unsigned> width{0},height{0};
 FrameSink(std::string d,std::string i,std::string p):directory(d),id(i),protocol(p){}
 void event(const std::string& type,unsigned w=0,unsigned h=0,const std::string& media=""){
  int fd=socket(AF_UNIX,SOCK_DGRAM|SOCK_CLOEXEC,0); if(fd<0)return;
  sockaddr_un a{};a.sun_family=AF_UNIX; auto path=directory+"/events.sock";if(path.size()>=sizeof(a.sun_path)){close(fd);return;}strcpy(a.sun_path,path.c_str());
  auto msg="{\"event\":\""+type+"\",\"id\":\""+id+"\",\"protocol\":\""+protocol+"\",\"width\":"+std::to_string(w)+",\"height\":"+std::to_string(h)+",\"mediaType\":\""+media+"\"}";
  sendto(fd,msg.data(),msg.size(),MSG_DONTWAIT,(sockaddr*)&a,sizeof(a));close(fd);
 }
 bool accepted() const {return access((directory+"/"+id+".accept").c_str(),F_OK)==0;}
 GstFlowReturn sample(GstAppSink* sink,bool preroll=false){
  auto* sample=preroll?gst_app_sink_pull_preroll(sink):gst_app_sink_pull_sample(sink);if(!sample)return GST_FLOW_EOS;
  GstVideoInfo info{};GstVideoFrame frame{};
  if(gst_video_info_from_caps(&info,gst_sample_get_caps(sample))&&GST_VIDEO_INFO_FORMAT(&info)==GST_VIDEO_FORMAT_BGRA&&gst_video_frame_map(&frame,&info,gst_sample_get_buffer(sample),GST_MAP_READ)){
   unsigned w=GST_VIDEO_INFO_WIDTH(&info),h=GST_VIDEO_INFO_HEIGHT(&info);
   if(w&&h&&w<=4096&&h<=4096&&uint64_t(w)*h<=16777216){
    bool resized=width.exchange(w)!=w;resized=(height.exchange(h)!=h)||resized;bool first=!announced.exchange(true);if(resized||first)event("video",w,h);
    if(accepted()){
     std::string path=directory+"/"+id+".frame";int fd=open(path.c_str(),O_CREAT|O_WRONLY|O_CLOEXEC|O_NOFOLLOW,0600);
     if(fd>=0){flock(fd,LOCK_EX);uint32_t header[]={0x46434153,w,h,w*4};bool ok=pwrite(fd,header,sizeof(header),0)==sizeof(header);
      for(unsigned y=0;ok&&y<h;y++){auto* row=(uint8_t*)GST_VIDEO_FRAME_PLANE_DATA(&frame,0)+y*GST_VIDEO_FRAME_PLANE_STRIDE(&frame,0);ok=pwrite(fd,row,w*4,16+size_t(y)*w*4)==ssize_t(w*4);}
      if(ok)ftruncate(fd,16+size_t(w)*h*4);flock(fd,LOCK_UN);close(fd);
     }
    }
   }
   gst_video_frame_unmap(&frame);
  }
  gst_sample_unref(sample);return GST_FLOW_OK;
 }
};
inline GstFlowReturn frame_sample(GstAppSink* sink,gpointer data){return static_cast<FrameSink*>(data)->sample(sink);}
inline GstFlowReturn frame_preroll(GstAppSink* sink,gpointer data){return static_cast<FrameSink*>(data)->sample(sink,true);}
