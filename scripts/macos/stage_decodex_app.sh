#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
STAGE_ROOT=${DECODEX_APP_STAGE_DIR:-"$ROOT/target/decodex-app"}
APP="$STAGE_ROOT/Decodex.app"
CONTENTS="$APP/Contents"
MACOS="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"
FRAMEWORKS="$CONTENTS/Frameworks"
HELPERS="$CONTENTS/Helpers"
MENU_BAR_PACKAGE="$ROOT/apps/decodex-gpui/menubar"
MENU_BAR_LIBRARY="libDecodexMenuBar.dylib"
NATIVE_CLIENT_LIBRARY="libdecodex_app_client_ffi.dylib"
SIGN_IDENTITY=${DECODEX_APP_SIGN_IDENTITY:?set DECODEX_APP_SIGN_IDENTITY to a Developer ID or Apple Development identity}
DEVELOPER_DIR=${DEVELOPER_DIR:-/Applications/Xcode-beta.app/Contents/Developer}
export DEVELOPER_DIR

if [ "$SIGN_IDENTITY" = "-" ]; then
	echo "Decodex.app requires a stable Apple codesigning identity; ad-hoc signing is unsupported." >&2
	exit 2
fi

cargo +stable build --locked --release --bin decodex-gpui
cargo +stable build --locked --release --bin decodexd
cargo +stable build --locked --release -p decodex-app-client-ffi --lib
SWIFT_BIN=$(swift build --package-path "$MENU_BAR_PACKAGE" -c release --product DecodexMenuBar --show-bin-path)
swift build --package-path "$MENU_BAR_PACKAGE" -c release --product DecodexMenuBar

case "$APP" in
	/Decodex.app) exit 2 ;;
	*/Decodex.app) ;;
	*) exit 2 ;;
esac
rm -rf -- "$APP"
mkdir -p "$MACOS" "$RESOURCES" "$FRAMEWORKS" "$HELPERS"
cp "$ROOT/apps/decodex-gpui/packaging/Info.plist" "$CONTENTS/Info.plist"
cp "$ROOT/target/release/decodex-gpui" "$MACOS/decodex-gpui"
cp "$ROOT/target/release/decodexd" "$HELPERS/decodexd"
cp "$ROOT/target/release/$NATIVE_CLIENT_LIBRARY" "$FRAMEWORKS/$NATIVE_CLIENT_LIBRARY"
cp "$SWIFT_BIN/$MENU_BAR_LIBRARY" "$FRAMEWORKS/$MENU_BAR_LIBRARY"
cp "$ROOT/assets/app-icon/generated/app-icon.icns" "$RESOURCES/AppIcon.icns"
cp "$ROOT/assets/tray-icon/generated/tray-icon-template.png" "$RESOURCES/StatusBarIcon.png"
chmod 755 \
	"$MACOS/decodex-gpui" \
	"$HELPERS/decodexd" \
	"$FRAMEWORKS/$NATIVE_CLIENT_LIBRARY" \
	"$FRAMEWORKS/$MENU_BAR_LIBRARY"

codesign --force --options runtime --timestamp=none --sign "$SIGN_IDENTITY" \
	--identifier box.acg.decodex.daemon "$HELPERS/decodexd"
codesign --force --options runtime --timestamp=none --sign "$SIGN_IDENTITY" \
	--identifier box.acg.decodex.native-client "$FRAMEWORKS/$NATIVE_CLIENT_LIBRARY"
codesign --force --options runtime --timestamp=none --sign "$SIGN_IDENTITY" \
	--identifier box.acg.decodex.menu-bar "$FRAMEWORKS/$MENU_BAR_LIBRARY"
codesign --force --options runtime --timestamp=none --sign "$SIGN_IDENTITY" "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"
plutil -lint "$CONTENTS/Info.plist"
test "$(plutil -extract CFBundleIdentifier raw "$CONTENTS/Info.plist")" = box.acg.decodex
test "$(plutil -extract CFBundleExecutable raw "$CONTENTS/Info.plist")" = decodex-gpui
test "$(plutil -extract CFBundleName raw "$CONTENTS/Info.plist")" = Decodex
test "$(plutil -extract CFBundleDisplayName raw "$CONTENTS/Info.plist")" = Decodex
test -f "$RESOURCES/AppIcon.icns"
test -f "$RESOURCES/StatusBarIcon.png"
test ! -e "$CONTENTS/Library/LoginItems"
test -x "$HELPERS/decodexd"
test -x "$FRAMEWORKS/$NATIVE_CLIENT_LIBRARY"
test -x "$FRAMEWORKS/$MENU_BAR_LIBRARY"

printf '%s\n' "$APP"
