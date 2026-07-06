"""Changed-path and surface classification constants for lifecycle hook safeguards."""

from __future__ import annotations

from pathlib import Path

STATE_DIR = Path.home() / ".codex" / "hack-ink-hooks"
STATE_PATH = STATE_DIR / "events.jsonl"
LARGE_CHANGE_THRESHOLD = 800
LARGE_ADDITION_THRESHOLD = 500
SOURCE_ADDITION_THRESHOLD = 250
NEW_SOURCE_FILE_THRESHOLD = 350
LARGE_SOURCE_FILE_LINE_THRESHOLD = 1000
LARGE_SOURCE_TOUCH_THRESHOLD = 80
PUBLIC_SURFACE_PREFIXES = (
    ".agents/",
    ".codex/",
    ".github/",
    "docs/",
    "plugins/",
    "scripts/",
)
PUBLIC_SURFACE_NAMES = {
    "AGENTS.md",
    "README.md",
    "SKILL.md",
}
TASK_RUNNER_NAMES = {
    "Justfile",
    "Makefile.toml",
    "Taskfile.yaml",
    "Taskfile.yml",
    "justfile",
}
DEPENDENCY_SURFACE_NAMES = {
    "Cargo.lock",
    "Cargo.toml",
    "Gemfile",
    "Gemfile.lock",
    "bun.lock",
    "bun.lockb",
    "go.mod",
    "go.sum",
    "package-lock.json",
    "package.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "poetry.lock",
    "pyproject.toml",
    "requirements.txt",
    "yarn.lock",
}
SOURCE_EXTENSIONS = {
    ".c",
    ".cc",
    ".cpp",
    ".cs",
    ".css",
    ".go",
    ".h",
    ".hpp",
    ".html",
    ".java",
    ".js",
    ".jsx",
    ".kt",
    ".mjs",
    ".php",
    ".py",
    ".rb",
    ".rs",
    ".scss",
    ".sh",
    ".sql",
    ".swift",
    ".ts",
    ".tsx",
    ".vue",
    ".zig",
}
GENERIC_MODULE_BUCKET_SEGMENTS = (
    "common",
    "misc",
    "shared",
    "utils",
)
MODULE_BOUNDARY_PROMPT_TERMS = [
    "code organization",
    "file split",
    "file splits",
    "file merge",
    "file merges",
    "merge files",
    "modular",
    "modularization",
    "module boundary",
    "module-boundary",
    "monolith",
    "refactor",
    "split file",
    "split files",
]
FAKE_RUST_MODULARIZATION_PATTERNS = (
    "include!(",
    "#[path",
)
PUBLIC_SURFACE_SEGMENTS = (
    "config",
    "configs",
    "help",
    "hook",
    "hooks",
    "skill",
    "skills",
    "workflow",
    "workflows",
)
PUBLIC_SURFACE_STEMS = (
    "config",
    "help",
    "hook",
    "skill",
    "status",
    "workflow",
)
EXCLUDED_LARGE_CHANGE_PARTS = (
    "/dist/",
    "/target/",
    "/node_modules/",
    "/vendor/",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "Cargo.lock",
)
