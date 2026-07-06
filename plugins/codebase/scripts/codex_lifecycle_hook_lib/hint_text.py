"""User-facing lifecycle hook hint text facade."""

from __future__ import annotations

from .hint_text_base import (
    COMMIT_SCHEMA,
    COMMIT_STYLE_HINT,
    ENGLISH_ONLY_HINT,
    KNOWLEDGE_HINT,
    REPO_WORK_HINT,
)
from .hint_text_deliberation import DELIBERATION_GATE_HINT
from .hint_text_module import (
    ANTI_MONOLITH_HINT,
    FAKE_MODULARIZATION_HINT,
    MODULE_BOUNDARY_HINT,
)
from .hint_text_runtime import (
    COMMIT_COMMAND_TERMS,
    ROOT_TASK_BRANCH_BLOCK,
    ROOT_TASK_BRANCH_SWITCH_BLOCK,
)
from .hint_text_surfaces import DEPENDENCY_POLICY_HINT, PUBLIC_SURFACE_HINT, TASK_RUNNER_HINT
