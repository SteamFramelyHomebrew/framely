#include <gst/gst.h>
#include <gst/app/gstappsrc.h>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <thread>
#include <chrono>
#include <cassert>
#include <cstdint>
#include <unistd.h>
static void pump(int ms){auto end=std::chrono::steady_clock::now()+std::chrono::milliseconds(ms);while(std::chrono::steady_clock::now()<end){while(g_main_context_iteration(nullptr,FALSE)){}std::this_thread::sleep_for(std::chrono::milliseconds(2));}}
static unsigned first_pixel(const std::string& path){std::ifstream file(path,std::ios::binary);file.seekg(16);return unsigned(file.get());}
int main(int argc,char** argv){
 char root[]="/tmp/framely-consent-test-XXXXXX";assert(mkdtemp(root));
 const std::string directory=root,id="0123456789abcdef0123456789abcdef",frame=directory+"/"+id+".frame",accept=directory+"/"+id+".accept";
 setenv("FRAMELY_CAST_DIR",root,1);setenv("FRAMELY_CAST_ID",id.c_str(),1);gst_init(nullptr,nullptr);
 for(int cycle=0;cycle<12;cycle++){
  std::filesystem::remove(frame);std::filesystem::remove(accept);
  GError* error=nullptr;
  auto* pipeline=gst_parse_launch("appsrc name=source is-live=true format=time caps=video/x-raw,format=BGRA,width=64,height=64,framerate=30/1 ! framelyvideosink name=receiver sync=false",&error);
  assert(pipeline&&!error);auto* source=gst_bin_get_by_name(GST_BIN(pipeline),"source");
  auto* receiver=gst_bin_get_by_name(GST_BIN(pipeline),"receiver");gpointer alive=receiver;g_object_add_weak_pointer(G_OBJECT(receiver),&alive);gst_object_unref(receiver);
  gst_element_set_state(pipeline,GST_STATE_PLAYING);
  auto push=[&](unsigned byte,GstClockTime pts){auto* buffer=gst_buffer_new_allocate(nullptr,64*64*4,nullptr);gst_buffer_memset(buffer,0,byte,64*64*4);GST_BUFFER_PTS(buffer)=pts;assert(gst_app_src_push_buffer(GST_APP_SRC(source),buffer)==GST_FLOW_OK);};
  push(77,0);pump(150);assert(!std::filesystem::exists(frame));
  std::ofstream(accept).close();pump(150);
  assert(std::filesystem::exists(frame)&&"accepting a static mirror must publish its cached frame without another buffer");assert(first_pixel(frame)==77);
  auto stamp=std::filesystem::last_write_time(frame);pump(100);assert(std::filesystem::last_write_time(frame)==stamp&&"steady consent must not rewrite a cached frame each timer tick");
  std::filesystem::remove(accept);pump(50);push(123,GST_SECOND);pump(100);assert(first_pixel(frame)==77);
  std::ofstream(accept).close();pump(150);assert(first_pixel(frame)==123&&"resume must show the newest frame even if sender is stationary");
  gst_element_set_state(pipeline,GST_STATE_NULL);gst_object_unref(source);gst_object_unref(pipeline);pump(20);
  assert(!alive&&"consent timer must release the video sink on teardown");
 }
 if(argc>1){
  // Continue the accepted session into independently fetched external video.
  auto* player=gst_element_factory_make("playbin3",nullptr);
  auto* video=gst_element_factory_make("framelyvideosink",nullptr);
  auto* audio=gst_element_factory_make("fakesink",nullptr);
  struct Playback {GstElement* player;bool paused=false;bool error=false;} playback{player};
  auto* watch_bus=gst_element_get_bus(player);
  guint watch=gst_bus_add_watch(watch_bus,[](GstBus*,GstMessage* message,gpointer user)->gboolean{
   auto* playback=static_cast<Playback*>(user);
   if(GST_MESSAGE_TYPE(message)==GST_MESSAGE_BUFFERING){int percent=0;gst_message_parse_buffering(message,&percent);gst_element_set_state(playback->player,percent<100||playback->paused?GST_STATE_PAUSED:GST_STATE_PLAYING);}
   else if(GST_MESSAGE_TYPE(message)==GST_MESSAGE_CLOCK_LOST&&!playback->paused){gst_element_set_state(playback->player,GST_STATE_PAUSED);gst_element_set_state(playback->player,GST_STATE_PLAYING);}
   else if(GST_MESSAGE_TYPE(message)==GST_MESSAGE_ERROR){GError* error=nullptr;gchar* debug=nullptr;gst_message_parse_error(message,&error,&debug);std::cerr<<error->message<<"\n";g_clear_error(&error);g_free(debug);playback->error=true;}
   return G_SOURCE_CONTINUE;
  },&playback);
  assert(player&&video&&audio);g_object_set(audio,"sync",TRUE,"async",FALSE,nullptr);g_object_set(player,"uri",argv[1],"video-sink",video,"audio-sink",audio,nullptr);
  std::filesystem::remove(frame);
  assert(gst_element_set_state(player,GST_STATE_PLAYING)!=GST_STATE_CHANGE_FAILURE);
  for(int n=0;n<120&&!std::filesystem::exists(frame);n++)pump(50);
  assert(std::filesystem::exists(frame)&&"external HLS video must produce decoded frames");
  uint32_t header[4]{};{std::ifstream f(frame,std::ios::binary);f.read(reinterpret_cast<char*>(header),16);}
  assert(header[1]==320&&header[2]==240&&"external video must replace the mirror dimensions");
  auto* bus=gst_element_get_bus(player);auto* message=gst_bus_pop_filtered(bus,GST_MESSAGE_ERROR);assert(!message);
  playback.paused=true;gst_element_set_state(player,GST_STATE_PAUSED);gst_element_get_state(player,nullptr,nullptr,2*GST_SECOND);pump(100);
  auto stamp=std::filesystem::last_write_time(frame);pump(150);assert(std::filesystem::last_write_time(frame)==stamp);
  playback.paused=false;gst_element_set_state(player,GST_STATE_PLAYING);
  for(int n=0;n<60&&std::filesystem::last_write_time(frame)==stamp;n++)pump(50);
  if(std::filesystem::last_write_time(frame)==stamp){
   GstState state,pending;gst_element_get_state(player,&state,&pending,0);gint64 position=0,duration=0;gst_element_query_position(player,GST_FORMAT_TIME,&position);gst_element_query_duration(player,GST_FORMAT_TIME,&duration);
   std::cerr<<"HLS resume stalled state="<<gst_element_state_get_name(state)<<" pending="<<gst_element_state_get_name(pending)<<" position="<<position<<" duration="<<duration<<"\n";
   while(auto* msg=gst_bus_pop(bus)){if(GST_MESSAGE_TYPE(msg)==GST_MESSAGE_ERROR){GError* e=nullptr;gchar* debug=nullptr;gst_message_parse_error(msg,&e,&debug);std::cerr<<e->message<<"\n";g_clear_error(&e);g_free(debug);}gst_message_unref(msg);}
  }
  assert(std::filesystem::last_write_time(frame)!=stamp);
  assert(!playback.error);g_source_remove(watch);gst_object_unref(watch_bus);
  gst_element_set_state(player,GST_STATE_NULL);gst_object_unref(bus);gst_object_unref(player);
  std::cout<<"PASS: accepted mirror to external HLS playback, updated dimensions, pause, resume and stop\n";
 }
 std::filesystem::remove_all(directory);
 std::cout<<"PASS: static first frame after acceptance, paused frame privacy, resume, no redundant writes and 12 clean teardown cycles\n";
}
