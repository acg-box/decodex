#!/bin/bash

set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$repository_root"

cargo +stable build -p decodexd
DECODEX_TEST_DAEMON="$repository_root/target/debug/decodexd" \
	cargo +stable test -p decodex-gpui 'bundled_daemon::tests::process_' -- --ignored
