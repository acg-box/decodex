#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "Decodex.app staging is macOS-only; skipping."
  exit 0
fi

stage_root="$(mktemp -d "${TMPDIR:-/tmp}/decodex-app-stage.XXXXXX")"
trap 'rm -rf "$stage_root"' EXIT

DECODEX_APP_STAGE_DIR="$stage_root" scripts/macos/stage_decodex_app.sh

app_path="$stage_root/Decodex.app"
contents="$app_path/Contents"
info="$contents/Info.plist"

test -d "$app_path"
test -x "$contents/MacOS/decodex-gpui"
test -x "$contents/Helpers/decodexd"
test -x "$contents/Frameworks/libDecodexMenuBar.dylib"
test -x "$contents/Frameworks/libdecodex_app_client_ffi.dylib"
test -f "$contents/Resources/AppIcon.icns"
test -f "$contents/Resources/StatusBarIcon.png"
test ! -e "$contents/Library/LoginItems"
test "$(find "$stage_root" -type d -name '*.app' | wc -l | tr -d ' ')" = 1
test "$(find "$contents/MacOS" -type f | wc -l | tr -d ' ')" = 1
test "$(find "$contents/Helpers" -type f | wc -l | tr -d ' ')" = 1
test "$(find "$contents/Frameworks" -type f | wc -l | tr -d ' ')" = 2

codesign --verify --deep --strict "$app_path"
codesign --verify --strict "$contents/Helpers/decodexd"
codesign --verify --strict "$contents/Frameworks/libDecodexMenuBar.dylib"
codesign --verify --strict "$contents/Frameworks/libdecodex_app_client_ffi.dylib"
app_signing="$(codesign -dvvv "$app_path" 2>&1)"
app_team="$(printf '%s\n' "$app_signing" | sed -n 's/^TeamIdentifier=//p')"
test -n "$app_team"
printf '%s\n' "$app_signing" | grep '^Authority=' >/dev/null
for nested_code in \
  "$contents/Helpers/decodexd" \
  "$contents/Frameworks/libDecodexMenuBar.dylib" \
  "$contents/Frameworks/libdecodex_app_client_ffi.dylib"
do
  nested_team="$(codesign -dvvv "$nested_code" 2>&1 | sed -n 's/^TeamIdentifier=//p')"
  test "$nested_team" = "$app_team"
done
plutil -lint "$info"
plutil -extract CFBundleName raw "$info" | grep -qx 'Decodex'
plutil -extract CFBundleDisplayName raw "$info" | grep -qx 'Decodex'
plutil -extract CFBundleIdentifier raw "$info" | grep -qx 'box.acg.decodex'
plutil -extract CFBundleExecutable raw "$info" | grep -qx 'decodex-gpui'
plutil -extract CFBundleIconFile raw "$info" | grep -qx 'AppIcon'
plutil -extract LSMinimumSystemVersion raw "$info" | grep -qx '27.0'
plutil -extract NSSupportsAutomaticTermination raw "$info" | grep -qx 'false'
plutil -extract NSSupportsSuddenTermination raw "$info" | grep -qx 'false'
python3 scripts/macos/verify_decodex_bundle_contracts.py \
  --daemon "$contents/Helpers/decodexd" \
  --native-client "$contents/Frameworks/libdecodex_app_client_ffi.dylib" \
  --menu-bar "$contents/Frameworks/libDecodexMenuBar.dylib"
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_menu_bar_create' >/dev/null
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_menu_bar_set_visible' >/dev/null
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_menu_bar_launch_at_login_status' >/dev/null
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_menu_bar_set_launch_at_login' >/dev/null
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_menu_bar_open_login_items_settings' >/dev/null
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_app_was_launched_as_login_item' >/dev/null
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_menu_bar_destroy' >/dev/null
nm -gj "$contents/Frameworks/libdecodex_app_client_ffi.dylib" | grep -Fx '_decodex_app_native_client_create' >/dev/null

mismatch_library="$stage_root/libmismatched_native_client.dylib"
xcrun clang -dynamiclib scripts/macos/fixtures/mismatched_native_client.c -o "$mismatch_library"
if python3 scripts/macos/verify_decodex_bundle_contracts.py \
  --daemon "$contents/Helpers/decodexd" \
  --native-client "$mismatch_library" \
  --menu-bar "$contents/Frameworks/libDecodexMenuBar.dylib"
then
  echo "The staged bundle contract check accepted a mismatched native client fixture." >&2
  exit 1
fi
if plutil -extract LSUIElement raw "$info" >/dev/null 2>&1; then
  echo "Decodex.app must remain a regular windowed application." >&2
  exit 1
fi
