"""Runtime and branch-safety lifecycle hook hint text."""

from __future__ import annotations

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
COMMIT_COMMAND_TERMS = [
    "git commit",
    "git push",
    "gh pr merge",
]
