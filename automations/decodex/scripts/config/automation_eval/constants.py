"""Shared automation evaluation constants."""

from __future__ import annotations

from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[5]
MANIFEST_PATH = REPO_ROOT / "automations/upstream/automations.toml"
VALID_SOURCE_ROOTS = {
    "automations/decodex",
    "automations/upstream",
}
REQUIRED_FORBIDDEN_PROMPT_FRAGMENTS = [
    "/Users/",
    "/home/",
    "Documents/automations",
    ".github/workflows",
    "site/src/content",
    ".agent/decodex",
    "~/.codex/decodex",
    ".codex/decodex",
    "accounts.jsonl",
    "auth.json",
    "runtime.sqlite3",
    "DECODEX_AGENT_HOME",
    "migrate-agent-home",
]
REQUIRED_PREFLIGHT_FRAGMENTS = [
    "pwd",
    "git status --short --branch",
    "git rev-parse HEAD",
    "fail closed",
]
