#!/bin/bash

set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$repository_root"

cargo +stable build --bin decodex
DECODEX_TEST_SERVICE="$repository_root/target/debug/decodex" \
	cargo +stable test -p decodex-gpui 'bundled_daemon::tests::process_' -- --ignored
