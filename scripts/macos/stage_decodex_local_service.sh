#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
STAGE_ROOT=${DECODEX_LOCAL_SERVICE_STAGE_DIR:-"$ROOT/target/decodex-local-service"}
SIGN_IDENTITY=${DECODEX_LOCAL_SERVICE_SIGN_IDENTITY:?set DECODEX_LOCAL_SERVICE_SIGN_IDENTITY to a Developer ID or Apple Development identity}
PROFILE=release

if [[ $SIGN_IDENTITY == "-" ]]; then
	echo "error: an ad-hoc signature has no TeamIdentifier and cannot be installed" >&2
	exit 2
fi

cargo +stable build --locked --profile "$PROFILE" \
	-p decodex-cli \
	-p decodex-database-transfer

install -d -m 700 "$STAGE_ROOT"
install -m 755 "$ROOT/target/$PROFILE/decodex" "$STAGE_ROOT/decodex"
install -m 755 \
	"$ROOT/target/$PROFILE/decodex-database-transfer" \
	"$STAGE_ROOT/decodex-database-transfer"

codesign --force --options runtime --timestamp=none \
	--identifier box.acg.decodex.cli \
	--sign "$SIGN_IDENTITY" "$STAGE_ROOT/decodex"
codesign --force --options runtime --timestamp=none \
	--identifier box.acg.decodex.database-transfer \
	--sign "$SIGN_IDENTITY" "$STAGE_ROOT/decodex-database-transfer"

for executable in decodex decodex-database-transfer; do
	codesign --verify --strict --verbose=2 "$STAGE_ROOT/$executable"
done
test "$(find "$STAGE_ROOT" -maxdepth 1 -type f | wc -l | tr -d ' ')" = 2
"$STAGE_ROOT/decodex" --output json build-info | python3 -c '
import json, re, sys
document = json.load(sys.stdin)
assert document.get("schema") == "decodex/build-info/1"
assert isinstance(document.get("version"), str) and document["version"]
assert isinstance(document.get("commit"), str) and re.fullmatch(r"[0-9a-f]{40}", document["commit"])
assert isinstance(document.get("dirty"), bool)
'

echo "$STAGE_ROOT"
