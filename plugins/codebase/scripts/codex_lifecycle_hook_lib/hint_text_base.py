"""General lifecycle hook hint text."""

from __future__ import annotations

COMMIT_SCHEMA = "decodex/commit/1"

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
