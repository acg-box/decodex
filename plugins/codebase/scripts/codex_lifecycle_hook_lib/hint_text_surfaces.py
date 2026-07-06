"""Surface-specific lifecycle hook hint text."""

from __future__ import annotations

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
