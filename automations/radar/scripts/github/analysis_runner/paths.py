"""Path resolution for local Radar Codex analysis."""

from __future__ import annotations

from pathlib import Path


SCRIPT_HOME = Path(__file__).resolve().parents[1]


def repo_root_from(bundle_path: Path) -> Path:
    resolved = bundle_path.resolve()
    for root in resolved.parents:
        if (
            root / "automations" / "radar" / "skills" / "github-signal" / "SKILL.md"
        ).exists():
            return root
    raise SystemExit(f"Unable to resolve repo root from {bundle_path}")
