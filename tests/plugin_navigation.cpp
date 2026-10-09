#include "plugin_navigation.h"
#include <cassert>
int main() {
 for (const auto& page : {"casting","casting-picture","casting-receive","casting-devices","settings","updates","about","terminal","files","apk-containers","apk-settings"}) assert(manager_navigation_page(page));
 assert(!manager_navigation_page("unknown"));
 const std::string origin="http://localhost:19626";
 assert(allow_plugin_navigation(origin+"/window",origin,true,true));
 assert(allow_plugin_navigation("about:blank",origin,false,false));
 assert(allow_plugin_navigation("http://localhost:19629/framely-window/main",origin,false,true));
 assert(allow_plugin_navigation("http://localhost:19629/framely-window/login?ticket=test",origin,false,true));
 assert(!allow_plugin_navigation("http://localhost:19629/",origin,true,true));
 assert(!allow_plugin_navigation("http://localhost:19629/",origin,false,false));
 for (const auto& url : {"https://example.com/", "http://192.168.5.67:19629/", "http://localhost.evil:19629/", "http://localhost:19629@evil/", "http://localhost:19629\\@evil/", "http://localhost:0/", "http://localhost:65536/", "http://localhost:/", "http://localhost:999999999999999999999/"})
   assert(!allow_plugin_navigation(url,origin,false,true));
}
