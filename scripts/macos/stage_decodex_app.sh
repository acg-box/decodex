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
DEFAULT_SIGN_IDENTITY="4EBCADF6B4D513E45CE33EC6934C08DBB0F03D7F"
DEFAULT_SIGN_TEAM_IDENTIFIER="4N949UKQ55"
SIGN_IDENTITY=${DECODEX_APP_SIGN_IDENTITY:-$DEFAULT_SIGN_IDENTITY}
SIGN_TEAM_IDENTIFIER=${DECODEX_APP_SIGN_TEAM_IDENTIFIER:-$DEFAULT_SIGN_TEAM_IDENTIFIER}
DEVELOPER_DIR=${DEVELOPER_DIR:-/Applications/Xcode-beta.app/Contents/Developer}
export DEVELOPER_DIR

if [ "$SIGN_IDENTITY" = "-" ]; then
	echo "Decodex.app requires a stable Apple codesigning identity; ad-hoc signing is unsupported." >&2
	exit 2
fi

verify_signing_team() {
	signed_path=$1
	details=$(codesign -dvvv "$signed_path" 2>&1)
	actual_team=$(printf '%s\n' "$details" | sed -n 's/^TeamIdentifier=//p')
	if [ "$actual_team" != "$SIGN_TEAM_IDENTIFIER" ]; then
		echo "Decodex signing team '$actual_team' does not match '$SIGN_TEAM_IDENTIFIER'." >&2
		exit 2
	fi
}

cargo +stable build --locked --release --bin decodex-gpui
cargo +stable build --locked --release --bin decodex
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
cp "$ROOT/target/release/decodex" "$HELPERS/decodex"
cp "$ROOT/target/release/$NATIVE_CLIENT_LIBRARY" "$FRAMEWORKS/$NATIVE_CLIENT_LIBRARY"
cp "$SWIFT_BIN/$MENU_BAR_LIBRARY" "$FRAMEWORKS/$MENU_BAR_LIBRARY"
cp "$ROOT/assets/app-icon/generated/app-icon.icns" "$RESOURCES/AppIcon.icns"
cp "$ROOT/assets/tray-icon/generated/tray-icon-template.png" "$RESOURCES/StatusBarIcon.png"
chmod 755 \
	"$MACOS/decodex-gpui" \
	"$HELPERS/decodex" \
	"$FRAMEWORKS/$NATIVE_CLIENT_LIBRARY" \
	"$FRAMEWORKS/$MENU_BAR_LIBRARY"

python3 "$ROOT/scripts/macos/verify_decodex_bundle_contracts.py" \
	--service "$HELPERS/decodex" \
	--app-info "$CONTENTS/Info.plist" \
	--native-client "$FRAMEWORKS/$NATIVE_CLIENT_LIBRARY" \
	--menu-bar "$FRAMEWORKS/$MENU_BAR_LIBRARY" \
	--stamp-app-info

codesign --force --options runtime --timestamp=none --sign "$SIGN_IDENTITY" \
	--identifier box.acg.decodex.cli "$HELPERS/decodex"
codesign --force --options runtime --timestamp=none --sign "$SIGN_IDENTITY" \
	--identifier box.acg.decodex.native-client "$FRAMEWORKS/$NATIVE_CLIENT_LIBRARY"
codesign --force --options runtime --timestamp=none --sign "$SIGN_IDENTITY" \
	--identifier box.acg.decodex.menu-bar "$FRAMEWORKS/$MENU_BAR_LIBRARY"
codesign --force --options runtime --timestamp=none --sign "$SIGN_IDENTITY" "$APP"
for signed_path in \
	"$HELPERS/decodex" \
	"$FRAMEWORKS/$NATIVE_CLIENT_LIBRARY" \
	"$FRAMEWORKS/$MENU_BAR_LIBRARY"
do
	codesign --verify --strict --verbose=2 "$signed_path"
	verify_signing_team "$signed_path"
done
codesign --verify --deep --strict --verbose=2 "$APP"
verify_signing_team "$APP"
plutil -lint "$CONTENTS/Info.plist"
test "$(plutil -extract CFBundleIdentifier raw "$CONTENTS/Info.plist")" = box.acg.decodex
test "$(plutil -extract CFBundleExecutable raw "$CONTENTS/Info.plist")" = decodex-gpui
test "$(plutil -extract CFBundleName raw "$CONTENTS/Info.plist")" = Decodex
test "$(plutil -extract CFBundleDisplayName raw "$CONTENTS/Info.plist")" = Decodex
test -f "$RESOURCES/AppIcon.icns"
test -f "$RESOURCES/StatusBarIcon.png"
test ! -e "$CONTENTS/Library/LoginItems"
test -x "$HELPERS/decodex"
test -x "$FRAMEWORKS/$NATIVE_CLIENT_LIBRARY"
test -x "$FRAMEWORKS/$MENU_BAR_LIBRARY"

printf '%s\n' "$APP"
