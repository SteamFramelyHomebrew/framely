#pragma once
// CEF view/input coordinates stay in DIP; only the paint buffer uses this scale.
inline int render_screen_info(cef_screen_info_t* info,int width,int height,float scale){
 info->size=sizeof(*info);info->device_scale_factor=float(scale);info->depth=32;info->depth_per_component=8;info->is_monochrome=0;
 info->rect={0,0,width,height};info->available_rect=info->rect;return 1;
}
