# This hook runs only in Framely's private Lepton adapter, never in its image.
if [[ -n "${FRAMELY_UPDATE_DIR:-}" ]]; then
    if declare -F setup_podman_mounts >/dev/null && declare -F generate_zygote_launch_rc >/dev/null; then
        eval "$(declare -f setup_podman_mounts | sed '1s/setup_podman_mounts/framely_update_original_mounts/')"
        function setup_podman_mounts() {
            framely_update_original_mounts "$@" || return
            # Replace the single original entry, retaining a forwarding endpoint.
            local i found=false
            for ((i=0; i<${#PODMAN_CMDLINE[@]}; i++)); do
                if [[ "${PODMAN_CMDLINE[i]}" == *,destination=/lepton/steam.pipe,* ]]; then
                    PODMAN_CMDLINE[i]="${PODMAN_CMDLINE[i]/,destination=\/lepton\/steam.pipe,/,destination=\/lepton\/framely-original.pipe,}"
                    found=true
                elif [[ "${PODMAN_CMDLINE[i]}" == *:/lepton/steam.pipe:* ]]; then
                    PODMAN_CMDLINE[i]="${PODMAN_CMDLINE[i]/:\/lepton\/steam.pipe:/:\/lepton\/framely-original.pipe:}"
                    found=true
                fi
            done
            [[ "$found" == true ]] || { echo 'Framely: unsupported Steam pipe mount' >&2; return 64; }
            podman_mount_entry "$FRAMELY_UPDATE_DIR" /framely-apk-updates rw
            podman_mount_entry "${FRAMELY_UPDATE_PIPE:?}" /lepton/steam.pipe rw
            podman_mount_entry "${FRAMELY_UPDATE_DEX:?}" /vendor/share/framely-updates.dex ro
            podman_mount_entry "${FRAMELY_UPDATE_SCRIPT:?}" /vendor/share/framely-updates.sh ro
            podman_mount_entry "${FRAMELY_UPDATE_PACKAGES:?}" /vendor/share/framely-update-packages ro
        }
        eval "$(declare -f generate_zygote_launch_rc | sed '1s/generate_zygote_launch_rc/framely_update_original_rc/')"
        function generate_zygote_launch_rc() {
            framely_update_original_rc "$@" || return
            cat >> "$(prefix)/init.zygote64.rc" <<'FRAMELY_UPDATE_RC'

service framely-apk-updates /system/bin/sh /vendor/share/framely-updates.sh
    class late_start
    user root
    group root
    disabled

on property:sys.boot_completed=1
    start framely-apk-updates
FRAMELY_UPDATE_RC
        }
    else
        echo 'Framely: APK update interception unavailable with this Lepton entry' >&2
    fi
fi
