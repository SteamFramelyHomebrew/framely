#pragma once
#include <cmath>
#include <cstdint>
#include <optional>

constexpr float cast_window_initial_height = 1.8f;
inline float cast_window_initial_width(int width, int height) {
    return cast_window_initial_height * float(width) / height;
}
// A pixel-only resolution change must not resize the physical window.
inline bool cast_window_aspect_changed(int old_width, int old_height, int width, int height) {
    return int64_t(old_width) * height != int64_t(width) * old_height;
}
template<class Overlay, class Handle>
std::optional<float> cast_window_resized_width(Overlay* overlays, Handle overlay,
        int old_width, int old_height, int width, int height) {
    float current_width = 0;
    if (overlays->GetOverlayWidthInMeters(overlay, &current_width) ||
        !std::isfinite(current_width) || current_width <= 0) return std::nullopt;
    float next_width = current_width * float(old_height) / old_width * float(width) / height;
    if (!std::isfinite(next_width) || next_width <= 0) return std::nullopt;
    return next_width;
}
