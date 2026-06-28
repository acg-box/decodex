#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from semantic_drift_git import git_diff
from semantic_drift_parse import parse_diff
from semantic_drift_scan import stale_phrase_hits


def build_packet(repo_root: Path, diff_text: str) -> dict[str, Any]:
    parsed = parse_diff(diff_text)
    stale_hits = stale_phrase_hits(repo_root, parsed["removed_terms"])
    review_required = bool(
        parsed["added_claims"]
        or parsed["removed_terms"]
        or stale_hits
        or (parsed["changed_docs"] and parsed["changed_executable"])
    )
    return {
        **parsed,
        "stale_phrase_hits": stale_hits,
        "review_required": review_required,
        "agent_verdict_required": review_required,
        "limitations": [
            "This helper collects candidate drift evidence only.",
            "The agent must still compare claims against evidence and return pass, fail, or needs-human.",
        ],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Collect a Decodex semantic drift audit packet from git diff.")
    parser.add_argument("--repo", default=".", help="Repository root. Defaults to the current directory.")
    parser.add_argument("--rev", help="Optional diff base or range passed to git diff.")
    parser.add_argument("--json", action="store_true", help="Print compact JSON instead of pretty JSON.")
    args = parser.parse_args()
    repo_root = Path(args.repo).resolve()
    packet = build_packet(repo_root, git_diff(repo_root, args.rev))
    if args.json:
        print(json.dumps(packet, sort_keys=True, separators=(",", ":")))
    else:
        print(json.dumps(packet, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
