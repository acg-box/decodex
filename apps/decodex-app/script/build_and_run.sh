#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-run}"
PRODUCT_NAME="Decodex"
EXECUTABLE_NAME="DecodexApp"
NATIVE_CLIENT_NAME="libdecodex_app_client_ffi.dylib"
BUNDLE_ID="${DECODEX_APP_BUNDLE_ID:-space.decodex.app}"
BUNDLE_DISPLAY_NAME="${DECODEX_APP_DISPLAY_NAME:-Decodex}"
MIN_SYSTEM_VERSION="27.0"
DEFAULT_SIGN_IDENTITY="x@acg.box"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKTREE_ROOT="$(git -C "$ROOT_DIR" rev-parse --show-toplevel)"
GIT_COMMON_DIR="$(git -C "$ROOT_DIR" rev-parse --path-format=absolute --git-common-dir)"
COMMON_ROOT="$(cd "$GIT_COMMON_DIR/.." && pwd)"
STAGE_DIR="${DECODEX_APP_STAGE_DIR:-$COMMON_ROOT/target/decodex-app}"
APP_BUNDLE="$STAGE_DIR/$PRODUCT_NAME.app"
APP_CONTENTS="$APP_BUNDLE/Contents"
APP_MACOS="$APP_CONTENTS/MacOS"
APP_FRAMEWORKS="$APP_CONTENTS/Frameworks"
APP_RESOURCES="$APP_CONTENTS/Resources"
APP_BINARY="$APP_MACOS/$EXECUTABLE_NAME"
APP_NATIVE_CLIENT="$APP_FRAMEWORKS/$NATIVE_CLIENT_NAME"
INFO_PLIST="$APP_CONTENTS/Info.plist"
APP_ICON_SOURCE="$WORKTREE_ROOT/assets/app-icon/generated/app-icon.icns"
APP_ICON_NAME="AppIcon.icns"
STATUS_ICON_SOURCE="$WORKTREE_ROOT/assets/tray-icon/generated/tray-icon-template.png"
STATUS_ICON_NAME="StatusBarIcon.png"
SWIFT_BUILD_FLAGS=(-c release)
RUST_BUILD_FLAGS=(--release)
RUST_TARGET_DIR=""
BUILD_ROOT=""
BUILD_BINARY=""
NATIVE_CLIENT_BINARY=""
RESOLVED_SIGN_IDENTITY=""
RUST_PROFILE="release"

if [[ "${DECODEX_APP_CARGO_LOCKED:-0}" == "1" ]]; then
	RUST_BUILD_FLAGS+=(--locked)
fi

APP_VERSION="${DECODEX_APP_VERSION:-}"
if [[ -z "$APP_VERSION" ]]; then
	APP_VERSION="$(
		sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\(.*\)"/\1/p' \
			"$WORKTREE_ROOT/Cargo.toml" | head -n 1
	)"
fi
APP_VERSION="${APP_VERSION:-0.2.0}"

developer_dir_has_macos_swiftui_macros() {
	local developer_dir="$1"

	[[ -f "$developer_dir/Platforms/MacOSX.platform/Developer/usr/lib/swift/host/plugins/libSwiftUIMacros.dylib" ]]
}

ensure_macos_swiftui_macro_toolchain() {
	local active_developer_dir xcode_beta_developer_dir

	active_developer_dir="${DEVELOPER_DIR:-}"
	if [[ -z "$active_developer_dir" ]]; then
		active_developer_dir="$(xcode-select -p 2>/dev/null || true)"
	fi
	if [[ -n "$active_developer_dir" ]] && developer_dir_has_macos_swiftui_macros "$active_developer_dir"; then
		return 0
	fi

	xcode_beta_developer_dir="/Applications/Xcode-beta.app/Contents/Developer"
	if developer_dir_has_macos_swiftui_macros "$xcode_beta_developer_dir"; then
		export DEVELOPER_DIR="$xcode_beta_developer_dir"
		return 0
	fi

	echo "error: the active developer directory is missing macOS SwiftUI macro support." >&2
	echo "error: install Xcode beta at /Applications/Xcode-beta.app or set DEVELOPER_DIR to a full Xcode with libSwiftUIMacros.dylib." >&2
	exit 1
}

terminate_running_app() {
	pkill -x "$EXECUTABLE_NAME" >/dev/null 2>&1 || true
}

write_info_plist() {
	cat >"$INFO_PLIST" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>$EXECUTABLE_NAME</string>
  <key>CFBundleIdentifier</key>
  <string>$BUNDLE_ID</string>
  <key>CFBundleName</key>
  <string>$PRODUCT_NAME</string>
  <key>CFBundleDisplayName</key>
  <string>$BUNDLE_DISPLAY_NAME</string>
  <key>CFBundleIconFile</key>
  <string>${APP_ICON_NAME%.icns}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$APP_VERSION</string>
  <key>CFBundleVersion</key>
  <string>$APP_VERSION</string>
  <key>LSMinimumSystemVersion</key>
  <string>$MIN_SYSTEM_VERSION</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
</dict>
</plist>
PLIST
}

resolve_signing_identity() {
	local requested_identity identity_list identity

	requested_identity="${DECODEX_APP_SIGN_IDENTITY:-$DEFAULT_SIGN_IDENTITY}"
	identity_list="$(security find-identity -v -p codesigning 2>/dev/null || true)"
	if [[ -n "$requested_identity" ]]; then
		while IFS= read -r line; do
			identity="${line#*\"}"
			identity="${identity%%\"*}"
			if [[ -n "$identity" && "$identity" == *"$requested_identity"* ]]; then
				RESOLVED_SIGN_IDENTITY="$identity"
				return 0
			fi
		done <<<"$identity_list"
	fi

	while IFS= read -r line; do
		identity="${line#*\"}"
		identity="${identity%%\"*}"
		if [[ -n "$identity" && "$identity" == Apple\ Development:* ]]; then
			RESOLVED_SIGN_IDENTITY="$identity"
			return 0
		fi
	done <<<"$identity_list"

	return 1
}

sign_staged_app_bundle() {
	local requested_identity entitlements_file

	requested_identity="${DECODEX_APP_SIGN_IDENTITY:-$DEFAULT_SIGN_IDENTITY}"
	if ! resolve_signing_identity; then
		echo "error: no valid macOS codesigning identity matching the configured signing selector was found." >&2
		echo "error: import the real signing certificate or set DECODEX_APP_SIGN_IDENTITY to a valid identity." >&2
		echo "error: Decodex.app staging requires a stable codesigning identity." >&2
		exit 1
	fi

	codesign \
		--force \
		--options runtime \
		--sign "$RESOLVED_SIGN_IDENTITY" \
		"$APP_NATIVE_CLIENT"

	entitlements_file="$BUILD_ROOT/$EXECUTABLE_NAME-entitlement.plist"
	if [[ -f "$entitlements_file" ]]; then
		codesign \
			--force \
			--deep \
			--options runtime \
			--sign "$RESOLVED_SIGN_IDENTITY" \
			--entitlements "$entitlements_file" \
			"$APP_BUNDLE"
	else
		codesign \
			--force \
			--deep \
			--options runtime \
			--sign "$RESOLVED_SIGN_IDENTITY" \
			"$APP_BUNDLE"
	fi
}

stage_app_bundle() {
	ensure_macos_swiftui_macro_toolchain
	BUILD_ROOT="$(swift build --package-path "$ROOT_DIR" "${SWIFT_BUILD_FLAGS[@]}" --show-bin-path)"
	BUILD_BINARY="$BUILD_ROOT/$EXECUTABLE_NAME"
	RUST_TARGET_DIR="$WORKTREE_ROOT/target/decodex-app-native-client"

	swift build --package-path "$ROOT_DIR" "${SWIFT_BUILD_FLAGS[@]}" --product "$EXECUTABLE_NAME"
	CARGO_TARGET_DIR="$RUST_TARGET_DIR" cargo build \
		-p decodex-app-client-ffi \
		--lib \
		"${RUST_BUILD_FLAGS[@]}"

	NATIVE_CLIENT_BINARY="$RUST_TARGET_DIR/$RUST_PROFILE/$NATIVE_CLIENT_NAME"

	rm -rf "$APP_BUNDLE"
	mkdir -p "$APP_MACOS" "$APP_FRAMEWORKS" "$APP_RESOURCES"
	cp "$BUILD_BINARY" "$APP_BINARY"
	cp "$NATIVE_CLIENT_BINARY" "$APP_NATIVE_CLIENT"
	chmod +x "$APP_BINARY"
	chmod +x "$APP_NATIVE_CLIENT"
	if [[ -f "$APP_ICON_SOURCE" ]]; then
		cp "$APP_ICON_SOURCE" "$APP_RESOURCES/$APP_ICON_NAME"
	fi
	if [[ -f "$STATUS_ICON_SOURCE" ]]; then
		cp "$STATUS_ICON_SOURCE" "$APP_RESOURCES/$STATUS_ICON_NAME"
	fi
	write_info_plist
	sign_staged_app_bundle
	codesign --verify --deep --strict "$APP_BUNDLE"
}

if [[ "$MODE" != "stage" && "$MODE" != "--stage" ]]; then
	terminate_running_app
fi

stage_app_bundle

open_app() {
	/usr/bin/open "$APP_BUNDLE"
}

case "$MODE" in
	stage|--stage)
		;;
	run)
		open_app
		;;
	--logs|logs)
		open_app
		/usr/bin/log stream --info --style compact --predicate "process == \"$EXECUTABLE_NAME\""
		;;
	--verify|verify)
		open_app
		sleep 1
		pgrep -x "$EXECUTABLE_NAME" >/dev/null
		codesign --verify --deep --strict "$APP_BUNDLE"
		;;
	*)
		echo "usage: $0 [run|stage|--logs|--verify]" >&2
		exit 2
		;;
esac
