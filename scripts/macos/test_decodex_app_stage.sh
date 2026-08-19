#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Decodex.app staging is macOS-only; skipping."
  exit 0
fi

./apps/decodex-app/script/build_and_run.sh stage

common_root="$(cd "$(git rev-parse --git-common-dir)/.." && pwd)"
worktree_root="$(git rev-parse --show-toplevel)"
stage_dir="${DECODEX_APP_STAGE_DIR:-$common_root/target/decodex-app}"
app_path="$stage_dir/Decodex.app"
compatibility_source="$worktree_root/apps/decodex-app/Sources/DecodexApp/DecodexNativeCompatibility.swift"

test -d "$app_path"
test -x "$app_path/Contents/MacOS/DecodexApp"
native_client="$app_path/Contents/Frameworks/libdecodex_app_client_ffi.dylib"
test -f "$native_client"
test ! -e "$app_path/Contents/Helpers"
test -f "$app_path/Contents/Info.plist"
test -f "$app_path/Contents/Resources/AppIcon.icns"
test -f "$app_path/Contents/Resources/StatusBarIcon.png"

codesign --verify --deep --strict "$app_path"
codesign --verify --strict "$native_client"

loader_dir="$(mktemp -d "${TMPDIR:-/tmp}/decodex-native-loader.XXXXXX")"
trap 'rm -rf "$loader_dir"' EXIT
cat >"$loader_dir/main.swift" <<'SWIFT'
import Darwin
import Foundation

guard CommandLine.arguments.count == 2 else {
    exit(1)
}
do {
    let handle = try DecodexNativeCompatibility.openLibrary(
        at: URL(fileURLWithPath: CommandLine.arguments[1])
    )
    dlclose(handle)
} catch {
    exit(1)
}
SWIFT
xcrun swiftc "$compatibility_source" "$loader_dir/main.swift" -o "$loader_dir/native-loader"
"$loader_dir/native-loader" "$native_client"
for symbol in \
  _decodex_app_native_client_abi_version \
  _decodex_app_native_client_artifact_cohort \
  _decodex_app_native_client_create \
  _decodex_app_native_client_request \
  _decodex_app_native_client_free \
  _decodex_app_native_client_destroy
do
  nm -gU "$native_client" | awk '{print $NF}' | grep -qx "$symbol"
done
codesign_details="$(codesign -dv --verbose=4 "$app_path" 2>&1)"
grep -q '^TeamIdentifier=' <<<"$codesign_details"
grep -q 'flags=.*runtime' <<<"$codesign_details"

plutil -extract CFBundleName raw "$app_path/Contents/Info.plist" | grep -qx 'Decodex'
plutil -extract CFBundleDisplayName raw "$app_path/Contents/Info.plist" | grep -qx 'Decodex'
plutil -extract CFBundleIconFile raw "$app_path/Contents/Info.plist" | grep -qx 'AppIcon'
plutil -extract CFBundleIdentifier raw "$app_path/Contents/Info.plist" | grep -qx 'space.decodex.app'
plutil -extract LSUIElement raw "$app_path/Contents/Info.plist" | grep -qx 'true'
