# Always mount the shipped layer if available, but activate it only for the
# explicitly selected package. No Android/system/Lepton files are replaced.
if [[ -n "${FRAMELY_FBO_LIBRARY:-}" ]]; then
    declare -F setup_podman_mounts >/dev/null || { echo 'Unsupported Lepton graphics hooks' >&2; exit 64; }
    eval "$(declare -f setup_podman_mounts | sed '1s/setup_podman_mounts/framely_original_fbo_mounts/')"
    function setup_podman_mounts() {
        framely_original_fbo_mounts "$@" || return
        podman_mount_entry "$FRAMELY_FBO_LIBRARY" /data/local/debug/gles/libFramelyFBOCompat.so ro
    }
fi
if [[ -n "${FRAMELY_FBO_LIBRARY:-}" ]]; then
    [[ -z "${FRAMELY_FBO_PACKAGE:-}" || "$FRAMELY_FBO_PACKAGE" =~ ^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z][A-Za-z0-9_]*)+$ && -n "${FRAMELY_FBO_LIBRARY:-}" ]] || { echo 'Invalid framebuffer compatibility configuration' >&2; exit 64; }
    declare -F generate_vulkan_settings_script >/dev/null || { echo 'Unsupported Lepton graphics settings hook' >&2; exit 64; }
    eval "$(declare -f generate_vulkan_settings_script | sed '1s/generate_vulkan_settings_script/framely_original_fbo_settings/')"
    function generate_vulkan_settings_script() {
        # Lepton does not clear stale GLES settings when its GLES list is empty.
        # Remove our previous selection before its own settings are generated.
        printf '%s\n' 'if [ "$(settings get global gpu_debug_layers_gles)" = libFramelyFBOCompat.so ]; then settings delete global gpu_debug_layers_gles; settings delete global gpu_debug_layer_app; fi;'
        framely_original_fbo_settings "$@" || return
        [[ -n "${FRAMELY_FBO_PACKAGE:-}" ]] || return 0
        printf '%s\n' 'settings put global enable_gpu_debug_layers 1;' "settings put global gpu_debug_app $FRAMELY_FBO_PACKAGE;" 'settings put global gpu_debug_layers_gles libFramelyFBOCompat.so;' "settings put global gpu_debug_layer_app $FRAMELY_FBO_PACKAGE;"
    }
fi
