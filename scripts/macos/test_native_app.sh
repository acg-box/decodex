#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
cargo +stable build --locked -p decodex-app-client-ffi --lib
build_root="$(cargo +stable metadata --locked --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
DECODEX_TEST_NATIVE_LIBRARY="$build_root/debug/libdecodex_app_client_ffi.dylib" \
  swift test --package-path apps/decodex-gpui/menubar "$@"
