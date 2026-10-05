#pragma once
#include "include/capi/cef_frame_capi.h"
#include "native_scroll_script.h"
// Run in every newly loaded frame; no network request, SDK or bridge required.
inline void inject_page_scroll(cef_frame_t* frame){
 if(!frame||!frame->is_valid(frame))return;
 cef_string_t script{},source{};
 cef_string_utf8_to_utf16(native_scroll_script,sizeof(native_scroll_script)-1,&script);
 frame->execute_java_script(frame,&script,&source,1);
 cef_string_utf16_clear(&script);
}
