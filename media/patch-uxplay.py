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
