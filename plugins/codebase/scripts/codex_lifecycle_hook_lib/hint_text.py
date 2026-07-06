"""User-facing lifecycle hook hint text."""

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
ANTI_MONOLITH_HINT = (
    "Large, generated, or growing implementation files are present. Treat size as "
    "a module-boundary review trigger, not a split rule. Before commit, push, or "
    "ready/done claims, load $codebase:work and check ownership: split files that "
    "mix unrelated concerns, or state why the current owner boundary is deliberate. "
    "Use $deliberation:skeptic when the structure is material."
)
MODULE_BOUNDARY_HINT = (
    "Module-boundary work is in scope. Load $codebase:work and use ownership rules "
    "before judging or editing: split or merge by responsibility, public contract, "
    "state ownership, change cadence, validation surface, and reader navigation. "
    "Do not use fixed line counts as the decision rule."
)
FAKE_MODULARIZATION_HINT = (
    "The current diff has signs of pseudo-modularization such as textual includes, "
    "original-scope fragment wiring, compatibility shims, or files that only move "
    "code without creating an owner boundary. Do not claim that as modularization. "
    "Replace it with explicit owner APIs and visibility boundaries, or document it "
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
