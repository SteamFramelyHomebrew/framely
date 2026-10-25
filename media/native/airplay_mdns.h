#pragma once
#include <dns_sd.h>
#ifdef __cplusplus
extern "C" {
#endif
DNSServiceErrorType framely_mdns_register(DNSServiceRef*, DNSServiceFlags, uint32_t,
    const char*, const char*, const char*, const char*, uint16_t, uint16_t,
    const void*, DNSServiceRegisterReply, void*);
void framely_mdns_deallocate(DNSServiceRef);
#ifdef __cplusplus
}
#endif
