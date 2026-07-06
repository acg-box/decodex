"""Git command classification facade for lifecycle hook safeguards."""

from __future__ import annotations

from .shell_git_checkout import checkout_branch_target, target_is_default_branch
from .shell_git_parse import split_git_command
from .shell_git_paths import git_command_cwd_targets_root
from .shell_git_switch import (
    git_switch_targets,
    switches_root_back_to_default_branch,
    switches_root_to_non_default_branch,
)
