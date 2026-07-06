"""Smoke tests for the codebase lifecycle hook plugin bundle."""

from __future__ import annotations

import unittest
from importlib.machinery import SourceFileLoader
from importlib.util import module_from_spec, spec_from_loader
from pathlib import Path


SCRIPT_PATH = Path(__file__).parents[1] / "scripts" / "codex_lifecycle_hook"


def load_hook_module():
    loader = SourceFileLoader("codebase_plugin_lifecycle_hook", str(SCRIPT_PATH))
    spec = spec_from_loader(loader.name, loader)
    if spec is None:
        raise RuntimeError("failed to load lifecycle hook module spec")
    module = module_from_spec(spec)
    loader.exec_module(module)
    return module


class LifecycleHookSmokeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.hook = load_hook_module()
        self.hook.large_change_paths = lambda stats=None: []
        self.hook.public_surface_paths = lambda paths=None: []
        self.hook.dependency_surface_paths = lambda paths=None: []
        self.hook.task_runner_paths = lambda paths=None: []
        self.hook.fake_modularization_paths = lambda paths=None: []
        self.hook.module_boundary_risk_paths = lambda paths=None: []

    def test_module_prompt_routes_to_codebase_work(self) -> None:
        hints = self.hook.route_hints("Refactor this module boundary", "/tmp/repo")

        joined = "\n".join(hints)
        self.assertIn("$codebase:work", joined)
        self.assertIn("Module-boundary work is in scope", joined)

    def test_read_only_root_task_branch_command_is_allowed(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "git status --short"}},
            "/tmp/repo",
            "git status --short",
        )

        self.assertIsNone(reason)


if __name__ == "__main__":
    unittest.main()
