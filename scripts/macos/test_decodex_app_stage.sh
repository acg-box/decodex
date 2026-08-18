#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Decodex.app staging is macOS-only; skipping."
  exit 0
fi

./apps/decodex-app/script/build_and_run.sh stage

common_root="$(cd "$(git rev-parse --git-common-dir)/.." && pwd)"
stage_dir="${DECODEX_APP_STAGE_DIR:-$common_root/target/decodex-app}"
app_path="$stage_dir/Decodex.app"

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

guard CommandLine.arguments.count == 2,
      let handle = dlopen(CommandLine.arguments[1], RTLD_NOW | RTLD_LOCAL),
      let symbol = dlsym(handle, "decodex_app_native_client_abi_version")
else {
    exit(1)
}
defer { dlclose(handle) }
typealias ABIVersion = @convention(c) () -> UInt32
let abiVersion = unsafeBitCast(symbol, to: ABIVersion.self)
guard abiVersion() == 1 else {
    exit(1)
}
SWIFT
xcrun swiftc "$loader_dir/main.swift" -o "$loader_dir/native-loader"
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
