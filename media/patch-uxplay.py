#!/usr/bin/env python3
"""Add only lifecycle events to UxPlay; protocol/decoder code remains upstream."""
from pathlib import Path
import sys
p=Path(sys.argv[1])/'uxplay.cpp'
s=p.read_text()
if 'framely_client_ended' not in s:
    at=s.index('extern "C" void conn_init')
    s=s[:at]+'''#include "framely/frame_sink.h"
static void framely_client_ended() {
    const char* directory=getenv("FRAMELY_CAST_DIR"),*id=getenv("FRAMELY_CAST_ID");
    if(directory&&id)FrameSink(directory,id,"AirPlay").event("ended");
}

'''+s[at:]
    s=s.replace('if (open_connections == 0) {','if (open_connections == 0) {\n        framely_client_ended();',1)
    s=s.replace('extern "C" void conn_reset (void *cls, int reason) {','extern "C" void conn_reset (void *cls, int reason) {\n    framely_client_ended();',1)
    p.write_text(s)
    from shutil import copyfile
    dst=p.parent/'framely';dst.mkdir(exist_ok=True)
    copyfile(Path(__file__).parent/'native/frame_sink.h',dst/'frame_sink.h')

# Keep one client throughout a consented session; a second device must not
# inherit the first device's permission while UxPlay still has open sockets.
s=p.read_text()
if 'framely_client_device' not in s:
    s=s.replace('#include "framely/frame_sink.h"','#include "framely/frame_sink.h"\n#include <mutex>\nstatic std::mutex framely_client_mutex;\nstatic std::string framely_client_device;')
    s=s.replace('static void framely_client_ended() {','static void framely_client_ended() {\n    {std::lock_guard<std::mutex> lock(framely_client_mutex);framely_client_device.clear();}')
    marker='    // Pass device model to renderer for device frame display'
    assert marker in s
    s=s.replace(marker,'''    if (*admit && deviceid) {
        std::lock_guard<std::mutex> lock(framely_client_mutex);
        if (framely_client_device.empty()) framely_client_device=deviceid;
        else if (framely_client_device!=deviceid) *admit=false;
    }
'''+marker)
    p.write_text(s)

# A GStreamer sink can survive successive audio sessions. Announce each valid
# media SETUP independently of its first decoded buffer.
s=p.read_text()
if 'framely_media_request' not in s:
    marker='    unsigned char type;\n    LOGI("ct=%d'
    assert marker in s
    s=s.replace(marker,'''    // framely_media_request: a new SETUP needs fresh consent after teardown.
    const char* directory=getenv("FRAMELY_CAST_DIR"),*id=getenv("FRAMELY_CAST_ID");
    if(directory&&id)FrameSink(directory,id,"AirPlay").event("request");
    unsigned char type;
    LOGI("ct=%d''',1)
    p.write_text(s)

# Mirroring may have no audio stream. Its size report arrives before decoding,
# so it must also request consent independently of the decoder's first frame.
s=p.read_text()
if 'framely_mirror_request' not in s:
    marker='extern "C" void video_report_size(void *cls, float *width_source, float *height_source, float *width, float *height) {'
    assert marker in s
    s=s.replace(marker,marker+'''
    // framely_mirror_request: notify even while the video decoder is starting.
    const char* directory=getenv("FRAMELY_CAST_DIR"),*id=getenv("FRAMELY_CAST_ID");
    if(directory&&id&&*width_source>0&&*width_source<=4096&&*height_source>0&&*height_source<=4096)
        FrameSink(directory,id,"AirPlay").event("request",(unsigned)*width_source,(unsigned)*height_source);
''',1)
    p.write_text(s)

# Classify media before a decoded buffer arrives, including URL playback.
s=p.read_text()
if 'framely_media_kind' not in s:
    marker='if(directory&&id)FrameSink(directory,id,"AirPlay").event("request");'
    assert marker in s
    s=s.replace(marker, 'if(directory&&id)FrameSink(directory,id,"AirPlay").event("request",0,0,*usingScreen?"video":"audio"); // framely_media_kind',1)
    marker='extern "C" void on_video_play(void *cls, const char* location, const float start_position) {'
    assert marker in s
    s=s.replace(marker, marker+'\n    const char* directory=getenv("FRAMELY_CAST_DIR"),*id=getenv("FRAMELY_CAST_ID");\n    if(directory&&id)FrameSink(directory,id,"AirPlay").event("request",0,0,"video");',1)
    # stdout is a file in production; retain useful errors before a restart.
    s=s.replace('    va_end(vargs);', '    va_end(vargs);\n    fflush(stdout);',1)
    p.write_text(s)
from shutil import copyfile
copyfile(Path(__file__).parent/'native/frame_sink.h',p.parent/'framely/frame_sink.h')

# A mirror SETUP is already an explicit video request; do not wait for sound
# or the first codec packet before asking for consent.
header=p.parent/'lib/raop.h'
s=header.read_text()
if 'framely_video_setup' not in s:
    marker='    void  (*video_report_size)'
    assert marker in s
    s=s.replace(marker, '    void (*framely_video_setup)(void *cls);\n'+marker,1)
    header.write_text(s)
handlers=p.parent/'lib/raop_handlers.h'
s=handlers.read_text()
if 'callbacks.framely_video_setup' not in s:
    marker='                    raop_rtp_mirror_start(conn->raop_rtp_mirror, &dport, raop->clientFPSdata);'
    assert marker in s
    s=s.replace(marker,marker+'\n                    if (raop->callbacks.framely_video_setup) raop->callbacks.framely_video_setup(raop->callbacks.cls);',1)
    handlers.write_text(s)
s=p.read_text()
if 'extern "C" void framely_video_setup' not in s:
    marker='extern "C" void video_report_size('
    at=s.index(marker)
    s=s[:at]+'''extern "C" void framely_video_setup(void *cls) {
    const char* directory=getenv("FRAMELY_CAST_DIR"),*id=getenv("FRAMELY_CAST_ID");
    if(directory&&id)FrameSink(directory,id,"AirPlay").event("request",0,0,"video");
}

'''+s[at:]
    marker='    raop_cbs.video_report_size = video_report_size;'
    assert marker in s
    s=s.replace(marker,marker+'\n    raop_cbs.framely_video_setup = framely_video_setup;',1)
    p.write_text(s)
# HLS uses playbin's separate audio sink; apply the same consent gate there.
renderer=p.parent/'renderers/video_renderer.c'
s=renderer.read_text()
if 'Framely HLS consent' not in s:
    marker='                    g_object_set(G_OBJECT (renderer_type[i]->pipeline), "video-sink", playbin_videosink, NULL);'
    assert marker in s
    s=s.replace(marker,marker+'''
                    /* Framely HLS consent: audio must use the accepted session too. */
                    if (!strcmp(videosink, "framelyvideosink")) {
                        GstElement *audio = gst_element_factory_make("framelyaudiosink", NULL);
                        g_assert(audio);
                        g_object_set(renderer_type[i]->pipeline, "audio-sink", audio, NULL);
                    }
''',1)
    renderer.write_text(s)
