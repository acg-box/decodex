#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
exec "$repo_root/tools/lane-authority-inventory/run_locked_python.sh" \
	"$repo_root/tools/lane-authority-inventory/verify_contract.py" --phase P2
