#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
python3 "$repo_root/scripts/lane_authority_v2_baseline.py" --self-test
exec python3 "$repo_root/scripts/lane_authority_v2_baseline.py"
