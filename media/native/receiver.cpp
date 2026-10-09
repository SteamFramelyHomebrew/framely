#include "frame_sink.h"
#include <libgupnp/gupnp.h>
#include "json.hpp"
#include <map>
#include <memory>
#include <iostream>
#include <fstream>
#include <cstdio>
#include <mutex>
#include <ifaddrs.h>
#include <net/if.h>

using json = nlohmann::json;
static std::string directory, current, own_udn, xml_directory;
static GMainLoop* loop;
static std::mutex output_mutex;
static bool receive_enabled = false;
static constexpr const char* protocols = "http-get:*:video/mp4:*,http-get:*:audio/mpeg:*,http-get:*:audio/mp4:*,http-get:*:audio/flac:*,http-get:*:audio/wav:*,http-get:*:application/vnd.apple.mpegurl:*,http-get:*:application/x-mpegURL:*,http-get:*:video/mpeg:*";
static void output(json j) { std::lock_guard<std::mutex> lock(output_mutex); std::cout << j.dump() << std::endl; }
struct Media {
    FrameSink sink;
    GstElement* pipeline = nullptr;
    std::string uri, metadata;
    bool accepted = false, paused = false, muted = false;
    double volume = 1.;
    guint watch = 0;
    Media(std::string id, std::string u, std::string m): sink(directory,id,"DLNA"), uri(u), metadata(m) {}
    ~Media() {
        if (watch) g_source_remove(watch);
        if (pipeline) { gst_element_set_state(pipeline,GST_STATE_NULL); gst_object_unref(pipeline); }
    }
};
static std::map<std::string,std::unique_ptr<Media>> sessions;
static std::map<std::string,GUPnPServiceProxy*> devices;
static std::vector<GUPnPService*> transports, renderers;
static Media* active() { auto i = sessions.find(current); return i == sessions.end() ? nullptr : i->second.get(); }
static std::string state() { auto* m = active(); return !m ? "NO_MEDIA_PRESENT" : !m->accepted ? "STOPPED" : m->paused ? "PAUSED_PLAYBACK" : "PLAYING"; }
static gint64 position(Media* m, bool duration) {
    gint64 value = 0;
    if (m && m->pipeline) {
        if (duration) gst_element_query_duration(m->pipeline,GST_FORMAT_TIME,&value);
        else gst_element_query_position(m->pipeline,GST_FORMAT_TIME,&value);
    }
    return std::max(gint64(0),value);
}
static std::string time_string(gint64 ns) {
    auto seconds = ns / GST_SECOND;
    char text[48]; snprintf(text,sizeof(text),"%02lld:%02lld:%02lld",(long long)(seconds/3600),(long long)(seconds/60%60),(long long)(seconds%60));
    return text;
}
static std::string last_change(bool rendering) {
    std::string values;
    if (rendering) {
        auto* m = active();
        values = "<Volume channel=\"Master\" val=\"" + std::to_string(m ? unsigned(m->volume*100) : 100) + "\"/><Mute channel=\"Master\" val=\"" + std::string(m && m->muted ? "1" : "0") + "\"/>";
    } else values = "<TransportState val=\"" + state() + "\"/><TransportStatus val=\"OK\"/><TransportPlaySpeed val=\"1\"/>";
    return "<Event xmlns=\"urn:schemas-upnp-org:metadata-1-0/" + std::string(rendering ? "RCS" : "AVT") + "/\"><InstanceID val=\"0\">" + values + "</InstanceID></Event>";
}
static void notify() {
    for (auto* service : transports) { auto value = last_change(false); gupnp_service_notify(service,"LastChange",G_TYPE_STRING,value.c_str(),nullptr); }
    for (auto* service : renderers) { auto value = last_change(true); gupnp_service_notify(service,"LastChange",G_TYPE_STRING,value.c_str(),nullptr); }
}
static void erase_media(const std::string& id) {
    sessions.erase(id); unlink((directory+"/"+id+".accept").c_str());
    if (current == id) current.clear();
    notify();
}
static gboolean erase_later(gpointer data) { std::unique_ptr<std::string> id((std::string*)data); erase_media(*id); return G_SOURCE_REMOVE; }
static gboolean bus(GstBus*, GstMessage* message, gpointer data) {
    auto* media = (Media*)data;
    if (GST_MESSAGE_TYPE(message) == GST_MESSAGE_ERROR || GST_MESSAGE_TYPE(message) == GST_MESSAGE_EOS) {
        if (GST_MESSAGE_TYPE(message) == GST_MESSAGE_ERROR) {
            GError* error=nullptr; gchar* debug=nullptr; gst_message_parse_error(message,&error,&debug);
            output({{"event","error"},{"id",media->sink.id},{"error",error ? error->message : "Media playback failed"}});
            g_clear_error(&error); g_free(debug);
        } else output({{"event","ended"},{"id",media->sink.id}});
        // Destroy on the main loop after this callback has returned. NULL state joins
        // streaming threads before their FrameSink and Media pointers are released.
        media->watch=0;
        g_idle_add(erase_later,new std::string(media->sink.id));
        return G_SOURCE_REMOVE;
    }
    return G_SOURCE_CONTINUE;
}
static void streams_changed(GstElement* p, gpointer data) {
    auto* m=(Media*)data; gint video=0,audio=0; g_object_get(p,"n-video",&video,"n-audio",&audio,nullptr);
    if (audio>0 && video==0 && !m->sink.announced.exchange(true)) m->sink.event("audio");
}
static void play(Media& media) {
    if (!media.pipeline) {
        media.pipeline=gst_element_factory_make("playbin",nullptr);
        if (!media.pipeline) throw std::runtime_error("GStreamer playbin unavailable");
        gst_object_ref_sink(media.pipeline);
        g_signal_connect(media.pipeline,"audio-changed",G_CALLBACK(streams_changed),&media);
        g_signal_connect(media.pipeline,"video-changed",G_CALLBACK(streams_changed),&media);
        GError* error=nullptr;
        auto* video=gst_parse_bin_from_description("videoflip video-direction=auto ! videoconvert ! videoscale ! video/x-raw,pixel-aspect-ratio=1/1,format=BGRA ! appsink name=frames emit-signals=true max-buffers=1 drop=true sync=true",true,&error);
        if (!video) { std::string reason=error ? error->message : "GStreamer video sink unavailable"; g_clear_error(&error); throw std::runtime_error(reason); }
        gst_object_ref_sink(video);
        auto* sink=gst_bin_get_by_name(GST_BIN(video),"frames");
        g_signal_connect(sink,"new-sample",G_CALLBACK(frame_sample),&media.sink);
        g_signal_connect(sink,"new-preroll",G_CALLBACK(frame_preroll),&media.sink); gst_object_unref(sink);
        auto* audio=gst_element_factory_make("pulsesink",nullptr);
        if (!audio) { gst_object_unref(video); throw std::runtime_error("GStreamer PulseAudio sink unavailable"); }
        gst_object_ref_sink(audio);
        g_object_set(media.pipeline,"uri",media.uri.c_str(),"video-sink",video,"audio-sink",audio,"volume",media.volume,"mute",media.muted,nullptr);
        gst_object_unref(audio); gst_object_unref(video);
        auto* b=gst_element_get_bus(media.pipeline); media.watch=gst_bus_add_watch(b,bus,&media); gst_object_unref(b);
    }
    if (gst_element_set_state(media.pipeline,media.paused ? GST_STATE_PAUSED : GST_STATE_PLAYING)==GST_STATE_CHANGE_FAILURE)
        throw std::runtime_error("Media could not start playback");
}
static bool seekable(Media& m) {
    if(!m.pipeline)return false;
    auto* query=gst_query_new_seeking(GST_FORMAT_TIME);gboolean allowed=false;
    if(gst_element_query(m.pipeline,query))gst_query_parse_seeking(query,nullptr,&allowed,nullptr,nullptr);
    gst_query_unref(query);return allowed;
}
static void seek(Media& m, double seconds) {
    if (!seekable(m) || !std::isfinite(seconds) || seconds<0 || seconds>86400*365 ||
        !gst_element_seek_simple(m.pipeline,GST_FORMAT_TIME,(GstSeekFlags)(GST_SEEK_FLAG_FLUSH|GST_SEEK_FLAG_ACCURATE),gint64(seconds*GST_SECOND)))
        throw std::runtime_error("This media cannot seek to the requested position");
}
static void set_volume(Media& m, double volume) {
    if (!std::isfinite(volume) || volume<0 || volume>1) throw std::runtime_error("Invalid volume");
    m.volume=volume; if (m.pipeline) g_object_set(m.pipeline,"volume",volume,nullptr); notify();
}
static void invoke(GUPnPServiceProxy* proxy,GUPnPServiceProxyAction* action) {
    GError* error=nullptr;
    if (!gupnp_service_proxy_call_action(proxy,action,nullptr,&error)) {
        std::string message=error ? error->message : "UPnP action failed"; g_clear_error(&error); gupnp_service_proxy_action_unref(action); throw std::runtime_error(message);
    }
    gupnp_service_proxy_action_unref(action);
}
static gboolean input(GIOChannel* channel,GIOCondition condition,gpointer) {
    if (condition&G_IO_HUP) { g_main_loop_quit(loop); return G_SOURCE_REMOVE; }
    gchar* line=nullptr; gsize length=0;
    if (g_io_channel_read_line(channel,&line,&length,nullptr,nullptr)!=G_IO_STATUS_NORMAL) return G_SOURCE_CONTINUE;
    std::unique_ptr<char,decltype(&g_free)> owner(line,g_free); json request;
    try {
        request=json::parse(line); std::string kind=request.value("kind",""), id=request.value("id",""); json result=true;
        if (kind=="devices") {
            result=json::array();
            for (auto& [id,p]:devices) {
                auto* info=GUPNP_DEVICE_INFO(g_object_get_data(G_OBJECT(p),"device")); auto* name=gupnp_device_info_get_friendly_name(info);
                auto* context=gupnp_service_info_get_context(GUPNP_SERVICE_INFO(p));
                result.push_back({{"id",id},{"name",name ? name : id},{"localAddress",gssdp_client_get_host_ip(GSSDP_CLIENT(context))}}); g_free(name);
            }
        } else if (kind=="send" || kind=="send.stop") {
            auto it=devices.find(id); if (it==devices.end()) throw std::runtime_error("Device is no longer available");
            if (kind=="send") {
                invoke(it->second,gupnp_service_proxy_action_new("SetAVTransportURI","InstanceID",G_TYPE_UINT,0,"CurrentURI",G_TYPE_STRING,request.at("uri").get<std::string>().c_str(),"CurrentURIMetaData",G_TYPE_STRING,"",nullptr));
                invoke(it->second,gupnp_service_proxy_action_new("Play","InstanceID",G_TYPE_UINT,0,"Speed",G_TYPE_STRING,"1",nullptr));
            } else invoke(it->second,gupnp_service_proxy_action_new("Stop","InstanceID",G_TYPE_UINT,0,nullptr));
        } else {
            auto it=sessions.find(id); if (it==sessions.end()) throw std::runtime_error("Media request expired"); auto& m=*it->second;
            if (kind=="accept") {
                if (m.accepted) throw std::runtime_error("Media already accepted");
                std::ofstream gate(directory+"/"+id+".accept"); if (!gate) throw std::runtime_error("Cannot accept media"); gate.close();
                m.accepted=true; play(m); notify();
            } else if (kind=="stop" || kind=="reject") { erase_media(id); output({{"event","ended"},{"id",id}}); }
            else {
                if (!m.accepted) throw std::runtime_error("Media has not been accepted");
                if (kind=="pause") { m.paused=request.value("paused",true); gst_element_set_state(m.pipeline,m.paused ? GST_STATE_PAUSED : GST_STATE_PLAYING); notify(); }
                else if (kind=="volume") set_volume(m,request.value("volume",1.));
                else if (kind=="seek") seek(m,request.value("seconds",0.));
                else if (kind=="status") result={{"position",position(&m,false)/double(GST_SECOND)},{"duration",position(&m,true)/double(GST_SECOND)},{"paused",m.paused},{"volume",m.volume},{"seekable",seekable(m)}};
                else throw std::runtime_error("Unknown media action");
            }
        }
        output({{"request",request.value("request",0)},{"result",result}});
    } catch (const std::exception& e) { output({{"request",request.value("request",0)},{"error",e.what()}}); }
    return G_SOURCE_CONTINUE;
}
static void available(GUPnPControlPoint*,GUPnPDeviceProxy* device,gpointer) {
    auto* service=gupnp_device_info_get_service(GUPNP_DEVICE_INFO(device),"urn:schemas-upnp-org:service:AVTransport:1"); if (!service) return;
    std::string id=gupnp_device_info_get_udn(GUPNP_DEVICE_INFO(device));
    if (id==own_udn) { g_object_unref(service); return; }
    if (devices.count(id)) g_object_unref(devices[id]);
    g_object_set_data_full(G_OBJECT(service),"device",g_object_ref(device),g_object_unref); devices[id]=GUPNP_SERVICE_PROXY(service);
}
static void unavailable(GUPnPControlPoint*,GUPnPDeviceProxy* device,gpointer) {
    std::string id=gupnp_device_info_get_udn(GUPNP_DEVICE_INFO(device)); auto it=devices.find(id);
    if (it!=devices.end() && g_object_get_data(G_OBJECT(it->second),"device")==device) { g_object_unref(it->second); devices.erase(it); }
}
static void action(GUPnPService*,GUPnPServiceAction* a,gpointer) {
    const char* name=gupnp_service_action_get_name(a);
    try {
        if (strcmp(name,"GetProtocolInfo") && strncmp(name,"GetCurrentConnection",20)) {
            guint instance=0; gupnp_service_action_get(a,"InstanceID",G_TYPE_UINT,&instance,nullptr);
            if (instance!=0) { gupnp_service_action_return_error(a,718,"Invalid InstanceID"); return; }
        }
        if (!strcmp(name,"SetAVTransportURI")) {
            gchar *uri=nullptr,*metadata=nullptr; gupnp_service_action_get(a,"CurrentURI",G_TYPE_STRING,&uri,"CurrentURIMetaData",G_TYPE_STRING,&metadata,nullptr);
            std::string u=uri ? uri : "", meta=metadata ? metadata : ""; g_free(uri); g_free(metadata);
            if (u.size()>8192 || meta.size()>65536 || (u.rfind("http://",0)!=0 && u.rfind("https://",0)!=0)) throw std::runtime_error("Only HTTP media URLs are supported");
            if (sessions.size()>=16) throw std::runtime_error("Window limit reached");
            gchar* id=g_uuid_string_random(); current=id; g_free(id);
            sessions[current]=std::make_unique<Media>(current,u,meta); notify();
            output({{"event","request"},{"id",current},{"protocol","DLNA"},{"uri",u}});
        } else if (!strcmp(name,"GetTransportInfo")) gupnp_service_action_set(a,"CurrentTransportState",G_TYPE_STRING,state().c_str(),"CurrentTransportStatus",G_TYPE_STRING,"OK","CurrentSpeed",G_TYPE_STRING,"1",nullptr);
        else if (!strcmp(name,"GetMediaInfo")) { auto* m=active(); auto duration=time_string(position(m,true)); gupnp_service_action_set(a,"NrTracks",G_TYPE_UINT,m ? 1u : 0u,"MediaDuration",G_TYPE_STRING,duration.c_str(),"CurrentURI",G_TYPE_STRING,m ? m->uri.c_str() : "","CurrentURIMetaData",G_TYPE_STRING,m ? m->metadata.c_str() : "","NextURI",G_TYPE_STRING,"","NextURIMetaData",G_TYPE_STRING,"","PlayMedium",G_TYPE_STRING,"NETWORK","RecordMedium",G_TYPE_STRING,"NOT_IMPLEMENTED","WriteStatus",G_TYPE_STRING,"NOT_IMPLEMENTED",nullptr); }
        else if (!strcmp(name,"GetPositionInfo")) { auto* m=active(); auto duration=time_string(position(m,true)), pos=time_string(position(m,false)); gupnp_service_action_set(a,"Track",G_TYPE_UINT,m ? 1u : 0u,"TrackDuration",G_TYPE_STRING,duration.c_str(),"TrackMetaData",G_TYPE_STRING,m ? m->metadata.c_str() : "","TrackURI",G_TYPE_STRING,m ? m->uri.c_str() : "","RelTime",G_TYPE_STRING,pos.c_str(),"AbsTime",G_TYPE_STRING,pos.c_str(),"RelCount",G_TYPE_INT,0,"AbsCount",G_TYPE_INT,0,nullptr); }
        else if (!strcmp(name,"GetDeviceCapabilities")) gupnp_service_action_set(a,"PlayMedia",G_TYPE_STRING,"NETWORK","RecMedia",G_TYPE_STRING,"NOT_IMPLEMENTED","RecQualityModes",G_TYPE_STRING,"NOT_IMPLEMENTED",nullptr);
        else if (!strcmp(name,"GetTransportSettings")) gupnp_service_action_set(a,"PlayMode",G_TYPE_STRING,"NORMAL","RecQualityMode",G_TYPE_STRING,"NOT_IMPLEMENTED",nullptr);
        else if (!strcmp(name,"GetCurrentTransportActions")) gupnp_service_action_set(a,"Actions",G_TYPE_STRING,active() && active()->accepted ? "Play,Pause,Stop,Seek" : "Play,Stop",nullptr);
        else if (!strcmp(name,"GetProtocolInfo")) gupnp_service_action_set(a,"Source",G_TYPE_STRING,"","Sink",G_TYPE_STRING,protocols,nullptr);
        else if (!strcmp(name,"GetCurrentConnectionIDs")) gupnp_service_action_set(a,"ConnectionIDs",G_TYPE_STRING,"0",nullptr);
        else if (!strcmp(name,"GetCurrentConnectionInfo")) gupnp_service_action_set(a,"RcsID",G_TYPE_INT,0,"AVTransportID",G_TYPE_INT,0,"ProtocolInfo",G_TYPE_STRING,"http-get:*:*:*","PeerConnectionManager",G_TYPE_STRING,"","PeerConnectionID",G_TYPE_INT,-1,"Direction",G_TYPE_STRING,"Input","Status",G_TYPE_STRING,"OK",nullptr);
        else if (!strcmp(name,"GetVolume")) gupnp_service_action_set(a,"CurrentVolume",G_TYPE_UINT,active() ? unsigned(active()->volume*100) : 100u,nullptr);
        else if (!strcmp(name,"GetMute")) gupnp_service_action_set(a,"CurrentMute",G_TYPE_BOOLEAN,active() && active()->muted,nullptr);
        else {
            auto* m=active(); if (!m) { gupnp_service_action_return_error(a,701,"No media present"); return; }
            if (!strcmp(name,"Stop")) { auto id=current; erase_media(id); output({{"event","ended"},{"id",id}}); }
            else if (!strcmp(name,"Pause")) { m->paused=true; if (m->pipeline) gst_element_set_state(m->pipeline,GST_STATE_PAUSED); notify(); }
            else if (!strcmp(name,"Play")) { gchar* speed=nullptr; gupnp_service_action_get(a,"Speed",G_TYPE_STRING,&speed,nullptr); bool supported=speed && !strcmp(speed,"1"); g_free(speed); if (!supported) { gupnp_service_action_return_error(a,717,"Unsupported play speed"); return; } m->paused=false; if (m->accepted) play(*m); notify(); }
            else if (!strcmp(name,"Seek")) { gchar *unit=nullptr,*target=nullptr; gupnp_service_action_get(a,"Unit",G_TYPE_STRING,&unit,"Target",G_TYPE_STRING,&target,nullptr); std::string u=unit ? unit : "",t=target ? target : ""; g_free(unit); g_free(target); unsigned h=0,min=0; double sec=0; int count=0; if (u!="REL_TIME" || sscanf(t.c_str(),"%u:%u:%lf%n",&h,&min,&sec,&count)!=3 || count!=int(t.size()) || min>=60 || sec<0 || sec>=60) throw std::runtime_error("Unsupported seek target"); seek(*m,h*3600. + min*60. + sec); }
            else if (!strcmp(name,"SetVolume")) { guint value=100; gupnp_service_action_get(a,"DesiredVolume",G_TYPE_UINT,&value,nullptr); set_volume(*m,std::min(value,100u)/100.); }
            else if (!strcmp(name,"SetMute")) { gboolean value=false; gupnp_service_action_get(a,"DesiredMute",G_TYPE_BOOLEAN,&value,nullptr); m->muted=value; if (m->pipeline) g_object_set(m->pipeline,"mute",value,nullptr); notify(); }
        }
        gupnp_service_action_return_success(a);
    } catch (const std::exception& e) { gupnp_service_action_return_error(a,714,e.what()); }
}
static void query_variable(GUPnPService*,const char* variable,GValue* value,gpointer rendering) {
    g_value_init(value,G_TYPE_STRING);
    if (!strcmp(variable,"LastChange")) { auto text=last_change(GPOINTER_TO_INT(rendering)); g_value_set_string(value,text.c_str()); }
    else if (!strcmp(variable,"SinkProtocolInfo")) g_value_set_string(value,protocols);
    else if (!strcmp(variable,"CurrentConnectionIDs")) g_value_set_string(value,"0");
    else g_value_set_string(value,"");
}
static bool lan_context(GUPnPContext* context) {
    ifaddrs* first=nullptr; if (getifaddrs(&first)) return false;
    bool result=false; auto* name=gssdp_client_get_interface(GSSDP_CLIENT(context));
    for (auto* i=first;i;i=i->ifa_next) if (i->ifa_name && name && !strcmp(i->ifa_name,name) && (i->ifa_flags&IFF_UP) && (i->ifa_flags&IFF_RUNNING) && (i->ifa_flags&IFF_BROADCAST) && !(i->ifa_flags&(IFF_LOOPBACK|IFF_POINTOPOINT))) result=true;
    freeifaddrs(first); return result;
}
static void context_available(GUPnPContextManager* manager,GUPnPContext* context,gpointer) {
    if (!lan_context(context)) return;
    try {
        g_object_set(gupnp_context_get_session(context),"timeout",5u,nullptr);
        auto* cp=gupnp_control_point_new(context,"urn:schemas-upnp-org:device:MediaRenderer:1");
        g_signal_connect(cp,"device-proxy-available",G_CALLBACK(available),nullptr); g_signal_connect(cp,"device-proxy-unavailable",G_CALLBACK(unavailable),nullptr);
        gupnp_context_manager_manage_control_point(manager,cp); gssdp_resource_browser_set_active(GSSDP_RESOURCE_BROWSER(cp),true); g_object_unref(cp);
        if (receive_enabled) {
            GError* error=nullptr; auto* root=gupnp_root_device_new(context,"device.xml",xml_directory.c_str(),&error);
            if (!root) { std::string reason=error ? error->message : "DLNA description error"; g_clear_error(&error); throw std::runtime_error(reason); }
            own_udn=gupnp_device_info_get_udn(GUPNP_DEVICE_INFO(root));
            for (auto type:{"AVTransport","RenderingControl","ConnectionManager"}) {
                std::string urn="urn:schemas-upnp-org:service:"+std::string(type)+":1"; auto* service=gupnp_device_info_get_service(GUPNP_DEVICE_INFO(root),urn.c_str());
                if (!service) throw std::runtime_error("Missing DLNA service");
                for (auto name:{"SetAVTransportURI","Play","Pause","Stop","Seek","GetTransportInfo","GetMediaInfo","GetPositionInfo","GetDeviceCapabilities","GetTransportSettings","GetCurrentTransportActions","GetProtocolInfo","GetCurrentConnectionIDs","GetCurrentConnectionInfo","GetVolume","GetMute","SetVolume","SetMute"}) {
                    std::string signal="action-invoked::"+std::string(name); g_signal_connect(service,signal.c_str(),G_CALLBACK(action),nullptr);
                }
                g_signal_connect(service,"query-variable",G_CALLBACK(query_variable),GINT_TO_POINTER(!strcmp(type,"RenderingControl")));
                if (!strcmp(type,"AVTransport")) transports.push_back(GUPNP_SERVICE(service));
                else if (!strcmp(type,"RenderingControl")) renderers.push_back(GUPNP_SERVICE(service));
                else g_object_unref(service);
            }
            gupnp_context_manager_manage_root_device(manager,root); gupnp_root_device_set_available(root,true); g_object_unref(root);
        }
        output({{"event","ready"},{"port",gupnp_context_get_port(context)},{"host",gssdp_client_get_host_ip(GSSDP_CLIENT(context))}});
    } catch (const std::exception& e) { output({{"event","fatal"},{"error",e.what()}}); }
}
int main(int argc,char** argv) {
    try {
        if (argc<4) throw std::runtime_error("receiver DIRECTORY XMLDIR RECEIVE");
        directory=argv[1]; xml_directory=argv[2]; receive_enabled=!strcmp(argv[3],"1"); gst_init(&argc,&argv);
        auto* manager=gupnp_context_manager_create_full(GSSDP_UDA_VERSION_1_0,G_SOCKET_FAMILY_IPV4,0);
        g_signal_connect(manager,"context-available",G_CALLBACK(context_available),nullptr);
        loop=g_main_loop_new(nullptr,false); auto* channel=g_io_channel_unix_new(STDIN_FILENO); g_io_add_watch(channel,(GIOCondition)(G_IO_IN|G_IO_HUP),input,nullptr);
        g_main_loop_run(loop); sessions.clear();
        for (auto& [_,p]:devices) g_object_unref(p);
        for (auto* p:transports) g_object_unref(p);
        for (auto* p:renderers) g_object_unref(p);
        g_io_channel_unref(channel); g_object_unref(manager); g_main_loop_unref(loop); return 0;
    } catch (const std::exception& e) { output({{"event","fatal"},{"error",e.what()}}); return 1; }
}
