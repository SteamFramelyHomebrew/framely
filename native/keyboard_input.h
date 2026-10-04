#pragma once
#include "include/capi/cef_browser_capi.h"
#include "vendor/json.hpp"
#include <string>

// Deliver edits to the originating frame, independent of keyboard overlay focus.
inline void keyboard_edit(cef_frame_t* frame,const std::string& edit){
 if(!frame)return;
 auto code="window.__framelyEditKeyboard && window.__framelyEditKeyboard("+nlohmann::json(edit).dump()+");";
 cef_string_t script{},source{};cef_string_utf8_to_utf16(code.data(),code.size(),&script);
 frame->execute_java_script(frame,&script,&source,1);cef_string_utf16_clear(&script);
}
