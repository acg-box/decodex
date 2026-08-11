#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
MANIFEST="$ROOT/spikes/gpui/Cargo.toml"
STAGE_ROOT=${DECODEX_GPUI_SPIKE_STAGE_DIR:-"$ROOT/target/gpui-spike"}
APP="$STAGE_ROOT/Decodex GPUI Spike.app"
CONTENTS="$APP/Contents"
MACOS="$CONTENTS/MacOS"
SIGN_IDENTITY=${DECODEX_GPUI_SPIKE_SIGN_IDENTITY:--}

cargo +stable build --manifest-path "$MANIFEST" --release --bin decodex-gpui-spike
rm -rf "$APP"
mkdir -p "$MACOS"
cp "$ROOT/spikes/gpui/packaging/Info.plist" "$CONTENTS/Info.plist"
cp "$ROOT/spikes/gpui/target/release/decodex-gpui-spike" "$MACOS/decodex-gpui-spike"
chmod 755 "$MACOS/decodex-gpui-spike"

codesign --force --options runtime --timestamp=none --sign "$SIGN_IDENTITY" "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"
plutil -lint "$CONTENTS/Info.plist"

printf '%s\n' "$APP"
