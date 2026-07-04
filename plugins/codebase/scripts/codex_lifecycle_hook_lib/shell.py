"""Shell and Git command classification facade for lifecycle hook safeguards."""

from __future__ import annotations

from .shell_git import (
    checkout_branch_target,
    git_command_cwd_targets_root,
    git_switch_targets,
    split_git_command,
    switches_root_back_to_default_branch,
    switches_root_to_non_default_branch,
    target_is_default_branch,
)
from .shell_mutation import (
    has_shell_control_or_redirection,
    has_shell_substitution,
    has_unquoted_redirection,
    has_unquoted_shell_control,
    is_only_root_switch_back,
    is_switch_back_segment,
    payload_is_mutating,
    payload_tool_name,
    shell_segment_is_mutating,
)
from .shell_tokens import (
    shell_segments,
    shell_token_segments,
    shell_tokens,
    text_has_any,
    unwrap_shell_command_tokens,
)
