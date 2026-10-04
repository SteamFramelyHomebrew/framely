#pragma once
// An active dashboard tab may remain selected after the dashboard is closed.
inline bool capture_view_visible(bool closing,bool overlay_visible,bool dashboard,bool dashboard_visible,bool active) {
 return !closing && overlay_visible && (!dashboard || (dashboard_visible && active));
}
