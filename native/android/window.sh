#!/system/bin/sh
set -eu
# podman exec lacks init's ART environment. Import only the runtime paths.
for entry in $(cat /proc/$(pidof system_server)/environ | tr '\0' '\n' | grep -E '^(ANDROID_(ROOT|DATA|ART_ROOT|I18N_ROOT|TZDATA_ROOT)|BOOTCLASSPATH)='); do
    export "$entry"
done
export CLASSPATH=/vendor/share/framely-window.dex
exec app_process /system/bin FramelyWindow "$@"
