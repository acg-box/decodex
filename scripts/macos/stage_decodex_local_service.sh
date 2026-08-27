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
	-p decodexd \
	-p decodex-cli \
	-p decodex-database-transfer

install -d -m 700 "$STAGE_ROOT"
install -m 755 "$ROOT/target/$PROFILE/decodexd" "$STAGE_ROOT/decodexd"
install -m 755 "$ROOT/target/$PROFILE/decodex" "$STAGE_ROOT/decodex"
install -m 755 \
	"$ROOT/target/$PROFILE/decodex-database-transfer" \
	"$STAGE_ROOT/decodex-database-transfer"

codesign --force --options runtime --timestamp=none \
	--identifier box.acg.decodex.daemon \
	--sign "$SIGN_IDENTITY" "$STAGE_ROOT/decodexd"
codesign --force --options runtime --timestamp=none \
	--identifier box.acg.decodex.cli \
	--sign "$SIGN_IDENTITY" "$STAGE_ROOT/decodex"
codesign --force --options runtime --timestamp=none \
	--identifier box.acg.decodex.database-transfer \
	--sign "$SIGN_IDENTITY" "$STAGE_ROOT/decodex-database-transfer"

for executable in decodexd decodex decodex-database-transfer; do
	codesign --verify --strict --verbose=2 "$STAGE_ROOT/$executable"
done

echo "$STAGE_ROOT"
