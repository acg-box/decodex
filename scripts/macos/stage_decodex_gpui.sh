#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
STAGE_ROOT=${DECODEX_GPUI_STAGE_DIR:-"$ROOT/target/decodex-gpui"}
APP="$STAGE_ROOT/Decodex.app"
CONTENTS="$APP/Contents"
MACOS="$CONTENTS/MacOS"
LOGIN_ITEMS="$CONTENTS/Library/LoginItems"
MENUBAR_APP="$LOGIN_ITEMS/DecodexMenuBar.app"
MENUBAR_STAGE_ROOT="$STAGE_ROOT/menubar"
MENUBAR_SOURCE_APP="$MENUBAR_STAGE_ROOT/Decodex.app"
SIGN_IDENTITY=${DECODEX_GPUI_SIGN_IDENTITY:--}
DEVELOPER_DIR=${DEVELOPER_DIR:-/Applications/Xcode-beta.app/Contents/Developer}
export DEVELOPER_DIR

cargo +stable build --locked --release --bin decodex-gpui
rm -rf "$APP"
mkdir -p "$MACOS" "$LOGIN_ITEMS"
cp "$ROOT/apps/decodex-gpui/packaging/Info.plist" "$CONTENTS/Info.plist"
cp "$ROOT/target/release/decodex-gpui" "$MACOS/decodex-gpui"
chmod 755 "$MACOS/decodex-gpui"

if [ "$SIGN_IDENTITY" = "-" ]; then
	DECODEX_APP_STAGE_DIR="$MENUBAR_STAGE_ROOT" \
	DECODEX_APP_BUNDLE_ID=box.acg.decodex.menubar \
	DECODEX_APP_DISPLAY_NAME="Decodex Menu Bar" \
		"$ROOT/apps/decodex-app/script/build_and_run.sh" stage
else
	DECODEX_APP_STAGE_DIR="$MENUBAR_STAGE_ROOT" \
	DECODEX_APP_BUNDLE_ID=box.acg.decodex.menubar \
	DECODEX_APP_DISPLAY_NAME="Decodex Menu Bar" \
	DECODEX_APP_SIGN_IDENTITY="$SIGN_IDENTITY" \
		"$ROOT/apps/decodex-app/script/build_and_run.sh" stage
fi
cp -R "$MENUBAR_SOURCE_APP" "$MENUBAR_APP"

codesign --force --options runtime --timestamp=none --sign "$SIGN_IDENTITY" "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"
plutil -lint "$CONTENTS/Info.plist"
test "$(plutil -extract CFBundleIdentifier raw "$CONTENTS/Info.plist")" = box.acg.decodex
test "$(plutil -extract CFBundleExecutable raw "$CONTENTS/Info.plist")" = decodex-gpui
test "$(plutil -extract CFBundleIdentifier raw "$MENUBAR_APP/Contents/Info.plist")" = box.acg.decodex.menubar
test "$(plutil -extract CFBundleExecutable raw "$MENUBAR_APP/Contents/Info.plist")" = DecodexApp
test -f "$MENUBAR_APP/Contents/Frameworks/libdecodex_app_client_ffi.dylib"

printf '%s\n' "$APP"
