"""Shell mutation classification facade for lifecycle hook safeguards."""

from __future__ import annotations

from .shell_command_mutation import (
    payload_is_mutating,
    payload_tool_name,
    shell_segment_is_mutating,
)
from .shell_control import has_shell_control_or_redirection, has_unquoted_shell_control
from .shell_quote_scan import has_shell_substitution, has_unquoted_redirection
from .shell_switch_back import is_only_root_switch_back, is_switch_back_segment
