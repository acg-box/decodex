"""Path classification and large-change heuristic facade."""

from __future__ import annotations

from .surface_base import (
    path_excluded_from_large_change,
    path_is_source_file,
)
from .surface_dependency import (
    dependency_surface_paths,
    path_is_dependency_surface,
    path_is_task_runner_surface,
    task_runner_paths,
)
from .surface_large_change import large_change_paths
from .surface_module import (
    fake_modularization_paths,
    module_boundary_risk_paths,
    path_has_generic_module_bucket,
)
from .surface_public import path_is_public_surface, public_surface_paths
