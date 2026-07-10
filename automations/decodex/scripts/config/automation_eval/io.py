"""File and path helpers for automation evaluation."""

from __future__ import annotations

import os
import tomllib
from pathlib import Path
from typing import Any

from automation_checkout import primary_checkout_for_branch
from automation_eval.constants import REPO_ROOT


def default_codex_home() -> str:
    return os.environ.get("CODEX_HOME") or str(Path.home() / ".codex")


def load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        return tomllib.load(handle)


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def expected_cwd(value: str) -> str:
    runtime_root = primary_checkout_for_branch(REPO_ROOT)
    return value.replace("{repo_root}", str(runtime_root))


def active_automation_path(codex_home: Path, automation_id: str) -> Path:
    return codex_home / "automations" / automation_id / "automation.toml"
