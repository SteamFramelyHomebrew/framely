#pragma once
#include <stdint.h>
struct FramelyDirectControl { uint64_t expires_ns; uint32_t enabled; uint32_t rumble; };
static inline int framely_direct_enabled(struct FramelyDirectControl c,uint64_t now){return c.enabled==1&&now<c.expires_ns;}
static inline int framely_direct_rumble(struct FramelyDirectControl c,uint64_t now){return framely_direct_enabled(c,now)&&c.rumble==1;}
