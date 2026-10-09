// Product integration test: receive the real stream in Framely's CEF engine.
#include "include/capi/cef_app_capi.h"
#include "include/capi/cef_client_capi.h"
#include "include/capi/cef_load_handler_capi.h"
#include "include/cef_api_hash.h"
#include <atomic>
#include <chrono>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <thread>
#include <fstream>
#include <vector>
template<class T> struct Handler {
 T api{};std::atomic<int> refs{1};
 Handler(){api.base.size=sizeof(T);api.base.add_ref=[](cef_base_ref_counted_t* p){++reinterpret_cast<Handler*>(p)->refs;};api.base.release=[](cef_base_ref_counted_t* p){--reinterpret_cast<Handler*>(p)->refs;return 0;};api.base.has_one_ref=[](cef_base_ref_counted_t* p){return int(reinterpret_cast<Handler*>(p)->refs==1);};api.base.has_at_least_one_ref=[](cef_base_ref_counted_t* p){return int(reinterpret_cast<Handler*>(p)->refs>0);};}
 T* acquire(){api.base.add_ref(&api.base);return &api;}
};
static Handler<cef_client_t> client;static Handler<cef_render_handler_t> render;static Handler<cef_display_handler_t> display;static Handler<cef_life_span_handler_t> life;static Handler<cef_load_handler_t> load;
static bool passed=false,failed=false,closed=false;static unsigned paints=0;static std::vector<unsigned char> pixels;static int width=0,height=0;
static void str(cef_string_t& s,const std::string& v){cef_string_utf8_to_utf16(v.data(),v.size(),&s);}
static std::string text(const cef_string_t* s){cef_string_utf8_t o{};cef_string_utf16_to_utf8(s->str,s->length,&o);std::string r(o.str,o.length);cef_string_utf8_clear(&o);return r;}
int main(int argc,char** argv){
 std::cout.setf(std::ios::unitbuf);
 const std::string url=argc>1?argv[1]:"", runtime=argc>2?argv[2]:"";
 cef_main_args_t args{argc,argv};if(std::strcmp(cef_api_hash(CEF_API_VERSION,0),CEF_API_HASH_PLATFORM))return 2;int child=cef_execute_process(&args,nullptr,nullptr);if(child>=0)return child;if(argc<3)return 2;
 auto* cef=std::getenv("FRAMELY_CEF_ROOT");if(!cef)return 2;
 cef_settings_t settings{};settings.size=sizeof(settings);settings.windowless_rendering_enabled=1;str(settings.root_cache_path,runtime+"/cache");str(settings.log_file,runtime+"/cef.log");str(settings.resources_dir_path,cef);str(settings.locales_dir_path,std::string(cef)+"/locales");
 if(!cef_initialize(&args,&settings,nullptr,nullptr))return 3;
 render.api.get_view_rect=[](cef_render_handler_t*,cef_browser_t*,cef_rect_t* r){*r={0,0,960,720};};
 render.api.on_paint=[](cef_render_handler_t*,cef_browser_t*,cef_paint_element_type_t type,size_t,const cef_rect_t*,const void* data,int w,int h){if(type==PET_VIEW){paints++;width=w;height=h;pixels.assign((const unsigned char*)data,(const unsigned char*)data+size_t(w)*h*4);}};
 display.api.on_console_message=[](cef_display_handler_t*,cef_browser_t*,cef_log_severity_t,const cef_string_t* m,const cef_string_t*,int){auto s=text(m);std::cout<<s<<std::endl;if(s.rfind("CAST_BROWSER_PASS",0)==0)passed=true;if(s.rfind("CAST_BROWSER_FAIL",0)==0)failed=true;return 1;};
 life.api.on_before_close=[](cef_life_span_handler_t*,cef_browser_t*){closed=true;};
 load.api.on_load_end=[](cef_load_handler_t*,cef_browser_t*,cef_frame_t* frame,int){if(!frame->is_main(frame))return;cef_string_t script{},source{};
 str(script,R"JS((()=>{let tries=0;const timer=setInterval(()=>{const video=document.querySelector('video');if(video?.readyState>=2&&video.currentTime>1){clearInterval(timer);if(video.videoWidth!==640||video.videoHeight!==360){console.error('CAST_BROWSER_FAIL unexpected dimensions '+video.videoWidth+'x'+video.videoHeight);return;}console.log('CAST_BROWSER_PASS '+JSON.stringify({width:video.videoWidth,height:video.videoHeight,time:video.currentTime,frames:video.getVideoPlaybackQuality?.().totalVideoFrames}));}else if(++tries>350){clearInterval(timer);console.error('CAST_BROWSER_FAIL '+document.body.innerText+' video='+JSON.stringify(video?{ready:video.readyState,time:video.currentTime,error:video.error?.message}:null));}},100);})())JS");
 frame->execute_java_script(frame,&script,&source,1);cef_string_utf16_clear(&script);};
 client.api.get_render_handler=[](cef_client_t*){return render.acquire();};client.api.get_display_handler=[](cef_client_t*){return display.acquire();};client.api.get_life_span_handler=[](cef_client_t*){return life.acquire();};client.api.get_load_handler=[](cef_client_t*){return load.acquire();};
 cef_window_info_t win{};win.size=sizeof(win);win.windowless_rendering_enabled=1;win.runtime_style=CEF_RUNTIME_STYLE_ALLOY;cef_browser_settings_t bs{};bs.size=sizeof(bs);bs.windowless_frame_rate=30;cef_string_t uri{};str(uri,url);auto* browser=cef_browser_host_create_browser_sync(&win,&client.api,&uri,&bs,nullptr,nullptr);cef_string_utf16_clear(&uri);if(!browser)return 4;
 auto* host=browser->get_host(browser);auto end=std::chrono::steady_clock::now()+std::chrono::seconds(40);
 while(std::chrono::steady_clock::now()<end&&!passed&&!failed){cef_do_message_loop_work();std::this_thread::sleep_for(std::chrono::milliseconds(5));}
 if(const char* output=getenv("FRAMELY_CAST_PREVIEW")){if(!pixels.empty()){std::ofstream file(output,std::ios::binary);file<<"P6\n"<<width<<" "<<height<<"\n255\n";for(size_t i=0;i<pixels.size();i+=4){unsigned char rgb[]={pixels[i+2],pixels[i+1],pixels[i]};file.write((const char*)rgb,3);}}}
 host->close_browser(host,1);for(int i=0;i<400&&!closed;i++){cef_do_message_loop_work();std::this_thread::sleep_for(std::chrono::milliseconds(5));}bool ok=passed&&!failed&&paints>0&&closed;std::cout<<"CAST_BROWSER_RESULT pass="<<ok<<" paints="<<paints<<" closed="<<closed<<std::endl;if(!closed)_Exit(5);host->base.release(&host->base);browser->base.release(&browser->base);cef_shutdown();return ok?0:5;
}
