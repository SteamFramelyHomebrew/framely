#!/usr/bin/env bash
set -euo pipefail
base=$(cd -- "$(dirname -- "$0")/.." && pwd)
cd "$base"
mkdir -p target/tests
for test in gamepad_mapping gamepad_steam entry_hold menu_geometry keyboard_state notification_geometry notification_badge ui_visibility plugin_navigation launcher_input launcher_gaze gaze_geometry launcher_fade launcher_pointer page_scroll paint_buffer render_timing launcher_priority; do
  "${CXX:-g++}" -std=c++17 -O2 -pthread -Inative -Inative/vendor/openvr "tests/$test.cpp" -ldl -o "target/tests/$test"
  "target/tests/$test"
done

python3 "$base/tests/test_ui_feedback.py"
