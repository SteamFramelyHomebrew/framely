#pragma once
#include <algorithm>
#include <cmath>
inline int browser_frame_rate(float refresh){
 // Preserve smooth UI on an unknown display, with a bounded update budget.
 return std::isfinite(refresh)&&refresh>0?std::clamp(int(std::round(std::clamp(refresh,60.f,144.f))),60,144):90;
}
