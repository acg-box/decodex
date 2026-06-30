#!/usr/bin/env python3
"""Tests for the lightweight Codex lifecycle hook."""

from __future__ import annotations

import json
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
