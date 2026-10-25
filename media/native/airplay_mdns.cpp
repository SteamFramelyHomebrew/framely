// SPDX-License-Identifier: AGPL-3.0-only
// Publish only UxPlay's records, independently of the system Avahi D-Bus policy.
#include "airplay_mdns.h"
#include <avahi-core/core.h>
#include <avahi-core/publish.h>
#include <avahi-core/log.h>
#include <avahi-common/alternative.h>
#include <avahi-common/error.h>
#include <avahi-common/malloc.h>
#include <avahi-common/strlst.h>
#include <avahi-common/thread-watch.h>
#include <arpa/inet.h>
#include <unistd.h>
#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
struct Service {
    std::string name, type;
    uint16_t port;
    AvahiStringList* txt = nullptr;
    AvahiSEntryGroup* group = nullptr;
    ~Service() {
        if (group) avahi_s_entry_group_free(group);
        avahi_string_list_free(txt);
    }
};
class Publisher {
    AvahiThreadedPoll* poll = nullptr;
    AvahiServer* server = nullptr;
    std::vector<std::unique_ptr<Service>> services;
    static void group_state(AvahiServer* server, AvahiSEntryGroup*, AvahiEntryGroupState state, void* data) {
        auto* service = static_cast<Service*>(data);
        if (state == AVAHI_ENTRY_GROUP_ESTABLISHED)
            std::fprintf(stderr, "AirPlay mDNS: published %s\n", service->type.c_str());
        else if (state == AVAHI_ENTRY_GROUP_COLLISION || state == AVAHI_ENTRY_GROUP_FAILURE) {
            std::fprintf(stderr, "AirPlay mDNS: cannot publish %s: %s\n", service->type.c_str(),
                state == AVAHI_ENTRY_GROUP_COLLISION ? "service name conflict" : avahi_strerror(avahi_server_errno(server)));
            std::exit(1);
        }
    }
    void publish(Service& service) {
        if (!service.group) service.group = avahi_s_entry_group_new(server, group_state, &service);
        if (!service.group) throw std::runtime_error(avahi_strerror(avahi_server_errno(server)));
        if (!avahi_s_entry_group_is_empty(service.group)) return;
        int error = avahi_server_add_service_strlst(server, service.group, AVAHI_IF_UNSPEC,
            AVAHI_PROTO_UNSPEC, AvahiPublishFlags(0), service.name.c_str(), service.type.c_str(),
            nullptr, nullptr, service.port, service.txt);
        if (error >= 0) error = avahi_s_entry_group_commit(service.group);
        if (error < 0) throw std::runtime_error(avahi_strerror(error));
    }
    static void server_state(AvahiServer* server, AvahiServerState state, void* data) {
        auto* self = static_cast<Publisher*>(data);
        // avahi_server_new can invoke us before it returns.
        self->server = server;
        try {
            if (state == AVAHI_SERVER_RUNNING) {
                for (auto& service : self->services) self->publish(*service);
            } else if (state == AVAHI_SERVER_REGISTERING || state == AVAHI_SERVER_COLLISION) {
                for (auto& service : self->services)
                    if (service->group) avahi_s_entry_group_reset(service->group);
                if (state == AVAHI_SERVER_COLLISION) {
                    char* name = avahi_alternative_host_name(avahi_server_get_host_name(server));
                    int error = avahi_server_set_host_name(server, name);
                    avahi_free(name);
                    if (error < 0) throw std::runtime_error(avahi_strerror(error));
                }
            } else if (state == AVAHI_SERVER_FAILURE) {
                throw std::runtime_error(avahi_strerror(avahi_server_errno(server)));
            }
        } catch (const std::exception& error) {
            std::fprintf(stderr, "AirPlay mDNS: %s\n", error.what());
            std::exit(1);
        }
    }
public:
    Publisher() {
        avahi_set_log_function([](AvahiLogLevel level, const char* message) {
            if (level <= AVAHI_LOG_WARN) std::fprintf(stderr, "AirPlay mDNS: %s\n", message);
        });
        poll = avahi_threaded_poll_new();
        if (!poll) throw std::runtime_error("cannot create mDNS poll");
        AvahiServerConfig config;
        avahi_server_config_init(&config);
        // Share the machine hostname with other local mDNS stacks, avoiding
        // conflicting reverse address records for the same network interfaces.
        config.publish_hinfo = config.publish_workstation = config.publish_domain = 0;
        config.enable_wide_area = 0;
        int error = 0;
        server = avahi_server_new(avahi_threaded_poll_get(poll), &config, server_state, this, &error);
        avahi_server_config_free(&config);
        if (!server || avahi_threaded_poll_start(poll) < 0) {
            if (server) avahi_server_free(server);
            avahi_threaded_poll_free(poll);
            throw std::runtime_error(error ? avahi_strerror(error) : "cannot start mDNS poll");
        }
    }
    ~Publisher() {
        avahi_threaded_poll_stop(poll);
        services.clear();
        avahi_server_free(server);
        avahi_threaded_poll_free(poll);
    }
    Service* add(const char* name, const char* type, uint16_t port, const void* txt, uint16_t size) {
        std::unique_ptr<Service> service(new Service);
        service->name = name; service->type = type; service->port = ntohs(port);
        if (avahi_string_list_parse(txt, size, &service->txt) < 0)
            throw std::runtime_error("invalid mDNS TXT record");
        avahi_threaded_poll_lock(poll);
        try {
            if (avahi_server_get_state(server) == AVAHI_SERVER_RUNNING) publish(*service);
            auto* result = service.get();
            services.push_back(std::move(service));
            avahi_threaded_poll_unlock(poll);
            return result;
        } catch (...) {
            // Free any registered group while its poll is still locked.
            service.reset();
            avahi_threaded_poll_unlock(poll);
            throw;
        }
    }
    void remove(Service* service) {
        avahi_threaded_poll_lock(poll);
        services.erase(std::remove_if(services.begin(), services.end(),
            [service](const std::unique_ptr<Service>& item) { return item.get() == service; }), services.end());
        avahi_threaded_poll_unlock(poll);
    }
};
Publisher& publisher() { static Publisher instance; return instance; }
}
extern "C" DNSServiceErrorType framely_mdns_register(DNSServiceRef* result, DNSServiceFlags flags,
    uint32_t interface_index, const char* name, const char* type, const char* domain, const char* host,
    uint16_t port, uint16_t size, const void* txt, DNSServiceRegisterReply callback, void*) {
    if (!result || !name || !type || flags || interface_index || domain || host || callback)
        return kDNSServiceErr_BadParam;
    try {
        *result = reinterpret_cast<DNSServiceRef>(publisher().add(name, type, port, txt, size));
        return kDNSServiceErr_NoError;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "AirPlay mDNS initialization failed: %s\n", error.what());
        return kDNSServiceErr_Unknown;
    }
}
extern "C" void framely_mdns_deallocate(DNSServiceRef service) {
    publisher().remove(reinterpret_cast<Service*>(service));
}
