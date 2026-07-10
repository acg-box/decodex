#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
	printf 'usage: %s C1I\n' "$0" >&2
	exit 64
fi

repo_root="$(git rev-parse --show-toplevel)"
case "$1" in
	C1I)
		set +e
		"$repo_root/tools/lane-authority-inventory/run_locked_python.sh" \
			"$repo_root/tools/lane-authority-inventory/verify_contract.py" --readiness C1I
		status=$?
		set -e
		case "$status" in
			1)
				exit 1
				;;
			2)
				exit 2
				;;
			0)
				printf 'C1I incomplete readiness unexpectedly succeeded\n' >&2
				exit 2
				;;
			*)
				printf 'C1I incomplete readiness returned unexpected status %s\n' "$status" >&2
				exit 70
				;;
		esac
		;;
	*)
		printf 'unsupported Lane Authority v2 incomplete gate: %s\n' "$1" >&2
		exit 64
		;;
esac
