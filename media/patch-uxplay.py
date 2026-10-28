#!/usr/bin/env python3
"""Adapt UxPlay lifecycle events and explicit sinks; protocol/decoder code remains upstream."""
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

# Framely supplies both sinks explicitly; autodetect is not part of its pipeline.
# Keep upstream checks unchanged for all other UxPlay output modes.
audio=p.parent/'renderers/audio_renderer.c'
s=audio.read_text()
if 'framely_explicit_sinks' not in s:
    assert 'static gboolean check_plugins (void)' in s
    s=s.replace('static gboolean check_plugins (void)', 'static gboolean check_plugins (bool needs_autodetect)',1)
    marker='        GstPlugin *plugin = NULL;'
    assert marker in s
    s=s.replace(marker, '        /* framely_explicit_sinks: no automatic device selection needed. */\n        if (!needs_autodetect && !strcmp(needed[i], "autodetect")) continue;\n'+marker,1)
    s=s.replace('bool gstreamer_init(){','bool gstreamer_init(bool needs_autodetect){',1)
    s=s.replace('return (bool) check_plugins ();','return (bool) check_plugins (needs_autodetect);',1)
    audio.write_text(s)
header=p.parent/'renderers/audio_renderer.h'
s=header.read_text()
if 'bool gstreamer_init();' in s:
    header.write_text(s.replace('bool gstreamer_init();','bool gstreamer_init(bool needs_autodetect);',1))
s=p.read_text()
if 'if (!gstreamer_init())' in s:
    s=s.replace('if (!gstreamer_init())', 'if (!gstreamer_init(!(videosink == "framelyvideosink" && audiosink == "framelyaudiosink")))',1)
    p.write_text(s)

# SteamOS can disable publication in the system Avahi daemon. Use its existing
# Avahi core library inside the receiver instead; leave system policy untouched.
for name in ('airplay_mdns.h', 'airplay_mdns.cpp'):
    copyfile(Path(__file__).parent/'native'/name, p.parent/'framely'/name)
dns=p.parent/'lib/dnssd.c'
s=dns.read_text()
if 'framely_mdns_register' not in s:
    s=s.replace('#include <dns_sd.h>', '#include <dns_sd.h>\n#include "../framely/airplay_mdns.h"', 1)
    marker='    dnssd->name_len = name_len;'
    assert marker in s
    s=s.replace(marker, '    if (getenv("FRAMELY_CAST_DIR")) {\n        dnssd->DNSServiceRegister = &framely_mdns_register;\n        dnssd->DNSServiceRefDeallocate = &framely_mdns_deallocate;\n    }\n\n'+marker, 1)
    dns.write_text(s)
cmake=p.parent/'lib/CMakeLists.txt'
s=cmake.read_text()
if 'framely/airplay_mdns.cpp' not in s:
    s+='\ntarget_sources(airplay PRIVATE ../framely/airplay_mdns.cpp)\npkg_check_modules(FRAMELY_AVAHI REQUIRED IMPORTED_TARGET avahi-core)\ntarget_link_libraries(airplay PUBLIC PkgConfig::FRAMELY_AVAHI)\n'
    cmake.write_text(s)

# Keep actionable protocol transitions without logging signed media URLs.
s=p.read_text()
s=s.replace('LOGI("********************on_video_play: location = %s*** start position %f ********************", url.c_str(), start_position);',
            'LOGI("AirPlay video playback requested; start position %f (source URL omitted)", start_position);')
p.write_text(s)
router=p.parent/'lib/raop.c'
s=router.read_text()
if 'framely_media_control_trace' not in s:
    marker='    logger_log(raop->logger, LOGGER_DEBUG, "Handling request %s with URL %s", method, url);'
    assert marker in s
    s=s.replace(marker, marker+'''
    /* framely_media_control_trace: no query strings, credentials or bodies. */
    if (getenv("FRAMELY_CAST_DIR")) {
        const char *control = NULL;
        if (!strcmp(url, "/play")) control = "play";
        else if (!strcmp(url, "/stop")) control = "stop";
        else if (!strcmp(url, "/server-info")) control = "server-info";
        else if (!strncmp(url, "/rate?", 6)) control = "rate";
        else if (!strncmp(url, "/scrub?", 7)) control = "scrub";
        else if (!strcmp(method, "TEARDOWN")) control = "teardown";
        if (control) logger_log(raop->logger, LOGGER_INFO, "AirPlay media control received: %s", control);
    }
''',1)
    router.write_text(s)

# Retrying a frame older than the pipeline base must recompute from the original
# remote timestamp. Adding the wall-clock offset twice schedules video decades
# into the future when the sender's clock is based on uptime.
s=p.read_text()
if 'framely_retry_original_timestamp' not in s:
    marker='''        do {
            data->ntp_time_remote = data->ntp_time_remote + remote_clock_offset;
            pts_mismatch = video_renderer_render_buffer(data->data, &(data->data_len), &(data->nal_count), &(data->ntp_time_remote));'''
    assert marker in s
    s=s.replace(marker,'''        // framely_retry_original_timestamp: retries must not accumulate offsets.
        const uint64_t remote_timestamp = data->ntp_time_remote;
        do {
            uint64_t frame_timestamp = remote_timestamp + remote_clock_offset;
            pts_mismatch = video_renderer_render_buffer(data->data, &(data->data_len), &(data->nal_count), &frame_timestamp);''',1)
    p.write_text(s)

# Backport direct HTTP(S) Content-Location support from upstream UxPlay
# 82ad05ce34ec862851fa6ab1240f50f58fb7fa5a (lib/http_handlers.h). Keep the
# existing FCUP reverse-proxy path for non-HTTP master.m3u8 sources.
http_handlers=p.parent/'lib/http_handlers.h'
s=http_handlers.read_text()
if 'framely_direct_video_source' not in s:
    required='''    if (!req_client_proc_name_node) {
        goto play_error;
    } else {'''
    assert required in s
    s=s.replace(required, '    if (req_client_proc_name_node) {', 1)
    s=s.replace('if (!strstr(supported_hls_proc_names, client_proc_name)){',
                'if (client_proc_name && !strstr(supported_hls_proc_names, client_proc_name)){', 1)
    marker='''    /* we only support HLS if the playback location is terminated by "/master.m3u8" */'''
    assert marker in s
    s=s.replace(marker, '''    /* framely_direct_video_source: regular URLs do not use the FCUP proxy. */
    if (!strncmp(playback_location, "http://", 7) || !strncmp(playback_location, "https://", 8)) {
        set_playback_location(airplay_video, playback_location, strlen(playback_location));
        raop->callbacks.on_video_play(raop->callbacks.cls,
                                      get_playback_location(airplay_video),
                                      start_position_seconds);
        plist_mem_free(playback_location);
        plist_free(req_root_node);
        return;
    }

'''+marker, 1)
    s=s.replace('"Content-Location has unsupported form:\\n%s\\n", playback_location',
                '"Unsupported AirPlay video source format (URL omitted)"')
    http_handlers.write_text(s)
