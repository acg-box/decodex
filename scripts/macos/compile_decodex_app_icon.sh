#!/bin/sh
# Compile the layered icon with the repository's Xcode toolchain.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
OUTPUT=${1:?Usage: compile_decodex_app_icon.sh OUTPUT_DIRECTORY}
DEVELOPER_DIR=${DEVELOPER_DIR:-/Applications/Xcode-beta.app/Contents/Developer}
export DEVELOPER_DIR
mkdir -p "$OUTPUT"
PARTIAL=$(mktemp "${TMPDIR:-/tmp}/decodex-icon-info.XXXXXX")
trap 'rm -f "$PARTIAL"' EXIT
MINIMUM=$(plutil -extract LSMinimumSystemVersion raw "$ROOT/apps/decodex-gpui/packaging/Info.plist")
xcrun actool "$ROOT/assets/app-icon/composer/AppIcon.icon" \
  --compile "$OUTPUT" --output-format human-readable-text --notices --warnings \
  --output-partial-info-plist "$PARTIAL" --app-icon AppIcon \
  --include-all-app-icons --minimum-deployment-target "$MINIMUM" --platform macosx
for key in CFBundleIconFile CFBundleIconName; do
  test "$(plutil -extract "$key" raw "$PARTIAL")" = \
    "$(plutil -extract "$key" raw "$ROOT/apps/decodex-gpui/packaging/Info.plist")"
done
test -s "$OUTPUT/Assets.car"
test -s "$OUTPUT/AppIcon.icns"
