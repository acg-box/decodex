#!/usr/bin/env python3
"""Tests for the lightweight Codex lifecycle hook."""

from __future__ import annotations

import contextlib
import io
import json
import sys
import tempfile
import unittest
from importlib.machinery import SourceFileLoader
from importlib.util import module_from_spec, spec_from_loader
from pathlib import Path


SCRIPT_PATH = (
    Path(__file__).parents[3] / "plugins" / "codebase" / "scripts" / "codex_lifecycle_hook"
)


def load_hook_module():
    loader = SourceFileLoader("codex_lifecycle_hook", str(SCRIPT_PATH))
    spec = spec_from_loader(loader.name, loader)
    if spec is None:
        raise RuntimeError("failed to load hook module spec")
    module = module_from_spec(spec)
    loader.exec_module(module)
    return module


class CodexLifecycleHookTests(unittest.TestCase):
    def setUp(self) -> None:
        self.hook = load_hook_module()
        self.hook.dependency_surface_paths = lambda paths=None: []
        self.hook.task_runner_paths = lambda paths=None: []
        self.hook.fake_modularization_paths = lambda paths=None: []

    def test_route_hints_selects_codebase_knowledge_and_deliberation(self) -> None:
        self.hook.large_change_paths = lambda stats=None: []
        self.hook.public_surface_paths = lambda paths=None: ["plugins/codebase/skills/work/SKILL.md"]

        hints = self.hook.route_hints(
            "Fix plugin docs after architecture decision and run skeptic review",
            "/tmp/repo",
            "PostToolUse",
        )

        joined = "\n".join(hints)
        self.assertIn("$codebase:work", joined)
        self.assertIn("OKF/LLM Wiki", joined)
        self.assertIn("$knowledge:writeback", joined)
        self.assertIn("$deliberation:skeptic", joined)

    def test_repo_work_prompt_requires_source_backed_docs_context(self) -> None:
        self.hook.large_change_paths = lambda stats=None: []
        self.hook.public_surface_paths = lambda paths=None: []

        hints = self.hook.route_hints("Fix the repo parser bug", "/tmp/repo")

        joined = "\n".join(hints)
        self.assertIn("Keep development source-backed", joined)
        self.assertIn("README/docs/AGENTS", joined)
        self.assertIn("repo-memory owner", joined)

    def test_route_hints_requires_git_root_for_codebase_prompt(self) -> None:
        hints = self.hook.route_hints("Fix this repo test", None)

        self.assertNotIn("$codebase:work", "\n".join(hints))

    def test_commit_prompt_adds_json_commit_contract(self) -> None:
        self.hook.large_change_paths = lambda stats=None: []
        self.hook.public_surface_paths = lambda paths=None: []
        self.hook.invalid_ahead_commit_subjects = lambda: []

        hints = self.hook.route_hints("git commit -m test && git push origin main", "/tmp/repo")

        joined = "\n".join(hints)
        self.assertIn("decodex/commit/1", joined)
        self.assertIn("single-line", joined)

    def test_git_push_reports_invalid_ahead_subjects(self) -> None:
        self.hook.large_change_paths = lambda stats=None: []
        self.hook.public_surface_paths = lambda paths=None: []
        self.hook.invalid_ahead_commit_subjects = lambda: ["plain prose subject"]

        hints = self.hook.route_hints("git push origin main", "/tmp/repo", "PreToolUse")

        joined = "\n".join(hints)
        self.assertIn("repair non-JSON ahead commit subjects", joined)
        self.assertIn("plain prose subject", joined)

    def test_root_task_branch_apply_patch_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"
        payload = {"tool_name": "apply_patch", "tool_input": {"patch": "*** Begin Patch"}}

        reason = self.hook.pre_tool_use_block_reason(
            payload,
            "/tmp/repo",
            "apply_patch *** Begin Patch",
        )

        self.assertIn("root worktree on a non-default branch", reason or "")
        self.assertIn(".worktrees/<task>", reason or "")

    def test_root_task_branch_read_only_command_is_allowed(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "git status --short"}},
            "/tmp/repo",
            "git status --short",
        )

        self.assertIsNone(reason)

    def test_root_task_branch_can_switch_back_to_default(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "git switch main"}},
            "/tmp/repo",
            "git switch main",
        )

        self.assertIsNone(reason)

    def test_root_task_branch_switch_back_mixed_with_mutation_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "git switch main && touch x"}},
            "/tmp/repo",
            "git switch main && touch x",
        )

        self.assertIn("root worktree on a non-default branch", reason or "")

    def test_root_task_branch_switch_back_with_redirection_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "git switch main > out"}},
            "/tmp/repo",
            "git switch main > out",
        )

        self.assertIn("root worktree on a non-default branch", reason or "")

    def test_root_task_branch_switch_back_with_command_substitution_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "git switch main $(touch x)"}},
            "/tmp/repo",
            "git switch main $(touch x)",
        )

        self.assertIn("root worktree on a non-default branch", reason or "")

    def test_root_task_branch_read_only_with_command_substitution_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "git status $(touch x)"}},
            "/tmp/repo",
            "git status $(touch x)",
        )

        self.assertIn("root worktree on a non-default branch", reason or "")

    def test_root_task_branch_quoted_command_substitution_search_is_allowed(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "rg '$(' docs"}},
            "/tmp/repo",
            "rg '$(' docs",
        )

        self.assertIsNone(reason)

    def test_root_task_branch_double_quoted_command_substitution_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "echo \"'$(touch x)'\""}},
            "/tmp/repo",
            "echo \"'$(touch x)'\"",
        )

        self.assertIn("root worktree on a non-default branch", reason or "")

    def test_root_task_branch_mutation_without_spaced_operator_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "git status&&touch out"}},
            "/tmp/repo",
            "git status&&touch out",
        )

        self.assertIn("root worktree on a non-default branch", reason or "")

    def test_root_task_branch_ampersand_separated_mutation_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "git status & touch out"}},
            "/tmp/repo",
            "git status & touch out",
        )

        self.assertIn("root worktree on a non-default branch", reason or "")

    def test_root_task_branch_newline_separated_mutation_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "git status\ntouch out"}},
            "/tmp/repo",
            "git status\ntouch out",
        )

        self.assertIn("root worktree on a non-default branch", reason or "")

    def test_root_task_branch_assignment_prefixed_mutation_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "VAR=1 touch x"}},
            "/tmp/repo",
            "VAR=1 touch x",
        )

        self.assertIn("root worktree on a non-default branch", reason or "")

    def test_root_task_branch_redirection_without_spaced_operator_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "echo hi>out"}},
            "/tmp/repo",
            "echo hi>out",
        )

        self.assertIn("root worktree on a non-default branch", reason or "")

    def test_root_default_branch_can_direct_push_mutate(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "main"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "apply_patch", "tool_input": {"patch": "*** Begin Patch"}},
            "/tmp/repo",
            "apply_patch *** Begin Patch",
        )

        self.assertIsNone(reason)

    def test_root_default_branch_task_switch_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "main"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "git switch -c xy/task"}},
            "/tmp/repo",
            "git switch -c xy/task",
        )

        self.assertIn("Refusing to switch the root worktree", reason or "")

    def test_root_default_branch_task_switch_with_git_c_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "main"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "git -C /tmp/repo switch -c xy/task"}},
            "/tmp/repo",
            "git -C /tmp/repo switch -c xy/task",
        )

        self.assertIn("Refusing to switch the root worktree", reason or "")

    def test_root_default_branch_assignment_prefixed_task_switch_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "main"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "VAR=1 git switch -c xy/task"}},
            "/tmp/repo",
            "VAR=1 git switch -c xy/task",
        )

        self.assertIn("Refusing to switch the root worktree", reason or "")

    def test_root_default_branch_env_prefixed_task_switch_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "main"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "env VAR=1 git switch -c xy/task"}},
            "/tmp/repo",
            "env VAR=1 git switch -c xy/task",
        )

        self.assertIn("Refusing to switch the root worktree", reason or "")

    def test_root_default_branch_env_option_prefixed_task_switch_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "main"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "env -i git switch -c xy/task"}},
            "/tmp/repo",
            "env -i git switch -c xy/task",
        )

        self.assertIn("Refusing to switch the root worktree", reason or "")

    def test_root_default_branch_env_double_dash_task_switch_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "main"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "env -- git switch -c xy/task"}},
            "/tmp/repo",
            "env -- git switch -c xy/task",
        )

        self.assertIn("Refusing to switch the root worktree", reason or "")

    def test_root_default_branch_env_split_string_task_switch_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "main"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "env -S 'git switch -c xy/task'"}},
            "/tmp/repo",
            "env -S 'git switch -c xy/task'",
        )

        self.assertIn("Refusing to switch the root worktree", reason or "")

    def test_root_default_branch_env_attached_split_string_task_switch_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "main"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "env -S'git switch -c xy/task'"}},
            "/tmp/repo",
            "env -S'git switch -c xy/task'",
        )

        self.assertIn("Refusing to switch the root worktree", reason or "")

    def test_root_task_branch_env_prefixed_git_mutation_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "env VAR=1 git reset --hard"}},
            "/tmp/repo",
            "env VAR=1 git reset --hard",
        )

        self.assertIn("root worktree on a non-default branch", reason or "")

    def test_root_task_branch_env_option_git_mutation_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "env -i git reset --hard"}},
            "/tmp/repo",
            "env -i git reset --hard",
        )

        self.assertIn("root worktree on a non-default branch", reason or "")

    def test_root_task_branch_env_split_string_git_mutation_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "env -S 'git reset --hard'"}},
            "/tmp/repo",
            "env -S 'git reset --hard'",
        )

        self.assertIn("root worktree on a non-default branch", reason or "")

    def test_root_task_branch_env_attached_split_string_git_mutation_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "env -S'git reset --hard'"}},
            "/tmp/repo",
            "env -S'git reset --hard'",
        )

        self.assertIn("root worktree on a non-default branch", reason or "")

    def test_root_default_branch_git_c_worktree_task_switch_is_allowed(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "main"
        self.hook.git_command_cwd_targets_root = lambda command_cwd, root: False

        reason = self.hook.pre_tool_use_block_reason(
            {
                "tool_name": "exec_command",
                "tool_input": {"cmd": "git -C .worktrees/task switch -c xy/task"},
            },
            "/tmp/repo",
            "git -C .worktrees/task switch -c xy/task",
        )

        self.assertIsNone(reason)

    def test_root_default_branch_git_c_worktrees_container_task_switch_is_blocked(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "main"

        reason = self.hook.pre_tool_use_block_reason(
            {
                "tool_name": "exec_command",
                "tool_input": {"cmd": "git -C .worktrees switch -c xy/task"},
            },
            "/tmp/repo",
            "git -C .worktrees switch -c xy/task",
        )

        self.assertIn("Refusing to switch the root worktree", reason or "")

    def test_root_default_branch_checkout_path_is_allowed(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "main"
        self.hook.git_branch_ref_exists = lambda target: False

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "git checkout -- README.md"}},
            "/tmp/repo",
            "git checkout -- README.md",
        )

        self.assertIsNone(reason)

    def test_root_default_branch_checkout_treeish_path_is_allowed(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "main"
        self.hook.git_branch_ref_exists = lambda target: True

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "git checkout HEAD -- README.md"}},
            "/tmp/repo",
            "git checkout HEAD -- README.md",
        )

        self.assertIsNone(reason)

    def test_root_non_default_read_only_search_with_git_words_is_allowed(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": 'rg "git reset" docs'}},
            "/tmp/repo",
            'rg "git reset" docs',
        )

        self.assertIsNone(reason)

    def test_root_non_default_read_only_search_with_quoted_redirection_is_allowed(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": 'rg "a > b" docs'}},
            "/tmp/repo",
            'rg "a > b" docs',
        )

        self.assertIsNone(reason)

    def test_root_non_default_read_only_search_single_quoted_redirection_is_allowed(self) -> None:
        self.hook.git_is_root_worktree = lambda: True
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "exec_command", "tool_input": {"cmd": "rg '>' docs"}},
            "/tmp/repo",
            "rg '>' docs",
        )

        self.assertIsNone(reason)

    def test_main_pre_tool_use_block_emits_decision_json_only(self) -> None:
        self.hook.load_payload = lambda: {"tool_name": "apply_patch", "tool_input": {"patch": "x"}}
        self.hook.git_root = lambda: "/tmp/repo"
        self.hook.pre_tool_use_block_reason = lambda payload, root, text: "blocked for test"
        self.hook.record_event = lambda event, payload, root, hints: None
        original_argv = sys.argv
        output = io.StringIO()
        try:
            sys.argv = ["codex_lifecycle_hook", "--event", "PreToolUse"]
            with contextlib.redirect_stdout(output):
                exit_code = self.hook.main()
        finally:
            sys.argv = original_argv

        self.assertEqual(exit_code, 0)
        self.assertEqual(json.loads(output.getvalue()), {"decision": "block", "reason": "blocked for test"})

    def test_main_pre_tool_use_uses_tool_input_command_text(self) -> None:
        seen: dict[str, str] = {}
        self.hook.load_payload = lambda: {
            "tool_name": "exec_command",
            "tool_input": {"cmd": "git switch -c xy/task"},
        }
        self.hook.git_root = lambda: "/tmp/repo"

        def fake_block_reason(payload, root, text):
            seen["text"] = text
            return "blocked for test"

        self.hook.pre_tool_use_block_reason = fake_block_reason
        self.hook.record_event = lambda event, payload, root, hints: None
        original_argv = sys.argv
        output = io.StringIO()
        try:
            sys.argv = ["codex_lifecycle_hook", "--event", "PreToolUse"]
            with contextlib.redirect_stdout(output):
                exit_code = self.hook.main()
        finally:
            sys.argv = original_argv

        self.assertEqual(exit_code, 0)
        self.assertEqual(seen["text"], "git switch -c xy/task")

    def test_linked_worktree_task_branch_mutation_is_allowed(self) -> None:
        self.hook.git_is_root_worktree = lambda: False
        self.hook.git_default_branch = lambda: "main"
        self.hook.git_current_branch = lambda: "xy/task"

        reason = self.hook.pre_tool_use_block_reason(
            {"tool_name": "apply_patch", "tool_input": {"patch": "*** Begin Patch"}},
            "/tmp/repo/.worktrees/task",
            "apply_patch *** Begin Patch",
        )

        self.assertIsNone(reason)

    def test_ready_with_large_change_adds_monolith_guard(self) -> None:
        self.hook.large_change_paths = lambda stats=None: ["src/large.rs"]
        self.hook.public_surface_paths = lambda paths=None: []

        hints = self.hook.route_hints("git commit -m test", "/tmp/repo", "PreToolUse")

        joined = "\n".join(hints)
        self.assertIn("$deliberation:skeptic", joined)
        self.assertIn("module-boundary", joined)

    def test_source_growth_adds_module_guard(self) -> None:
        self.hook.git_root = lambda: "/tmp/repo"
        self.hook.file_line_count = lambda path, root=None: 1200
        stats = [{"path": "src/app.tsx", "added": "120", "removed": "0", "changed": 120}]

        self.assertEqual(self.hook.large_change_paths(stats), ["src/app.tsx"])

    def test_fake_modularization_paths_detects_rust_include_escape(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = root / "src"
            src.mkdir()
            (src / "lib.rs").write_text(
                'mod generated { include!("fragments/generated.rs"); }\n',
                encoding="utf-8",
            )
            self.hook.git_root = lambda: str(root)
            # Exercise the real helper instead of the setUp stub.
            module = load_hook_module()
            module.git_root = lambda: str(root)

            self.assertEqual(
                module.fake_modularization_paths(["src/lib.rs", "src/view.ts"]),
                ["src/lib.rs"],
            )

    def test_fake_modularization_adds_hard_guard(self) -> None:
        self.hook.large_change_paths = lambda stats=None: []
        self.hook.public_surface_paths = lambda paths=None: []
        self.hook.fake_modularization_paths = lambda paths=None: ["src/lib.rs"]

        hints = self.hook.route_hints("", "/tmp/repo", "PostToolUse")

        joined = "\n".join(hints)
        self.assertIn("physical file splitting is not a Rust module boundary", joined)
        self.assertIn("normal `mod` modules", joined)
        self.assertIn("$deliberation:skeptic", joined)

    def test_small_source_change_does_not_add_module_guard(self) -> None:
        self.hook.git_root = lambda: "/tmp/repo"
        self.hook.file_line_count = lambda path, root=None: 200
        stats = [{"path": "src/app.tsx", "added": "20", "removed": "4", "changed": 24}]

        self.assertEqual(self.hook.large_change_paths(stats), [])

    def test_public_surface_change_adds_docs_coupling(self) -> None:
        self.hook.large_change_paths = lambda stats=None: []
        self.hook.public_surface_paths = lambda paths=None: ["plugins/codebase/skills/work/SKILL.md"]

        hints = self.hook.route_hints("", "/tmp/repo", "PostToolUse")

        joined = "\n".join(hints)
        self.assertIn("Use English for every durable or executable artifact", joined)
        self.assertIn("$knowledge:docs-drift", joined)
        self.assertIn("$knowledge:writeback", joined)

    def test_dependency_surface_adds_dependency_policy_hint(self) -> None:
        self.hook.large_change_paths = lambda stats=None: []
        self.hook.public_surface_paths = lambda paths=None: []
        self.hook.dependency_surface_paths = lambda paths=None: ["Cargo.toml"]

        hints = self.hook.route_hints("", "/tmp/repo", "PostToolUse")

        joined = "\n".join(hints)
        self.assertIn("$codebase:dependency-policy", joined)
        self.assertIn("roll or style-only", joined)
        self.assertIn("discovered-candidate inventory", joined)
        self.assertIn("residual dependency checks", joined)

    def test_task_runner_surface_adds_task_runner_hint(self) -> None:
        self.hook.large_change_paths = lambda stats=None: []
        self.hook.public_surface_paths = lambda paths=None: []
        self.hook.task_runner_paths = lambda paths=None: ["Makefile.toml"]

        hints = self.hook.route_hints("", "/tmp/repo", "PostToolUse")

        joined = "\n".join(hints)
        self.assertIn("task-runner checklist", joined)
        self.assertIn("action-first public names", joined)
        self.assertIn("no command aliases", joined)

    def test_public_surface_matching_avoids_substring_false_positive(self) -> None:
        self.assertFalse(self.hook.path_is_public_surface("src/helpful.rs"))
        self.assertFalse(self.hook.path_is_public_surface("tests/status_parser.rs"))
        self.assertTrue(self.hook.path_is_public_surface("docs/reference/index.md"))
        self.assertTrue(self.hook.path_is_public_surface("plugins/codebase/skills/work/SKILL.md"))

    def test_user_prompt_adds_conditional_deliberation_gate(self) -> None:
        self.hook.large_change_paths = lambda stats=None: []
        self.hook.public_surface_paths = lambda paths=None: []

        hints = self.hook.route_hints("Should we refactor this auth architecture?", "/tmp/repo")

        joined = "\n".join(hints)
        self.assertIn("Use English for every durable or executable artifact", joined)
        self.assertIn("When the task involves design", joined)
        self.assertIn("$deliberation:grill", joined)
        self.assertIn("$deliberation:scout", joined)
        self.assertIn("$deliberation:skeptic", joined)
        self.assertIn("1-2 files or one command", joined)
        self.assertIn("Do not wait for the user", joined)
        self.assertIn("bounded read-only scout/skeptic subagents", joined)

    def test_commit_subject_validation(self) -> None:
        valid = '{"schema":"decodex/commit/1","summary":"ship guard","authority":"XY-1099"}'

        self.assertTrue(self.hook.commit_subject_is_valid(valid))
        self.assertFalse(self.hook.commit_subject_is_valid("ship guard"))
        self.assertFalse(self.hook.commit_subject_is_valid('{"schema":"decodex/commit/1"}'))

    def test_walk_strings_collects_nested_payload_text(self) -> None:
        payload = {"prompt": ["hello", {"nested": "world"}], "count": 3}

        self.assertEqual(self.hook.walk_strings(payload), ["hello", "world"])

    def test_record_event_writes_jsonl_without_prompt_sample(self) -> None:
        self.hook.changed_file_stats = lambda: []
        self.hook.public_surface_paths = lambda: []
        with tempfile.TemporaryDirectory() as tmp:
            state_dir = Path(tmp)
            self.hook.STATE_DIR = state_dir
            self.hook.STATE_PATH = state_dir / "events.jsonl"

            self.hook.record_event(
                "UserPromptSubmit",
                {"prompt": "Fix docs with private context"},
                "/tmp/repo",
                ["hint"],
            )

            line = self.hook.STATE_PATH.read_text(encoding="utf-8").strip()
            record = json.loads(line)
            self.assertEqual(record["event"], "UserPromptSubmit")
            self.assertEqual(record["git_root"], "/tmp/repo")
            self.assertEqual(record["hints"], ["hint"])
            self.assertNotIn("sample", record)


if __name__ == "__main__":
    unittest.main()
