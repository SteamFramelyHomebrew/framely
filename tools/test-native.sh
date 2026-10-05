#!/usr/bin/env bash
set -euo pipefail
base=$(cd -- "$(dirname -- "$0")/.." && pwd)
cd "$base"
mkdir -p target/tests
for test in menu_geometry keyboard_state notification_geometry notification_badge ui_visibility plugin_navigation launcher_input launcher_pointer page_scroll paint_buffer render_timing; do
  "${CXX:-g++}" -std=c++17 -O2 -Inative -Inative/vendor/openvr "tests/$test.cpp" -o "target/tests/$test"
  "target/tests/$test"
done
