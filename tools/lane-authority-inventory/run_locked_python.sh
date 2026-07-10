#!/usr/bin/env bash
set -euo pipefail

if [[ $# -eq 0 ]]; then
	printf 'usage: %s PYTHON_ARGS...\n' "$0" >&2
	exit 64
fi

repo_root="$(git rev-parse --show-toplevel)"
lock="$repo_root/tools/lane-authority-inventory/requirements.lock"
tmp_root="$(mktemp -d "${TMPDIR:-/tmp}/decodex-c1i-python.XXXXXX")"
trap 'rm -rf "$tmp_root"' EXIT

export UV_NO_PROGRESS=1
uv venv --python "$(command -v python3)" "$tmp_root/venv" >/dev/null
uv pip sync \
	--python "$tmp_root/venv/bin/python" \
	--require-hashes \
	"$lock" >/dev/null
"$tmp_root/venv/bin/python" "$@"
