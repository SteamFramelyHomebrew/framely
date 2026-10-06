// Read-only OpenVR application identity assertions for gamepad_identity.py.
#include "openvr.h"
#include <cstdio>
#include <cstdlib>
int main(int argc, char** argv) {
    if (argc < 3 || argc % 2 != 1) return 2;
    vr::EVRInitError error;
    vr::VR_Init(&error, vr::VRApplication_Overlay);
    if (error) return 2;
    bool ok = true;
    for (int i = 1; i < argc; i += 2) {
        auto actual = vr::VRApplications()->GetApplicationProcessId(argv[i]);
        auto expected = std::strtoul(argv[i + 1], nullptr, 10);
        if (actual != expected) {
            std::fprintf(stderr, "Application identity mismatch: expected %lu, got %u\n", expected, actual);
            ok = false;
        }
    }
    vr::VR_Shutdown();
    return ok ? 0 : 1;
}
