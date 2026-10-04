#include "include/capi/cef_app_capi.h"
#include "include/capi/cef_image_capi.h"
#include "include/capi/cef_parser_capi.h"
#include "include/capi/cef_values_capi.h"
#include "include/capi/cef_client_capi.h"
#include "include/cef_api_hash.h"
#include "vendor/json.hpp"
#include "dock_uv_geometry.h"
#include "menu_geometry.h"
#include "menu_input.h"
#include "overlay_placement.h"
#include "render_resolution.h"
#include "notification_geometry.h"
#include "keyboard_state.h"
#include "keyboard_input.h"
#include "brand_mask.h"
#include "ui_visibility.h"
#include <GL/gl.h>
#include <GL/glx.h>
#include <arpa/inet.h>
#include <sys/socket.h>
#include <atomic>
#include <csignal>
#include <chrono>
#include <fstream>
#include <iostream>
#include <map>
#include <memory>
#include <mutex>
#include <thread>
#include <deque>
#include <unistd.h>

using json=nlohmann::json;
static volatile sig_atomic_t running=1;
static void stop(int){running=0;}
static std::string origin,runtime,token;
static vr::IVROverlay* overlays=nullptr;
static vr::IVRSystem* vr_system=nullptr;
static Display* xdisplay=nullptr;
static GLXContext glcontext=nullptr;
static GLXPbuffer glsurface=0;
static bool menu_open=false,keyboard_open=false,keyboard_close_pending=false;
static bool keyboard_toolbar=false,keyboard_chars=false,keyboard_done=false;
static MenuButtonInput menu_button;
static cef_frame_t* keyboard_frame=nullptr;
static std::string keyboard_view;
static KeyboardMenuGuard keyboard_menu;
static uint64_t keyboard_token=0;static vr::VROverlayHandle_t keyboard_target=0;
static std::mutex queue_mutex;
static std::mutex visibility_mutex;
static json visible_views=nullptr;
static std::deque<json> queue;

template<class T> struct Handler {
 T api{};std::atomic<int> refs{1};
 Handler(){api.base.size=sizeof(T);api.base.add_ref=[](cef_base_ref_counted_t* p){++reinterpret_cast<Handler*>(p)->refs;};api.base.release=[](cef_base_ref_counted_t* p){--reinterpret_cast<Handler*>(p)->refs;return 0;};api.base.has_one_ref=[](cef_base_ref_counted_t* p){return int(reinterpret_cast<Handler*>(p)->refs==1);};api.base.has_at_least_one_ref=[](cef_base_ref_counted_t* p){return int(reinterpret_cast<Handler*>(p)->refs>0);};}
 T* acquire(){api.base.add_ref(&api.base);return &api;}
};
struct View {
 std::string key,plugin,title;int width=600,height=840,render_scale=1;bool dashboard=false,shown=false,dirty=false,closing=false,closed=false;
 OverlayPlacement placement;std::chrono::steady_clock::time_point haptic_at{};
 bool active_seen=false;int browser_id=0;cef_browser_t* browser=nullptr;cef_browser_host_t* host=nullptr;
 vr::VROverlayHandle_t overlay=0,thumbnail=0;GLuint textures[2]{};int texture_slot=0;bool texture_ready[2]{};std::vector<uint8_t> pixels;
};
static std::map<std::string,std::unique_ptr<View>> views;
static View* find_browser(cef_browser_t* b){int id=b->get_identifier(b);for(auto& [key,v]:views)if(v->browser_id==id)return v.get();return nullptr;}
static Handler<cef_client_t> client;
static Handler<cef_render_handler_t> render_handler;
static Handler<cef_life_span_handler_t> life_handler;
static Handler<cef_display_handler_t> display_handler;
static Handler<cef_request_handler_t> request_handler;
static void str(cef_string_t& o,const std::string& s){cef_string_utf8_to_utf16(s.data(),s.size(),&o);}
static std::string text(const cef_string_t* s){cef_string_utf8_t o{};cef_string_utf16_to_utf8(s->str,s->length,&o);std::string r(o.str,o.length);cef_string_utf8_clear(&o);return r;}
static void execute(cef_frame_t* f,const std::string& code){if(!f||!f->is_valid(f))return;cef_string_t script{},url{};str(script,code);f->execute_java_script(f,&script,&url,1);cef_string_utf16_clear(&script);}
// Steam Frame's BarSurface/BarTab dimensions and state colors (80px, radius 6px).
static std::vector<uint8_t> icon(bool hover,bool active){
 std::vector<uint8_t> p(128*128*4);
 auto rounded=[](float x,float y,float half,float radius){float a=std::abs(x)-half+radius,b=std::abs(y)-half+radius;return std::hypot(std::max(a,0.f),std::max(b,0.f))+std::min(std::max(a,b),0.f)-radius;};
 for(int y=0;y<128;y++)for(int x=0;x<128;x++){
  float coverage=std::clamp(.5f-rounded(x-63.5f,y-63.5f,64.f,9.6f),0.f,1.f);
  int gx=std::min(47,int((x+.5f)*48/128)),gy=std::min(47,int((y+.5f)*48/128));
  float mark=framely_brand::mask[gy*48+gx]/255.f;
  int i=(y*128+x)*4;
  const int normal[3]={23,25,28},highlight[3]={32,35,40},blue[3]={147,197,237};
  for(int c=0;c<3;c++){float bg=(hover||active)?highlight[c]:normal[c];p[i+c]=uint8_t(bg+(blue[c]-bg)*mark);}
  // 32px-wide, 6px-high tab indicator is clipped to its upper 3px by the BarSurface.
  if(active){float qx=std::abs(x-63.5f)-20.8f,qy=std::abs(y-129.6f);float line=std::clamp(5.3f-std::hypot(std::max(qx,0.f),qy),0.f,1.f);const int blue[3]={26,159,255};for(int c=0;c<3;c++)p[i+c]=uint8_t(p[i+c]+(blue[c]-p[i+c])*line);}
  p[i+3]=uint8_t(255*coverage);
 }
 return p;
}

static json poll_http(int port){int fd=socket(AF_INET,SOCK_STREAM,0);if(fd<0)throw std::runtime_error("HTTP socket failed");timeval timeout{1,0};setsockopt(fd,SOL_SOCKET,SO_RCVTIMEO,&timeout,sizeof(timeout));setsockopt(fd,SOL_SOCKET,SO_SNDTIMEO,&timeout,sizeof(timeout));sockaddr_in addr{};addr.sin_family=AF_INET;addr.sin_port=htons(port);inet_pton(AF_INET,"127.0.0.1",&addr.sin_addr);if(connect(fd,reinterpret_cast<sockaddr*>(&addr),sizeof(addr))){close(fd);throw std::runtime_error("UI agent unavailable");}std::string payload;{std::lock_guard<std::mutex> lock(visibility_mutex);payload=visible_views.dump();}std::string req="POST /host/poll HTTP/1.1\r\nHost: 127.0.0.1:"+std::to_string(port)+"\r\nX-Framely-Native: "+token+"\r\nContent-Type: application/json\r\nContent-Length: "+std::to_string(payload.size())+"\r\nConnection: close\r\n\r\n"+payload;size_t sent=0;while(sent<req.size()){auto n=send(fd,req.data()+sent,req.size()-sent,MSG_NOSIGNAL);if(n<=0){close(fd);throw std::runtime_error("HTTP send failed");}sent+=n;}std::string response;char buffer[4096];ssize_t n;while((n=recv(fd,buffer,sizeof(buffer),0))>0){response.append(buffer,n);if(response.size()>4*1024*1024){close(fd);throw std::runtime_error("HTTP body limit");}}close(fd);auto body=response.find("\r\n\r\n");if(body==std::string::npos||response.find("200 OK")==std::string::npos)throw std::runtime_error("Invalid agent response");return json::parse(response.substr(body+4));}
// Frame renders the native laser cursor in the default overlay sort group.
// Raising a panel above that group can obscure the runtime cursor.
// OpenVR owns and renders the laser cursor. Coordinates must remain in the
// runtime's overlay mouse space, before conversion to CEF's top-left origin.
// Keep a wider curved mesh around the narrow button; transparent margins never hit input.
static constexpr int dock_canvas_width=640,dock_canvas_height=128,dock_icon_left=256;
static std::vector<uint8_t> dock_icon(bool hover,bool active){
 auto source=icon(hover,active);std::vector<uint8_t> canvas(dock_canvas_width*dock_canvas_height*4,0);
 for(int row=0;row<dock_canvas_height;row++)std::copy_n(source.data()+row*128*4,128*4,canvas.data()+(row*dock_canvas_width+dock_icon_left)*4);
 return canvas;
}
static vr::VROverlayHandle_t active_main_window(){
 static auto refreshed=std::chrono::steady_clock::time_point{};static vr::VROverlayHandle_t selected=0;
 auto now=std::chrono::steady_clock::now();if(now-refreshed<std::chrono::milliseconds(500)&&selected&&overlays->IsActiveDashboardOverlay(selected))return selected;
 refreshed=now;selected=0;
 overlays->FindOverlay("valve.steam.gamepadui.main",&selected);
 return selected;
}
static void hover_haptic(vr::VROverlayHandle_t overlay,std::chrono::steady_clock::time_point& last){
 auto now=std::chrono::steady_clock::now();if(now-last<std::chrono::milliseconds(80)||!overlays->IsOverlayVisible(overlay)||!overlays->IsHoverTargetOverlay(overlay))return;
 auto e=overlays->TriggerLaserMouseHapticVibration(overlay,.012f,120.f,.15f);if(!e)last=now;else std::cerr<<"Hover haptic failed "<<overlay<<" "<<e<<"\n";
}
static void laser_cursor(vr::VROverlayHandle_t overlay,const vr::VREvent_t& event){
 if(event.eventType==vr::VREvent_MouseMove){vr::HmdVector2_t position{{event.data.mouse.x,event.data.mouse.y}};overlays->SetOverlayCursorPositionOverride(overlay,&position);}
 else if(event.eventType==vr::VREvent_FocusLeave)overlays->ClearOverlayCursorPositionOverride(overlay);
}
static void show(View& v,bool shown){if(v.closing)return;if(v.key=="notifications")overlays->SetOverlayInputMethod(v.overlay,shown?vr::VROverlayInputMethod_Mouse:vr::VROverlayInputMethod_None);if(v.key=="menu"&&shown!=v.shown)overlays->SetOverlayFlag(v.overlay,vr::VROverlayFlags_WantsModalBehavior,shown&&!(keyboard_open&&keyboard_view=="menu"));bool actual=overlays->IsOverlayVisible(v.overlay);if(shown!=actual){auto e=shown?overlays->ShowOverlay(v.overlay):overlays->HideOverlay(v.overlay);if(e){std::cerr<<"Overlay visibility failed "<<v.key<<" "<<e<<"\n";return;}}if(shown!=v.shown){if(!shown)overlays->ClearOverlayCursorPositionOverride(v.overlay);if(v.key!="notifications")v.host->was_hidden(v.host,!shown);v.shown=shown;if(shown)v.host->invalidate(v.host,PET_VIEW);}}
static void close_keyboard(bool hide=true){if(keyboard_open&&hide)overlays->HideKeyboard();keyboard_open=false;keyboard_close_pending=false;keyboard_toolbar=false;keyboard_chars=false;keyboard_done=false;keyboard_target=0;keyboard_menu.end(std::chrono::steady_clock::now());if(keyboard_view=="menu"&&views.count("menu")&&!views.at("menu")->closing)overlays->SetOverlayFlag(views.at("menu")->overlay,vr::VROverlayFlags_WantsModalBehavior,views.at("menu")->shown);if(keyboard_frame){keyboard_frame->base.release(&keyboard_frame->base);keyboard_frame=nullptr;}keyboard_view.clear();}
static bool keyboard_event(const vr::VREvent_t& e,vr::VROverlayHandle_t source_overlay=0){
 // The runtime toolbar opens the keyboard without a host.keyboard request.
 if(e.eventType==vr::VREvent_KeyboardOpened_Global){
  auto target=e.data.keyboard.overlayHandle;
  if(keyboard_open&&target==keyboard_target)return true;
  View* owner=nullptr;for(auto&[key,v]:views)if(v->overlay==target&&!v->closing){owner=v.get();break;}
  if(!owner)return false;
  close_keyboard(false);owner->host->set_focus(owner->host,1);
  keyboard_frame=owner->browser->get_focused_frame(owner->browser);keyboard_view=owner->key;
  keyboard_target=target;keyboard_token=e.data.keyboard.uUserValue;keyboard_open=true;keyboard_toolbar=true;
  keyboard_menu.begin(owner->key=="menu");return true;
 }
 if(e.eventType!=vr::VREvent_KeyboardCharInput&&e.eventType!=vr::VREvent_KeyboardDone&&e.eventType!=vr::VREvent_KeyboardClosed&&e.eventType!=vr::VREvent_KeyboardClosed_Global)return false;
 if(!keyboard_open)return true;
 if(e.data.keyboard.uUserValue&&e.data.keyboard.uUserValue!=keyboard_token)return true;
 if(e.data.keyboard.overlayHandle&&e.data.keyboard.overlayHandle!=keyboard_target)return true;
 if(e.eventType==vr::VREvent_KeyboardCharInput){
  // OpenVR duplicates character events globally. Only the owner Overlay edits.
  if(keyboard_edit_source(source_overlay,keyboard_target)){keyboard_chars=true;keyboard_edit(keyboard_frame,std::string(e.data.keyboard.cNewInput,strnlen(e.data.keyboard.cNewInput,sizeof(e.data.keyboard.cNewInput))));}
 }else {keyboard_done=keyboard_done||e.eventType==vr::VREvent_KeyboardDone;keyboard_close_pending=true;}
 return true;
}
static void close_view(View& v){if(v.closing)return;if(keyboard_view==v.key)close_keyboard();v.closing=true;if(v.key=="menu")overlays->SetOverlayFlag(v.overlay,vr::VROverlayFlags_WantsModalBehavior,false);overlays->ClearOverlayCursorPositionOverride(v.overlay);overlays->HideOverlay(v.overlay);v.host->close_browser(v.host,1);}
static void anchor_to_head(View& v,float width,float x=0,float y=0,float z=-1.35f){vr::TrackedDevicePose_t poses[vr::k_unMaxTrackedDeviceCount]{};vr_system->GetDeviceToAbsoluteTrackingPose(vr::TrackingUniverseStanding,0,poses,vr::k_unMaxTrackedDeviceCount);if(!poses[0].bPoseIsValid)return;auto m=poses[0].mDeviceToAbsoluteTracking;for(int r=0;r<3;r++)m.m[r][3]+=m.m[r][0]*x+m.m[r][1]*y+m.m[r][2]*z;overlays->SetOverlayWidthInMeters(v.overlay,width);overlays->SetOverlayTransformAbsolute(v.overlay,vr::TrackingUniverseStanding,&m);}
static View& create_view(const std::string& key,const std::string& title,const std::string& url,bool dock,int width,int height,const std::string& plugin="",float physical_width=0){
 if(views.count(key)){auto& v=*views.at(key);if(dock)overlays->ShowDashboard(key.c_str());else if(key!="notifications")show(v,true);return v;}
 if(views.size()>=34)throw std::runtime_error("Window limit exceeded");auto v=std::make_unique<View>();v->key=key;v->plugin=plugin;v->title=title;v->dashboard=dock;v->width=width;v->height=height;v->render_scale=key=="framely.manager"?4:(key=="menu"||key=="notifications")?2:1;
 auto error=dock?overlays->CreateDashboardOverlay(key.c_str(),title.c_str(),&v->overlay,&v->thumbnail):overlays->CreateOverlay(key.c_str(),title.c_str(),&v->overlay);if(error)throw std::runtime_error("Overlay creation failed: "+std::to_string(error));
 overlays->SetOverlayInputMethod(v->overlay,key=="notifications"?vr::VROverlayInputMethod_None:vr::VROverlayInputMethod_Mouse);vr::HmdVector2_t scale{{float(width),float(height)}};overlays->SetOverlayMouseScale(v->overlay,&scale);overlays->SetOverlayFlag(v->overlay,vr::VROverlayFlags_HideLaserIntersection,false);overlays->SetOverlayFlag(v->overlay,vr::VROverlayFlags_SendVRSmoothScrollEvents,true);overlays->SetOverlayFlag(v->overlay,vr::VROverlayFlags_VisibleInDashboard,true);overlays->SetOverlayFlag(v->overlay,vr::VROverlayFlags_MakeOverlaysInteractiveIfVisible,true);overlays->SetOverlayFlag(v->overlay,vr::VROverlayFlags_SortWithNonSceneOverlays,false);overlays->SetOverlaySortOrder(v->overlay,key=="notifications"?100:0);

 // Keep the runtime's full dashboard controls, including its keyboard and close buttons.
 if(dock){
  auto result=overlays->SetOverlayFlag(v->overlay,vr::VROverlayFlags_MinimalControlBar,false);
  if(result)std::cerr<<"Dashboard controls unavailable "<<key<<" "<<result<<"\n";
  for(auto flag:{vr::VROverlayFlags_EnableControlBarKeyboard,vr::VROverlayFlags_EnableControlBarClose}){
   result=overlays->SetOverlayFlag(v->overlay,flag,true);
   if(result)std::cerr<<"Dashboard control unavailable "<<key<<" "<<result<<"\n";
  }
 }
 if(dock){auto pixels=icon(false,true);overlays->SetOverlayRaw(v->thumbnail,pixels.data(),128,128,4);overlays->SetOverlayWidthInMeters(v->overlay,physical_width>0?physical_width:(key=="framely.manager"?3.2f:1.2f));}else if(key!="menu"&&key!="notifications"){anchor_to_head(*v,physical_width>0?physical_width:1.0f);}
 cef_window_info_t info{};info.size=sizeof(info);info.windowless_rendering_enabled=1;cef_browser_settings_t bs{};bs.size=sizeof(bs);bs.windowless_frame_rate=30;bs.background_color=0;cef_string_t uri{};str(uri,url);auto* browser=cef_browser_host_create_browser_sync(&info,&client.api,&uri,&bs,nullptr,nullptr);cef_string_utf16_clear(&uri);
 if(!browser){overlays->DestroyOverlay(v->overlay);if(v->thumbnail)overlays->DestroyOverlay(v->thumbnail);throw std::runtime_error("Browser creation failed");}v->browser=browser;v->browser_id=browser->get_identifier(browser);v->host=browser->get_host(browser);glGenTextures(2,v->textures);auto* ptr=v.get();views.emplace(key,std::move(v));ptr->host->notify_screen_info_changed(ptr->host);ptr->host->was_resized(ptr->host);ptr->host->was_hidden(ptr->host,key!="notifications");if(dock)overlays->ShowDashboard(key.c_str());else if(key!="menu"&&key!="notifications")show(*ptr,true);return *ptr;
}
static void plugin_thumbnail(View& view,const std::string& encoded){
 if(!view.thumbnail||encoded.empty()||encoded.size()>1400000)return;
 cef_string_t value{};str(value,encoded);auto* data=cef_base64_decode(&value);cef_string_utf16_clear(&value);if(!data)return;
 std::vector<uint8_t> png(data->get_size(data));data->get_data(data,png.data(),png.size(),0);data->base.release(&data->base);
 auto* image=cef_image_create();if(!image)return;
 if(!image->add_png(image,1,png.data(),png.size())||image->get_width(image)>1024||image->get_height(image)>1024){image->base.release(&image->base);return;}
 int width=0,height=0;auto* bitmap=image->get_as_bitmap(image,1,CEF_COLOR_TYPE_RGBA_8888,CEF_ALPHA_TYPE_POSTMULTIPLIED,&width,&height);image->base.release(&image->base);
 if(!bitmap)return;if(width<=0||height<=0||width>1024||height>1024){bitmap->base.release(&bitmap->base);return;}
 std::vector<uint8_t> source(bitmap->get_size(bitmap));bitmap->get_data(bitmap,source.data(),source.size(),0);bitmap->base.release(&bitmap->base);
 if(source.size()!=size_t(width*height*4))return;
 std::vector<uint8_t> pixels(128*128*4,0);float scale=std::min(112.f/width,112.f/height);int w=std::max(1,int(width*scale)),h=std::max(1,int(height*scale));
 for(int y=0;y<h;y++)for(int x=0;x<w;x++){int sx=std::min(width-1,int(x/scale)),sy=std::min(height-1,int(y/scale));std::copy_n(source.data()+(sy*width+sx)*4,4,pixels.data()+((y+(128-h)/2)*128+x+(128-w)/2)*4);}
 overlays->SetOverlayRaw(view.thumbnail,pixels.data(),128,128,4);
}
static void commands(){std::deque<json> commands;{std::lock_guard<std::mutex> lock(queue_mutex);commands.swap(queue);}for(const auto& c:commands){try{auto kind=c.value("kind","");if(kind=="haptic"){auto key=c.value("view","");if(views.count(key)&&views.at(key)->shown&&!views.at(key)->closing)hover_haptic(views.at(key)->overlay,views.at(key)->haptic_at);}else if(kind=="manager.open"){if(views.count("framely.manager")&&views.at("framely.manager")->closing){std::lock_guard<std::mutex> lock(queue_mutex);queue.push_back(c);continue;}auto page=c.value("page",std::string{});auto& manager=create_view("framely.manager","Framely 插件管理",origin+"/manager"+(page=="catalog"?"#catalog":""),true,1440,810);if(page=="catalog"){auto* frame=manager.browser->get_main_frame(manager.browser);execute(frame,"location.hash=\"catalog\";window.dispatchEvent(new Event(\"hashchange\"))");frame->base.release(&frame->base);}menu_open=false;}else if(kind=="agreement.decline"){auto key=c.value("view","");if(key=="menu"){menu_open=false;if(views.count("menu")){show(*views.at("menu"),false);views.at("menu")->browser->reload(views.at("menu")->browser);}}else if(views.count(key))close_view(*views.at(key));}else if(kind=="manager.close"){if(views.count("framely.manager"))close_view(*views.at("framely.manager"));}else if(kind=="menu.close"){menu_open=false;}else if(kind=="window.open"){std::string plugin=c.at("plugin"),win=c.at("window");auto key="framely.window."+plugin+"."+win;if(views.count(key)&&views.at(key)->closing){std::lock_guard<std::mutex> lock(queue_mutex);queue.push_back(c);continue;}auto spec=c.at("spec");auto& opened=create_view("framely.window."+plugin+"."+win,spec.at("title"),origin+"/window/"+plugin+"/"+win,spec.value("dockIcon",false),spec.value("width",1600),spec.value("height",900),plugin,spec.value("widthMeters",3.f));plugin_thumbnail(opened,spec.value("iconData",std::string{}));menu_open=false;}else if(kind=="window.close"){auto key="framely.window."+c.at("plugin").get<std::string>()+"."+c.at("window").get<std::string>();if(views.count(key))close_view(*views.at(key));}else if(kind=="plugin.disabled"){for(auto&[key,v]:views)if(v->plugin==c.at("plugin"))close_view(*v);}else if(kind=="notification.changed"){auto& v=create_view("notifications","Framely notifications",origin+"/notifications",false,600,360);anchor_to_head(v,.42f,.42f,.32f,-.8f);v.host->was_hidden(v.host,0);v.host->invalidate(v.host,PET_VIEW);}else if(kind=="keyboard"){auto key=c.value("view","menu");if(!views.count(key))continue;auto& v=*views.at(key);if(v.closing)continue;close_keyboard();v.host->set_focus(v.host,1);keyboard_frame=v.browser->get_focused_frame(v.browser);keyboard_view=key;keyboard_target=v.overlay;++keyboard_token;auto existing=c.value("existing","");if(key=="menu")overlays->SetOverlayFlag(v.overlay,vr::VROverlayFlags_WantsModalBehavior,false);auto e=overlays->ShowKeyboardForOverlay(v.overlay,c.value("password",false)?vr::k_EGamepadTextInputModePassword:vr::k_EGamepadTextInputModeNormal,c.value("multiline",false)?vr::k_EGamepadTextInputLineModeMultipleLines:vr::k_EGamepadTextInputLineModeSingleLine,vr::KeyboardFlag_Modal|vr::KeyboardFlag_Minimal|vr::KeyboardFlag_ShowArrowKeys,"Framely",4096,existing.c_str(),keyboard_token);keyboard_open=e==vr::VROverlayError_None;keyboard_menu.begin(keyboard_open&&key=="menu");if(!keyboard_open)close_keyboard();}else if(kind=="notifications.empty"){if(views.count("notifications"))show(*views.at("notifications"),false);}}catch(const std::exception& e){std::cerr<<"Native command error: "<<e.what()<<"\n";}}
}
static void overlay_input(View& v,const vr::VREvent_t& e){if(v.closing||(v.key=="notifications"&&!v.shown))return;if(keyboard_event(e,v.overlay))return;laser_cursor(v.overlay,e);cef_mouse_event_t mouse{};mouse.x=int(e.data.mouse.x);mouse.y=v.height-int(e.data.mouse.y);switch(e.eventType){case vr::VREvent_Modal_Cancel:if(v.key=="menu"&&!keyboard_open&&!keyboard_menu.holds(std::chrono::steady_clock::now())&&menu_button.can_cancel(std::chrono::steady_clock::now())){menu_open=false;std::cout<<"Menu closed: outside click"<<std::endl;}break;case vr::VREvent_OverlayClosed:if(v.key!="menu"&&v.key!="notifications")close_view(v);break;case vr::VREvent_ButtonPress:if(e.data.controller.button==vr::k_EButton_ApplicationMenu){auto* frame=v.browser->get_main_frame(v.browser);execute(frame,"window.dispatchEvent(new Event('framely.back'))");frame->base.release(&frame->base);}break;case vr::VREvent_FocusLeave:v.host->send_mouse_move_event(v.host,&mouse,1);break;case vr::VREvent_MouseMove:v.host->send_mouse_move_event(v.host,&mouse,0);break;case vr::VREvent_MouseButtonDown:case vr::VREvent_MouseButtonUp:{v.host->set_focus(v.host,1);auto button=e.data.mouse.button==vr::VRMouseButton_Right?MBT_RIGHT:MBT_LEFT;mouse.modifiers=e.eventType==vr::VREvent_MouseButtonDown?EVENTFLAG_LEFT_MOUSE_BUTTON:0;v.host->send_mouse_click_event(v.host,&mouse,button,e.eventType==vr::VREvent_MouseButtonUp,1);break;}case vr::VREvent_ScrollSmooth:case vr::VREvent_ScrollDiscrete:mouse.x=v.width/2;mouse.y=v.height/2;v.host->send_mouse_wheel_event(v.host,&mouse,int(e.data.scroll.xdelta*120),int(e.data.scroll.ydelta*120));break;case vr::VREvent_OverlayGamepadFocusLost:if(!keyboard_open&&!v.dashboard&&v.key!="menu"&&v.key!="notifications")close_view(v);break;default:break;}}

int main(int argc,char** argv){
 std::cout.setf(std::ios::unitbuf);std::string initial;int port=0;for(int i=1;i+1<argc;i++){std::string s=argv[i];if(s=="--url")initial=argv[++i];else if(s=="--port")port=std::stoi(argv[++i]);else if(s=="--runtime")runtime=argv[++i];}
 const char* key=std::getenv("FRAMELY_NATIVE_TOKEN");if(key)token=key;
 cef_main_args_t args{argc,argv};if(std::strcmp(cef_api_hash(CEF_API_VERSION,0),CEF_API_HASH_PLATFORM)){std::cerr<<"CEF ABI mismatch\n";return 2;}int child=cef_execute_process(&args,nullptr,nullptr);if(child>=0)return child;
 if(initial.empty()||runtime.empty()||!port||token.empty())return 2;origin="http://127.0.0.1:"+std::to_string(port);std::signal(SIGINT,stop);std::signal(SIGTERM,stop);std::signal(SIGPIPE,SIG_IGN);
 // Check the existing runtime without bootstrapping it before opening the renderer.
 vr::EVRInitError vr_error;vr_system=vr::VR_Init(&vr_error,vr::VRApplication_Background);if(vr_error){std::cerr<<"SteamVR unavailable "<<vr_error<<"\n";return 3;}overlays=vr::VROverlay();
 vr::VROverlayHandle_t native_main=0,native_dock=0;
 if(!overlays||overlays->FindOverlay("valve.steam.gamepadui.main",&native_main)||overlays->FindOverlay("valve.steam.gamepadui.bar",&native_dock)){std::cerr<<"Waiting for native Steam UI\n";vr::VR_Shutdown();return 3;}
 vr::VR_Shutdown();
 // Frame's compositor requires the Overlay application type for visible UI.
 // The session agent and service gate this renderer on all native services.
 vr_system=vr::VR_Init(&vr_error,vr::VRApplication_Overlay);if(vr_error){std::cerr<<"SteamVR renderer unavailable "<<vr_error<<"\n";return 3;}overlays=vr::VROverlay();
 xdisplay=XOpenDisplay(nullptr);if(!xdisplay){vr::VR_Shutdown();return 4;}int n=0;int attrs[]={GLX_DRAWABLE_TYPE,GLX_PBUFFER_BIT,GLX_RENDER_TYPE,GLX_RGBA_BIT,None};auto* configs=glXChooseFBConfig(xdisplay,DefaultScreen(xdisplay),attrs,&n);if(!configs||!n){vr::VR_Shutdown();return 4;}int pba[]={GLX_PBUFFER_WIDTH,16,GLX_PBUFFER_HEIGHT,16,None};glsurface=glXCreatePbuffer(xdisplay,configs[0],pba);glcontext=glXCreateNewContext(xdisplay,configs[0],GLX_RGBA_TYPE,nullptr,True);XFree(configs);if(!glcontext||!glXMakeContextCurrent(xdisplay,glsurface,glsurface,glcontext)){vr::VR_Shutdown();return 4;}
 cef_settings_t settings{};settings.size=sizeof(settings);settings.windowless_rendering_enabled=1;std::string locale;for(const char* key:{"LC_ALL","LC_MESSAGES","LANG"}){const char* value=std::getenv(key);if(value&&*value){locale=value;break;}}auto suffix=locale.find_first_of(".@");if(suffix!=std::string::npos)locale.erase(suffix);std::replace(locale.begin(),locale.end(),'_','-');if(locale=="C"||locale=="POSIX"||locale.empty())locale="en-US";str(settings.accept_language_list,locale+",en-US,en");str(settings.root_cache_path,runtime+"/cef-cache");str(settings.log_file,runtime+"/cef.log");if(!cef_initialize(&args,&settings,nullptr,nullptr)){vr::VR_Shutdown();return 5;}
 render_handler.api.get_view_rect=[](cef_render_handler_t*,cef_browser_t* b,cef_rect_t* r){auto* v=find_browser(b);*r={0,0,v?v->width:600,v?v->height:840};};
 render_handler.api.get_screen_info=[](cef_render_handler_t*,cef_browser_t* b,cef_screen_info_t* info){auto* v=find_browser(b);return render_screen_info(info,v?v->width:600,v?v->height:840,v?v->render_scale:1);};
 render_handler.api.on_paint=[](cef_render_handler_t*,cef_browser_t* b,cef_paint_element_type_t type,size_t,const cef_rect_t*,const void* data,int w,int h){auto* v=find_browser(b);if(!v||v->closing||type!=PET_VIEW||w!=v->width*v->render_scale||h!=v->height*v->render_scale)return;v->pixels.resize(w*h*4);auto* source=static_cast<const uint8_t*>(data);for(int y=0;y<h;y++)std::copy_n(source+y*w*4,w*4,v->pixels.data()+(h-1-y)*w*4);v->dirty=true;};
 life_handler.api.on_before_popup=[](cef_life_span_handler_t*,cef_browser_t*,cef_frame_t*,int,const cef_string_t*,const cef_string_t*,cef_window_open_disposition_t,int,const cef_popup_features_t*,cef_window_info_t*,cef_client_t**,cef_browser_settings_t*,cef_dictionary_value_t**,int*){return 1;};
 life_handler.api.on_before_close=[](cef_life_span_handler_t*,cef_browser_t* b){auto* v=find_browser(b);if(v){v->closed=true;v->host->base.release(&v->host->base);v->host=nullptr;v->browser->base.release(&v->browser->base);v->browser=nullptr;}};
 display_handler.api.on_console_message=[](cef_display_handler_t*,cef_browser_t* b,cef_log_severity_t,const cef_string_t* message,const cef_string_t*,int){auto* v=find_browser(b);std::cout<<"UI ["<<(v?v->key:"loading")<<"] "<<text(message)<<std::endl;return 1;};
 request_handler.api.on_before_browse=[](cef_request_handler_t*,cef_browser_t*,cef_frame_t*,cef_request_t* r,int,int){auto* uri=r->get_url(r);auto s=text(uri);cef_string_userfree_utf16_free(uri);return int(s.rfind(origin+"/",0)!=0&&s!="about:blank");};
 request_handler.api.on_render_process_terminated=[](cef_request_handler_t*,cef_browser_t* b,cef_termination_status_t,int,const cef_string_t*){auto* v=find_browser(b);if(v){std::cerr<<"Renderer crashed: "<<v->key<<"\n";close_view(*v);if(v->key=="menu")running=0;}};
 client.api.get_render_handler=[](cef_client_t*){return render_handler.acquire();};client.api.get_life_span_handler=[](cef_client_t*){return life_handler.acquire();};client.api.get_display_handler=[](cef_client_t*){return display_handler.acquire();};client.api.get_request_handler=[](cef_client_t*){return request_handler.acquire();};
 vr::VROverlayHandle_t button=0;auto err=overlays->CreateOverlay("framely.dock.button","Framely",&button);if(err){cef_shutdown();vr::VR_Shutdown();return 6;}auto pixels=dock_icon(false,false);overlays->SetOverlayRaw(button,pixels.data(),dock_canvas_width,dock_canvas_height,4);overlays->SetOverlayInputMethod(button,vr::VROverlayInputMethod_Mouse);vr::HmdVector2_t scale{{dock_canvas_width,dock_canvas_height}};overlays->SetOverlayMouseScale(button,&scale);vr::VROverlayIntersectionMaskPrimitive_t mask{};mask.m_nPrimitiveType=vr::OverlayIntersectionPrimitiveType_Rectangle;mask.m_Primitive.m_Rectangle={dock_icon_left,0,128,128};if(overlays->SetOverlayIntersectionMask(button,&mask,1)){std::cerr<<"Dock input mask unavailable\n";overlays->DestroyOverlay(button);cef_shutdown();vr::VR_Shutdown();return 6;}overlays->SetOverlayFlag(button,vr::VROverlayFlags_VisibleInDashboard,true);overlays->SetOverlayFlag(button,vr::VROverlayFlags_MakeOverlaysInteractiveIfVisible,true);overlays->SetOverlayFlag(button,vr::VROverlayFlags_SortWithNonSceneOverlays,false);overlays->SetOverlaySortOrder(button,0);overlays->SetOverlayFlag(button,vr::VROverlayFlags_HideLaserIntersection,false);
 try{create_view("menu","Framely",initial,false,600,840);}catch(const std::exception& e){std::cerr<<e.what()<<"\n";running=0;}
 std::thread poller([port]{int failures=0;while(running){try{auto data=poll_http(port);std::lock_guard<std::mutex> lock(queue_mutex);for(const auto& c:data.at("commands")){if(queue.size()<256)queue.push_back(c);}failures=0;}catch(const std::exception& e){if(++failures>=10){std::cerr<<"Agent connection lost: "<<e.what()<<"\n";running=0;}}std::this_thread::sleep_for(std::chrono::milliseconds(100));}});
 DockUVState uv{};OverlayPlacement button_placement;bool icon_active=false,icon_hover=false;vr::VROverlayHandle_t last_dock=0;bool was_visible=false;vr::HmdMatrix34_t saved_anchor{},saved_frame{};float saved_size=0;DockCurveEvidence saved_evidence{};auto button_haptic_at=std::chrono::steady_clock::time_point{};auto anchored_at=std::chrono::steady_clock::time_point{};auto sampled_at=std::chrono::steady_clock::time_point{};
 while(running){auto frame_start=std::chrono::steady_clock::now();cef_do_message_loop_work();commands();vr::VREvent_t global{};while(vr_system->PollNextEvent(&global,sizeof(global))){if(global.eventType==vr::VREvent_Quit){vr_system->AcknowledgeQuit_Exiting();running=0;}keyboard_event(global);}vr::VROverlayHandle_t dock=0;vr::HmdVector2_t mouse_scale{};bool visible=!overlays->FindOverlay("valve.steam.gamepadui.bar",&dock)&&overlays->IsDashboardVisible()&&overlays->IsOverlayVisible(dock)&&!overlays->GetOverlayMouseScale(dock,&mouse_scale);
  if(visible&&(!was_visible||dock!=last_dock)){uv={};anchored_at={};sampled_at={};}last_dock=dock;
  // Width and aspect are only ray-solver seeds; physical size comes from intersections.
  float width=0,height=0;bool fresh=visible&&!overlays->GetOverlayWidthInMeters(dock,&width)&&std::isfinite(width)&&width>0&&width<10&&std::isfinite(mouse_scale.v[0])&&std::isfinite(mouse_scale.v[1])&&mouse_scale.v[0]>0&&mouse_scale.v[1]>0;
  if(fresh){height=width*mouse_scale.v[1]/mouse_scale.v[0];fresh=std::isfinite(height)&&height>.005f&&height<2;}
  vr::HmdMatrix34_t center{},anchor{};vr::HmdVector3_t head{};DockCurveEvidence evidence{};float size=0;bool anchor_ok=false;
  vr::HmdMatrix34_t frame{};bool frame_ok=visible&&fresh&&!overlays->GetTransformForOverlayCoordinates(dock,vr::TrackingUniverseStanding,{{mouse_scale.v[0]*.5f,mouse_scale.v[1]*.5f}},&center)&&dock_rigid_frame(center,frame);
  auto sample_now=std::chrono::steady_clock::now();
  if(frame_ok&&sample_now-sampled_at>=std::chrono::milliseconds(100)){sampled_at=sample_now;anchor_ok=dock_probe_origin(center,head)&&dock_uv_anchor(overlays,dock,center,head,width,height,anchor,evidence,size,uv);}
  auto now=std::chrono::steady_clock::now();if(anchor_ok){saved_anchor=anchor;saved_frame=frame;saved_size=size;saved_evidence=evidence;anchored_at=now;}else if(frame_ok&&now-anchored_at<std::chrono::milliseconds(350)){anchor=dock_follow_pose(saved_anchor,saved_frame,frame);size=saved_size;evidence=saved_evidence;anchor_ok=true;}
  if(keyboard_menu.holds(now)){if(views.count("menu"))show(*views.at("menu"),menu_open);}else if(anchor_ok){// Apply the button tilt in its pose: nonzero API pre-curve pitch hid it on Frame.
  auto icon_pose=anchor;float cp=std::cos(evidence.pre_curve_pitch),sp=std::sin(evidence.pre_curve_pitch);for(int r=0;r<3;r++){icon_pose.m[r][1]=anchor.m[r][1]*cp+anchor.m[r][2]*sp;icon_pose.m[r][2]=anchor.m[r][2]*cp-anchor.m[r][1]*sp;}button_placement.apply(overlays,button,icon_pose,size*5);button_placement.curve(overlays,button,size*5,evidence.radius,0);if(!overlays->IsOverlayVisible(button))overlays->ShowOverlay(button);if(views.count("menu")){MenuPlacement popup{};auto& v=*views.at("menu");vr::HmdMatrix34_t main_pose{};vr::VROverlayHandle_t main_overlay=0;vr::HmdVector2_t main_scale{};const vr::HmdMatrix34_t* main=nullptr;
   for(auto&[key,view]:views)if(view->dashboard&&view->shown&&!view->closing)main_overlay=view->overlay;
   if(!main_overlay)main_overlay=active_main_window();
   if(main_overlay&&!overlays->GetOverlayMouseScale(main_overlay,&main_scale)&&!overlays->GetTransformForOverlayCoordinates(main_overlay,vr::TrackingUniverseStanding,{{main_scale.v[0]*.5f,main_scale.v[1]*.5f}},&main_pose))main=&main_pose;
   if(evidence.radius>0?dock_menu_placement(anchor,size,evidence.radius,evidence.pre_curve_pitch,popup):menu_placement(anchor,size,popup,0,main)){v.placement.apply(overlays,v.overlay,popup.transform,popup.width);v.placement.curve(overlays,v.overlay,popup.width,popup.radius);show(v,menu_open);}else show(v,false);}}else if(!keyboard_menu.holds(now)){if(overlays->IsOverlayVisible(button))overlays->ClearOverlayCursorPositionOverride(button);overlays->HideOverlay(button);if(views.count("menu"))show(*views.at("menu"),false);}
  if(was_visible&&!visible&&!keyboard_open&&!keyboard_menu.holds(now)){menu_button.pressed=false;menu_open=false;for(auto&[key,v]:views)if(!v->dashboard&&key!="menu"&&key!="notifications")close_view(*v);}was_visible=visible;
  // Toggle once for a matched left click; never activate on a stale release.
  vr::VREvent_t event{};while(overlays->PollNextOverlayEvent(button,&event,sizeof(event))){
   laser_cursor(button,event);
   if(event.eventType==vr::VREvent_MouseButtonDown||event.eventType==vr::VREvent_MouseButtonUp){
    if(menu_button.event(event.eventType==vr::VREvent_MouseButtonDown,event.data.mouse.button==vr::VRMouseButton_Left,anchor_ok,now)){
     menu_open=!menu_open;if(menu_open){menu_button.opened(now);if(views.count("menu"))views.at("menu")->host->set_focus(views.at("menu")->host,1);}
     std::cout<<"Menu "<<(menu_open?"opened":"closed")<<std::endl;
    }
   }
  }
  for(auto&[key,v]:views){if(v->closing)continue;if(v->dashboard){bool active=overlays->IsDashboardVisible()&&overlays->IsActiveDashboardOverlay(v->overlay)&&overlays->IsOverlayVisible(v->overlay);if(active!=v->shown){v->host->was_hidden(v->host,!active);v->shown=active;}if(!active)v->active_seen=false;if(active&&!v->active_seen){v->active_seen=true;for(auto&[other,t]:views)if(!t->dashboard&&other!="menu"&&other!="notifications"&&!keyboard_open)close_view(*t);}}
   while(overlays->PollNextOverlayEvent(v->overlay,&event,sizeof(event)))overlay_input(*v,event);
   if(v->dirty&&!v->closing){
    glXMakeContextCurrent(xdisplay,glsurface,glsurface,glcontext);
    // Upload to the other texture; never redefine storage submitted this frame.
    const int slot=1-v->texture_slot;const GLuint id=v->textures[slot];
    glBindTexture(GL_TEXTURE_2D,id);
    glPixelStorei(GL_UNPACK_ALIGNMENT,4);glPixelStorei(GL_UNPACK_ROW_LENGTH,0);
    glPixelStorei(GL_UNPACK_SKIP_PIXELS,0);glPixelStorei(GL_UNPACK_SKIP_ROWS,0);
    if(!v->texture_ready[slot]){
     glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_LINEAR);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_LINEAR);
     glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_S,GL_CLAMP_TO_EDGE);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_T,GL_CLAMP_TO_EDGE);
     glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA8,v->width*v->render_scale,v->height*v->render_scale,0,GL_BGRA,GL_UNSIGNED_BYTE,nullptr);
     v->texture_ready[slot]=true;
    }
    glTexSubImage2D(GL_TEXTURE_2D,0,0,0,v->width*v->render_scale,v->height*v->render_scale,GL_BGRA,GL_UNSIGNED_BYTE,v->pixels.data());
    // glFlush queues the upload; glFinish makes the full image ready for sharing.
    glFinish();vr::Texture_t texture{reinterpret_cast<void*>(uintptr_t(id)),vr::TextureType_OpenGL,vr::ColorSpace_Auto};
    auto e=overlays->SetOverlayTexture(v->overlay,&texture);glFlush();
    if(e)std::cerr<<"Texture submission failed "<<key<<" "<<e<<"\n";
    else{v->texture_slot=slot;v->dirty=false;
     if(key=="notifications"){
      // Visibility follows the submitted frame, never a queue-change event.
      // Keep CEF painting while the overlay is hidden so a later toast can appear.
      // A transparent frame disables input even if the page stays mounted.
      auto bounds=notification_bounds(v->pixels,v->width*v->render_scale,v->height*v->render_scale,v->render_scale);
      bool visible=bounds.visible();
      if(visible){
       vr::VROverlayIntersectionMaskPrimitive_t mask{};mask.m_nPrimitiveType=vr::OverlayIntersectionPrimitiveType_Rectangle;
       mask.m_Primitive.m_Rectangle={bounds.x,bounds.y,bounds.width,bounds.height};
       auto error=overlays->SetOverlayIntersectionMask(v->overlay,&mask,1);
       if(error){std::cerr<<"Notification input mask failed "<<error<<"\n";visible=false;}
      }
      show(*v,visible);
     }
    }
   }
  }
  {json snapshot=json::object();for(auto&[key,v]:views){if(key=="menu"||key=="framely.manager"||key.rfind("framely.window.",0)==0)snapshot[key]=capture_view_visible(v->closing,overlays->IsOverlayVisible(v->overlay),v->dashboard,overlays->IsDashboardVisible(),overlays->IsActiveDashboardOverlay(v->overlay));}std::lock_guard<std::mutex> lock(visibility_mutex);visible_views=std::move(snapshot);}
  if(keyboard_close_pending){
   // A non-minimal runtime keyboard may buffer text until Done. Drain overlay
   // character events first so the global Done event cannot duplicate input.
   if(keyboard_toolbar&&keyboard_done&&!keyboard_chars){char text[16385]{};overlays->GetKeyboardText(text,sizeof(text));keyboard_edit(keyboard_frame,std::string(text,strnlen(text,sizeof(text))));}
   close_keyboard(false);
  }
  for(auto i=views.begin();i!=views.end();){auto& v=*i->second;if(v.closed){overlays->ClearOverlayTexture(v.overlay);overlays->DestroyOverlay(v.overlay);if(v.thumbnail)overlays->DestroyOverlay(v.thumbnail);glDeleteTextures(2,v.textures);i=views.erase(i);}else ++i;}
  bool hovered=anchor_ok&&overlays->IsHoverTargetOverlay(button);if(hovered&&!icon_hover)hover_haptic(button,button_haptic_at);if(icon_active!=menu_open||icon_hover!=hovered){icon_active=menu_open;pixels=dock_icon(hovered,menu_open);overlays->SetOverlayRaw(button,pixels.data(),dock_canvas_width,dock_canvas_height,4);}icon_hover=hovered;
  auto sync=overlays->WaitFrameSync(8);
  // A timeout already waited: never append another sleep to it. Bound retries
  // only for immediate API failures, so a broken sync call cannot busy-spin.
  if(sync!=vr::VROverlayError_None&&sync!=vr::VROverlayError_TimedOut)std::this_thread::sleep_until(frame_start+std::chrono::milliseconds(8));
 }
 close_keyboard();for(auto&[key,v]:views)close_view(*v);auto deadline=std::chrono::steady_clock::now()+std::chrono::seconds(5);while(std::chrono::steady_clock::now()<deadline){cef_do_message_loop_work();bool all=true;for(auto&[key,v]:views)all&=v->closed;if(all)break;std::this_thread::sleep_for(std::chrono::milliseconds(5));}poller.join();for(auto&[key,v]:views)if(!v->closed){std::cerr<<"Browser shutdown timed out\n";_Exit(7);}for(auto&[key,v]:views){overlays->DestroyOverlay(v->overlay);if(v->thumbnail)overlays->DestroyOverlay(v->thumbnail);glDeleteTextures(2,v->textures);}views.clear();overlays->DestroyOverlay(button);cef_shutdown();vr::VR_Shutdown();glXMakeContextCurrent(xdisplay,None,None,nullptr);glXDestroyContext(xdisplay,glcontext);glXDestroyPbuffer(xdisplay,glsurface);XCloseDisplay(xdisplay);return 0;
}
