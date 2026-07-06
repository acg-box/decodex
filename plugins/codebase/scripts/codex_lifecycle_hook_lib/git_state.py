"""Git state compatibility facade for lifecycle hook checks."""

from __future__ import annotations

from .git_commit_state import (
    ahead_commit_subjects,
    commit_subject_is_valid,
    invalid_ahead_commit_subjects,
)
from .git_core import git_output, git_root
from .git_diff_state import changed_file_stats, changed_paths, file_line_count
from .git_worktree_state import (
    git_branch_ref_exists,
    git_current_branch,
    git_default_branch,
    git_is_root_worktree,
)
