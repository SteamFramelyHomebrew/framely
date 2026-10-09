#pragma once
#include <string>

// Other localhost ports are only for embedded pages in plugin windows.
// The main frame remains on the Framely origin.
inline bool allow_plugin_navigation(const std::string& url, const std::string& origin,
                                    bool main_frame, bool plugin_window) {
    if (url == "about:blank" || url.rfind(origin + "/", 0) == 0) return true;
    if (main_frame || !plugin_window) return false;
    const std::string prefix = "http://localhost:";
    if (url.rfind(prefix, 0) != 0) return false;
    auto end = url.find('/', prefix.size());
    if (end == std::string::npos || end == prefix.size()) return false;
    unsigned port = 0;
    for (auto i = prefix.size(); i < end; ++i) {
        if (url[i] < '0' || url[i] > '9') return false;
        port = port * 10 + unsigned(url[i] - '0');
        if (port > 65535) return false;
    }
    return port > 0;
}

inline bool manager_navigation_page(const std::string& page) {
 return page=="casting"||page=="casting-picture"||page=="casting-receive"||page=="casting-devices"||page=="installed"||page=="catalog"||page=="sources"||page=="notification-settings"||page=="settings"||page=="launcher-settings"||page=="updates"||page=="about"||page=="terminal"||page=="files"||page=="apk"||page=="apk-containers"||page=="apk-cleanup"||page=="apk-settings";
}
