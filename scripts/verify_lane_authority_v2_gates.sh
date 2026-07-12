#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

checkpoint=${1:-}
if [[ ! $checkpoint =~ ^C[0-7]$ ]]; then
	printf 'usage: %s C0|C1|C2|C3|C4|C5|C6|C7\n' "$0" >&2
	exit 2
fi

manifest=apps/decodex/src/orchestrator/tests/fixtures/lane_authority_v2/scenario_manifest.json
test_names=()
while IFS= read -r test_name; do
	test_names+=("$test_name")
done < <(python3 - "$manifest" "$checkpoint" <<'PY'
import json
import sys

manifest_path, checkpoint = sys.argv[1:]
with open(manifest_path, encoding="utf-8") as handle:
    manifest = json.load(handle)

tests = [
    scenario["test_name"]
    for scenario in manifest["scenarios"]
    if scenario["checkpoint"] == checkpoint
]
if not tests:
    raise SystemExit(f"scenario manifest has no tests for {checkpoint}")
if len(tests) != len(set(tests)):
    raise SystemExit(f"scenario manifest has duplicate test names for {checkpoint}")
print("\n".join(tests))
PY
)

for test_name in "${test_names[@]}"; do
	match_count=$(rg -l --glob '*.rs' "fn ${test_name}\\(" apps/decodex/src | wc -l | tr -d ' ')
	if [[ $match_count != 1 ]]; then
		printf 'scenario test %s must have exactly one Rust definition; found %s\n' \
			"$test_name" "$match_count" >&2
		exit 1
	fi
	cargo test -p decodex "$test_name" --all-features --quiet
done

printf 'lane-authority-v2 %s: verified %s scenarios\n' "$checkpoint" "${#test_names[@]}"
