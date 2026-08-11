#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
STAGE_ROOT=${DECODEX_GPUI_STAGE_DIR:-"$ROOT/target/decodex-gpui"}
APP="$STAGE_ROOT/Decodex.app"
CONTENTS="$APP/Contents"
MACOS="$CONTENTS/MacOS"
SIGN_IDENTITY=${DECODEX_GPUI_SIGN_IDENTITY:--}
DEVELOPER_DIR=${DEVELOPER_DIR:-/Applications/Xcode-beta.app/Contents/Developer}
export DEVELOPER_DIR

cargo +stable build --locked --release --bin decodex-gpui
rm -rf "$APP"
mkdir -p "$MACOS"
cp "$ROOT/apps/decodex-gpui/packaging/Info.plist" "$CONTENTS/Info.plist"
cp "$ROOT/target/release/decodex-gpui" "$MACOS/decodex-gpui"
chmod 755 "$MACOS/decodex-gpui"

codesign --force --options runtime --timestamp=none --sign "$SIGN_IDENTITY" "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"
plutil -lint "$CONTENTS/Info.plist"
test "$(plutil -extract CFBundleIdentifier raw "$CONTENTS/Info.plist")" = box.acg.decodex
test "$(plutil -extract CFBundleExecutable raw "$CONTENTS/Info.plist")" = decodex-gpui

printf '%s\n' "$APP"
