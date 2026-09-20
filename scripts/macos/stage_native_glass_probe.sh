#!/usr/bin/env bash
# Build the isolated glass experiment. Never launch or replace Decodex.app.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
APP="$ROOT/target/native-glass-probe/Native Glass Probe.app"
# Match the local signing default in stage_decodex_app.sh.
SIGN_IDENTITY=${DECODEX_APP_SIGN_IDENTITY:-4EBCADF6B4D513E45CE33EC6934C08DBB0F03D7F}
cd "$ROOT"
cargo +stable build -p decodex-gpui --release --bin decodex-native-glass-probe --features native-glass-probe
mkdir -p "$APP/Contents/MacOS"
cp target/release/decodex-native-glass-probe "$APP/Contents/MacOS/decodex-native-glass-probe"
python3 - "$APP" <<'PY'
import pathlib
import plistlib
import sys
app = pathlib.Path(sys.argv[1])
info = {
    'CFBundleDevelopmentRegion': 'en',
    'CFBundleName': 'Native Glass Probe',
    'CFBundleDisplayName': 'Native Glass Probe',
    'CFBundleIdentifier': 'box.acg.decodex.glass-probe',
    'CFBundleExecutable': 'decodex-native-glass-probe',
    'CFBundlePackageType': 'APPL',
    'CFBundleInfoDictionaryVersion': '6.0',
    'CFBundleShortVersionString': '0.1.0',
    'CFBundleVersion': '1',
    'LSMinimumSystemVersion': '26.0',
    'NSHighResolutionCapable': True,
    'NSPrincipalClass': 'NSApplication',
    'NSSupportsAutomaticTermination': False,
    'NSSupportsSuddenTermination': False,
}
(app / 'Contents/Info.plist').write_bytes(plistlib.dumps(info))
PY
codesign --force --options runtime --timestamp=none --sign "$SIGN_IDENTITY" "$APP"
codesign --verify --deep --strict "$APP"
printf '%s\n' "$APP"
