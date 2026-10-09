#include "frame_sink.h"
#include <cstdlib>
#include <fstream>
struct FramelySink {GstBin parent;FrameSink* frame;GstElement* gate;GstElement* gain;guint timer;bool audio;};
struct FramelySinkClass {GstBinClass parent;};
typedef FramelySink FramelyAudio;typedef FramelySinkClass FramelyAudioClass;
G_DEFINE_TYPE(FramelySink,framely_sink,GST_TYPE_BIN)
G_DEFINE_TYPE(FramelyAudio,framely_audio,GST_TYPE_BIN)
static gboolean tick(gpointer p){auto* s=(FramelySink*)p;bool allowed=s->frame->accepted();g_object_set(s->gate,"drop",!allowed,nullptr);
 std::ifstream volume(s->frame->directory+"/"+s->frame->id+".volume");double gain=1.;if(volume>>gain)g_object_set(s->gain,"volume",std::clamp(gain,0.,1.),nullptr);return G_SOURCE_CONTINUE;}
static void audio_frame(GstElement*,GstBuffer*,gpointer p){auto* s=(FramelySink*)p;tick(s);if(!s->frame->announced.exchange(true))s->frame->event("audio");}
static void setup(FramelySink* s,bool audio){
 s->audio=audio;GError* error=nullptr;
 auto* bin=gst_parse_bin_from_description(audio?"identity name=tap signal-handoffs=true ! valve name=gate drop=true drop-mode=transform-to-gap ! volume name=gain ! pulsesink async=false":"videoflip video-direction=auto ! videoconvert ! videoscale ! video/x-raw,pixel-aspect-ratio=1/1,format=BGRA ! appsink name=frames emit-signals=true sync=true max-buffers=1 drop=true",true,&error);
 if(!bin){g_warning("Framely sink: %s",error?error->message:"pipeline error");g_clear_error(&error);return;}
 gst_bin_add(GST_BIN(s),bin);auto* pad=gst_element_get_static_pad(bin,"sink");gst_element_add_pad(GST_ELEMENT(s),gst_ghost_pad_new("sink",pad));gst_object_unref(pad);
 if(audio){s->gate=gst_bin_get_by_name(GST_BIN(bin),"gate");s->gain=gst_bin_get_by_name(GST_BIN(bin),"gain");auto* tap=gst_bin_get_by_name(GST_BIN(bin),"tap");g_signal_connect(tap,"handoff",G_CALLBACK(audio_frame),s);gst_object_unref(tap);}
 else {auto* sink=gst_bin_get_by_name(GST_BIN(bin),"frames");g_signal_connect(sink,"new-sample",G_CALLBACK(frame_sample),s->frame);g_signal_connect(sink,"new-preroll",G_CALLBACK(frame_preroll),s->frame);gst_object_unref(sink);}
}
static void finalize(GObject* p){auto* s=(FramelySink*)p;if(s->timer)g_source_remove(s->timer);if(s->gate)gst_object_unref(s->gate);if(s->gain)gst_object_unref(s->gain);delete s->frame;G_OBJECT_CLASS(framely_sink_parent_class)->finalize(p);}
static void framely_sink_class_init(FramelySinkClass* c){G_OBJECT_CLASS(c)->finalize=finalize;gst_element_class_set_static_metadata(GST_ELEMENT_CLASS(c),"Framely video consent sink","Sink/Video","Gated AirPlay receiver","Framely");}
static void initialize(FramelySink* s,bool audio){const char* d=getenv("FRAMELY_CAST_DIR"),*i=getenv("FRAMELY_CAST_ID");s->frame=new FrameSink(d?d:"/tmp",i?i:"airplay","AirPlay");GST_OBJECT_FLAG_SET(s,GST_ELEMENT_FLAG_SINK);setup(s,audio);}
static void framely_sink_init(FramelySink* s){initialize(s,false);}
static void framely_audio_class_init(FramelyAudioClass* c){G_OBJECT_CLASS(c)->finalize=finalize;gst_element_class_set_static_metadata(GST_ELEMENT_CLASS(c),"Framely audio consent sink","Sink/Audio","Gated AirPlay receiver","Framely");}
static void framely_audio_init(FramelyAudio* s){initialize(s,true);}
static gboolean plugin_init(GstPlugin* p){return gst_element_register(p,"framelyvideosink",GST_RANK_NONE,framely_sink_get_type()) && gst_element_register(p,"framelyaudiosink",GST_RANK_NONE,framely_audio_get_type());}
#ifndef PACKAGE
#define PACKAGE "framely"
#endif
GST_PLUGIN_DEFINE(GST_VERSION_MAJOR,GST_VERSION_MINOR,framely,"Framely casting",plugin_init,"1.0","AGPL","Framely","https://github.com/SteamFramelyHomebrew/framely")
