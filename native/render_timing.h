#pragma once
// Cap CEF painting independently of headset refresh to limit pixel readback.
inline constexpr int browser_frame_rate(float = 0){return 60;}
