#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

command -v sqlite3 >/dev/null
cargo build -p decodex --all-features --quiet

target_dir=${CARGO_TARGET_DIR:-$repo_root/target}
binary="$target_dir/debug/decodex"
test_home=$(mktemp -d)
trap 'rm -rf "$test_home"' EXIT

runtime_root="$test_home/.codex/decodex"
database="$runtime_root/generations/1/runtime.sqlite3"
mkdir -p "$runtime_root/runtime.sqlite3" "$(dirname "$database")"
: > "$database"
cat > "$runtime_root/runtime-format.toml" <<'EOF'
schema = "decodex/runtime-format/2"
generation = 1
database_relative_path = "generations/1/runtime.sqlite3"
EOF

HOME="$test_home" "$binary" project list >/dev/null

table_exists() {
	local table=$1
	[[ $(sqlite3 "$database" "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='$table';") == 1 ]]
}

table_exists lanes
! table_exists leases
! table_exists worktrees
[[ $(sqlite3 "$database" "SELECT value FROM schema_meta WHERE key='schema_version';") == 17 ]]

printf '%s\n' 'lane-authority-v2 fresh production schema: verified'
