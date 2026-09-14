#!/bin/sh
# Compile the layered icon with the repository's Xcode toolchain.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
OUTPUT=${1:?Usage: compile_decodex_app_icon.sh OUTPUT_DIRECTORY}
VARIANT=$(cat "$ROOT/assets/app-icon/default-variant")
SOURCE=${2:-"$ROOT/assets/app-icon/liquid-glass/$VARIANT/AppIcon.icon"}
test -f "$SOURCE/icon.json"
DEVELOPER_DIR=${DEVELOPER_DIR:-/Applications/Xcode-beta.app/Contents/Developer}
export DEVELOPER_DIR
mkdir -p "$OUTPUT"
PARTIAL=$(mktemp "${TMPDIR:-/tmp}/decodex-icon-info.XXXXXX")
trap 'rm -f "$PARTIAL"' EXIT
MINIMUM=$(plutil -extract LSMinimumSystemVersion raw "$ROOT/apps/decodex-gpui/packaging/Info.plist")
xcrun actool "$SOURCE" \
  --compile "$OUTPUT" --output-format human-readable-text --notices --warnings \
  --output-partial-info-plist "$PARTIAL" --app-icon AppIcon \
  --include-all-app-icons --minimum-deployment-target "$MINIMUM" --platform macosx
for key in CFBundleIconFile CFBundleIconName; do
  test "$(plutil -extract "$key" raw "$PARTIAL")" = \
    "$(plutil -extract "$key" raw "$ROOT/apps/decodex-gpui/packaging/Info.plist")"
done
test -s "$OUTPUT/Assets.car"
test -s "$OUTPUT/AppIcon.icns"

assetutil --info "$OUTPUT/Assets.car" | python3 -c '
import json,sys
items=json.load(sys.stdin)
required={"NSAppearanceNameAqua","NSAppearanceNameDarkAqua","ISAppearanceTintable"}
actual={item.get("Appearance") for item in items if item.get("AssetType")=="IconImageStack"}
if not required <= actual or not any(item.get("AssetType")=="Vector" for item in items):
    raise SystemExit("Native vector icon stacks for Default, Dark, and Mono are required")
'
