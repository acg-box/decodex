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
test -x "$contents/Helpers/decodex"
test ! -e "$contents/Helpers/decodexd"
test -x "$contents/Frameworks/libDecodexMenuBar.dylib"
test -s "$contents/Resources/ThirdPartyNotices/libwebrtc.txt"
test -x "$contents/Frameworks/libdecodex_app_client_ffi.dylib"
test -f "$contents/Resources/AppIcon.icns"
test -s "$contents/Resources/Assets.car"
test -f "$contents/Resources/StatusBarIcon.png"
test -s "$contents/Resources/StatusBarIcon-22.png"
test -s "$contents/Resources/StatusBarIcon-22@2x.png"
test ! -e "$contents/Library/LoginItems"
test ! -e "$contents/Resources/CodexRuntime"
test ! -e "$contents/Resources/CodexSchema"
test -d "$contents/Frameworks/Sparkle.framework"
python3 - "$contents" <<'PY'
import json
import sys
from pathlib import Path

contents = Path(sys.argv[1])
lock = json.loads(Path("codex-runtime.lock.json").read_text())
assert lock["source"]["url"].endswith(lock["upstreamCommit"])
assert json.loads((contents / "Resources/codex-runtime.lock.json").read_text()) == lock
PY
if DECODEX_BUILD_CHANNEL=release DECODEX_RUNTIME_LOCK_PATH="$stage_root/missing-release.json" \
    DECODEX_APP_STAGE_DIR="$stage_root" scripts/macos/stage_decodex_app.sh >"$stage_root/missing-release.log" 2>&1; then
  echo "Release packaging accepted a missing official release pin." >&2
  exit 1
fi
test -x "$contents/MacOS/decodex-gpui"
test "$(plutil -extract SUFeedURL raw "$info")" = 'https://github.com/acg-box/decodex/releases/latest/download/appcast.xml'
codesign --verify --deep --strict "$contents/Frameworks/Sparkle.framework"
test "$(find "$contents/MacOS" -type f | wc -l | tr -d ' ')" = 1
test "$(find "$contents/Helpers" -maxdepth 1 -type f | wc -l | tr -d ' ')" = 1
test "$(find "$contents/Frameworks" -maxdepth 1 -type f | wc -l | tr -d ' ')" = 2

test -n "$(plutil -extract NSMicrophoneUsageDescription raw "$info")"
codesign -d --entitlements :- "$app_path" 2>/dev/null | python3 -c 'import plistlib, sys; assert plistlib.loads(sys.stdin.buffer.read())["com.apple.security.device.audio-input"] is True'
codesign --verify --deep --strict "$app_path"
codesign --verify --strict "$contents/Helpers/decodex"
codesign --verify --strict "$contents/Frameworks/libDecodexMenuBar.dylib"
codesign --verify --strict "$contents/Frameworks/libdecodex_app_client_ffi.dylib"
app_signing="$(codesign -dvvv "$app_path" 2>&1)"
app_team="$(printf '%s\n' "$app_signing" | sed -n 's/^TeamIdentifier=//p')"
test -n "$app_team"
printf '%s\n' "$app_signing" | grep '^Authority=' >/dev/null
for nested_code in \
  "$contents/Helpers/decodex" \
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
plutil -extract CFBundleIconName raw "$info" | grep -qx 'AppIcon'
plutil -extract DecodexIconVariant raw "$info" | grep -qx "$(cat assets/app-icon/default-variant)"
plutil -extract DecodexIconBuildKind raw "$info" | grep -qx 'native-liquid-glass'
plutil -extract LSMinimumSystemVersion raw "$info" | grep -qx '27.0'
plutil -extract NSSupportsAutomaticTermination raw "$info" | grep -qx 'false'
plutil -extract NSSupportsSuddenTermination raw "$info" | grep -qx 'false'
test "$(plutil -extract CFBundleShortVersionString raw "$info")" = \
  "$("$contents/Helpers/decodex" --output json build-info | python3 -c 'import json,sys; print(json.load(sys.stdin)["version"])')"
test "$(plutil -extract DecodexBuildCommit raw "$info")" = \
  "$("$contents/Helpers/decodex" --output json build-info | python3 -c 'import json,sys; print(json.load(sys.stdin)["commit"])')"
test "$(plutil -extract DecodexBuildDirty raw "$info")" = \
  "$("$contents/Helpers/decodex" --output json build-info | python3 -c 'import json,sys; print(str(json.load(sys.stdin)["dirty"]).lower())')"
python3 scripts/macos/verify_decodex_bundle_contracts.py \
  --service "$contents/Helpers/decodex" \
  --app-info "$info" \
  --native-client "$contents/Frameworks/libdecodex_app_client_ffi.dylib" \
  --menu-bar "$contents/Frameworks/libDecodexMenuBar.dylib"
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_menu_bar_create' >/dev/null
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_menu_bar_set_visible' >/dev/null
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_menu_bar_launch_at_login_status' >/dev/null
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_menu_bar_set_launch_at_login' >/dev/null
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_menu_bar_open_login_items_settings' >/dev/null
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_app_was_launched_as_login_item' >/dev/null
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_menu_bar_destroy' >/dev/null
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_update_create' >/dev/null
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_update_check' >/dev/null
nm -gj "$contents/Frameworks/libDecodexMenuBar.dylib" | grep -Fx '_decodex_update_destroy' >/dev/null
nm -gj "$contents/Frameworks/libdecodex_app_client_ffi.dylib" | grep -Fx '_decodex_app_native_client_create' >/dev/null
nm -gj "$contents/Frameworks/libdecodex_app_client_ffi.dylib" | grep -Fx '_decodex_reset_card_journal_v2' >/dev/null

mismatch_library="$stage_root/libmismatched_native_client.dylib"
rustc +stable --edition=2024 --crate-type cdylib \
  scripts/macos/fixtures/mismatched_native_client.rs -o "$mismatch_library"
if python3 scripts/macos/verify_decodex_bundle_contracts.py \
  --service "$contents/Helpers/decodex" \
  --app-info "$info" \
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
