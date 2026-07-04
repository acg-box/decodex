"""Lifecycle hook constants and hint text."""

from __future__ import annotations

from pathlib import Path

from typing import Any


STATE_DIR = Path.home() / ".codex" / "hack-ink-hooks"
STATE_PATH = STATE_DIR / "events.jsonl"
COMMIT_SCHEMA = "decodex/commit/1"
LARGE_CHANGE_THRESHOLD = 800
LARGE_ADDITION_THRESHOLD = 500
SOURCE_ADDITION_THRESHOLD = 250
NEW_SOURCE_FILE_THRESHOLD = 350
LARGE_SOURCE_FILE_LINE_THRESHOLD = 1000
LARGE_SOURCE_TOUCH_THRESHOLD = 80

REPO_WORK_HINT = (
    "For non-trivial repository implementation or verification, load $codebase:work. "
    "Keep development source-backed: inspect the nearest checked-in README/docs/"
    "AGENTS, OKF/LLM Wiki, or repo-memory owner when that context may shape command "
    "authority, architecture, or durable claims. Use $codebase:verification before "
    "ready/done/fixed claims."
)
KNOWLEDGE_HINT = (
    "For docs, OKF/LLM Wiki, semantic drift, repo-memory, or knowledge updates, "
    "load the matching $knowledge:* owner, including $knowledge:writeback after "
    "stable changes."
)
ENGLISH_ONLY_HINT = (
    "Use English for every durable or executable artifact by default. Direct "
    "user-facing chat may mirror the user's language. Non-English durable content "
    "requires an explicit user request, preserved external source text, or a "
    "language/locale artifact such as i18n, translation fixtures, tokenizer tests, "
    "or locale catalogs."
)

COMMIT_STYLE_HINT = (
    "When Codex creates or pushes commits in this checkout, use a single-line "
    f"`{COMMIT_SCHEMA}` JSON commit message with required `schema`, `summary`, "
    "and `authority` fields. Do not use prose commit subjects."
)
ANTI_MONOLITH_HINT = (
    "Large, generated, or growing implementation files are present. Before commit, "
    "push, or ready/done claims, load $codebase:work and run a module-boundary check: "
    "split files that mix unrelated concerns or state why the owner boundary is "
    "deliberate. Use $deliberation:skeptic when the structure is material."
)
FAKE_MODULARIZATION_HINT = (
    "Rust files in the current diff use `include!`, `#[path]`, or equivalent "
    "original-scope fragment wiring. Do not claim this as modularization: physical "
    "file splitting is not a Rust module boundary. Replace it with normal `mod` "
    "modules, explicit owner APIs, and visibility boundaries, or document the include "
    "as generated/FFI adapter plumbing that does not count toward the refactor."
)
PUBLIC_SURFACE_HINT = (
    "Changed files include docs, plugin/skill, command/config, status/help, or "
    "workflow-facing surfaces. Before ready/done/commit, read or update the "
    "source-backed docs/OKF/LLM Wiki owner; use $knowledge:docs-drift or "
    "$knowledge:writeback when a source-backed claim changed."
)
DEPENDENCY_POLICY_HINT = (
    "Changed files include dependency manifests, lockfiles, generated dependency "
    "artifacts, or workflow action refs. Load $codebase:dependency-policy and report "
    "whether this is a roll or style-only dependency change with version/SHA evidence, "
    "a discovered-candidate inventory, and residual dependency checks."
)
TASK_RUNNER_HINT = (
    "Changed files include Makefile.toml or an equivalent task-runner surface. Load "
    "$codebase:work and apply the task-runner checklist: action-family grouping, "
    "action-first public names, no command aliases, deterministic ordering, no long "
    "inline shell, and stale-command reverse checks."
)

ROOT_TASK_BRANCH_BLOCK = (
    "Refusing repository mutation from the root worktree on a non-default branch. "
    "Root direct-push work must stay on the default branch. PR/task-branch work "
    "must happen in `.worktrees/<task>/`; a task branch in the root worktree is not "
    "an isolated working context."
)
ROOT_TASK_BRANCH_SWITCH_BLOCK = (
    "Refusing to switch the root worktree to a task branch. Root direct-push work "
    "must stay on the default branch. PR/task-branch work must happen in "
    "`.worktrees/<task>/`."
)
DELIBERATION_GATE_HINT = (
    "When the task involves design, architecture, refactor, root-cause debugging, "
    "research, option comparison, or important ready/done claims, use compact "
    "first-principles framing with $deliberation:grill, source-backed evidence "
    "with $deliberation:scout when facts are not local and obvious, and skeptic "
    "review with $deliberation:skeptic before material conclusions. Inline only "
    "for one local question that fits in 1-2 files or one command and cannot affect "
    "architecture, debugging, review repair, public contracts, docs drift, "
    "commit/land, or ready/done decisions. Do not wait for the user to explicitly "
    "request subagents; when the inline exception fails and subagent tools "
    "are allowed, dispatch bounded read-only scout/skeptic subagents. If "
    "subagent tools are unavailable, name the inline fallback."
)

COMMIT_COMMAND_TERMS = [
    "git commit",
    "git push",
    "gh pr merge",
]
ALWAYS_MUTATING_TOOL_NAMES = {
    "apply_patch",
    "edit",
    "write",
}
MUTATING_COMMAND_TERMS = (
    "add",
    "am",
    "apply",
    "cherry-pick",
    "clean",
    "commit",
    "merge",
    "mv",
    "pull",
    "push",
    "rebase",
    "reset",
    "restore",
    "rm",
    "stash",
    "switch",
    "checkout",
)
GIT_GLOBAL_OPTIONS_WITH_ARG = {
    "-C",
    "-c",
    "--exec-path",
    "--git-dir",
    "--namespace",
    "--super-prefix",
    "--work-tree",
}
GIT_GLOBAL_OPTIONS_WITH_VALUE_PREFIX = (
    "--exec-path=",
    "--git-dir=",
    "--namespace=",
    "--super-prefix=",
    "--work-tree=",
)
GIT_BRANCH_CREATE_OPTIONS = {
    "-b",
    "-B",
    "-c",
    "-C",
    "--create",
    "--force-create",
    "--orphan",
}
GIT_OPTIONS_WITH_ARG = {
    "--conflict",
    "--pathspec-from-file",
}
SHELL_MUTATING_COMMANDS = {
    "cp",
    "install",
    "mkdir",
    "mv",
    "rm",
    "tee",
    "touch",
}
REDIRECTION_OPERATORS = {
    ">",
    ">>",
    "1>",
    "1>>",
    "2>",
    "2>>",
    "&>",
    "&>>",
}
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
