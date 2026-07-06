"""Dependency and task-runner surface path classification."""

from __future__ import annotations

from .constants import DEPENDENCY_SURFACE_NAMES, TASK_RUNNER_NAMES
from .git_state import changed_paths


def path_is_task_runner_surface(path: str) -> bool:
    return path.rsplit("/", maxsplit=1)[-1] in TASK_RUNNER_NAMES


def task_runner_paths(paths: list[str] | None = None) -> list[str]:
    paths = changed_paths() if paths is None else paths
    return [path for path in paths if path_is_task_runner_surface(path)]


def path_is_dependency_surface(path: str) -> bool:
    name = path.rsplit("/", maxsplit=1)[-1]
    if name in DEPENDENCY_SURFACE_NAMES:
        return True
    return path.startswith(".github/workflows/") and path.endswith((".yml", ".yaml"))


def dependency_surface_paths(paths: list[str] | None = None) -> list[str]:
    paths = changed_paths() if paths is None else paths
    return [path for path in paths if path_is_dependency_surface(path)]
