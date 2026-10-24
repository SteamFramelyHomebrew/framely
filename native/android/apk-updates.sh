#!/system/bin/sh
# Keep the original URI path usable if the Android helper is incompatible.
if (
    set -eu
    for entry in $(cat /proc/$(pidof system_server)/environ | tr '\0' '\n' | grep -E '^(ANDROID_(ROOT|DATA|ART_ROOT|I18N_ROOT|TZDATA_ROOT)|BOOTCLASSPATH)='); do
        export "$entry"
    done
    export CLASSPATH=/vendor/share/framely-updates.dex
    exec app_process /system/bin FramelyUpdates /framely-apk-updates /vendor/share/framely-update-packages
); then
    exit 0
fi
echo 'Framely: APK update helper failed; restoring original Steam URI forwarding' >&2
while true; do
    cat /lepton/steam.pipe > /lepton/framely-original.pipe
    sleep 1
done
