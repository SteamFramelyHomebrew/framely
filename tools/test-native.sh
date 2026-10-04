#!/usr/bin/env bash
set -euo pipefail
base=$(cd -- "$(dirname -- "$0")/.." && pwd)
cd "$base"
mkdir -p target/tests
for test in menu_geometry keyboard_state notification_geometry ui_visibility; do
  "${CXX:-g++}" -std=c++17 -O2 -Inative -Inative/vendor/openvr "tests/$test.cpp" -o "target/tests/$test"
  "target/tests/$test"
done
