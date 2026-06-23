#!/usr/bin/env python3
"""Tests for the lightweight Codex lifecycle hook."""

from __future__ import annotations

import json
import tempfile
import unittest
from importlib.machinery import SourceFileLoader
from importlib.util import module_from_spec, spec_from_loader
from pathlib import Path


SCRIPT_PATH = Path(__file__).with_name("codex_lifecycle_hook")


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

    def test_route_hints_selects_codebase_knowledge_and_deliberation(self) -> None:
        hints = self.hook.route_hints(
            "Fix plugin docs after architecture decision and run skeptic review",
            "/tmp/repo",
        )

        joined = "\n".join(hints)
        self.assertIn("$codebase:work", joined)
        self.assertIn("$knowledge:writeback", joined)
        self.assertIn("$deliberation:challenge", joined)

    def test_route_hints_requires_git_root_for_codebase_prompt(self) -> None:
        hints = self.hook.route_hints("Fix this repo test", None)

        self.assertNotIn("$codebase:work", "\n".join(hints))

    def test_walk_strings_collects_nested_payload_text(self) -> None:
        payload = {"prompt": ["hello", {"nested": "world"}], "count": 3}

        self.assertEqual(self.hook.walk_strings(payload), ["hello", "world"])

    def test_record_event_writes_jsonl(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            state_dir = Path(tmp)
            self.hook.STATE_DIR = state_dir
            self.hook.STATE_PATH = state_dir / "events.jsonl"

            self.hook.record_event("UserPromptSubmit", {"prompt": "Fix docs"}, "/tmp/repo", ["hint"])

            line = self.hook.STATE_PATH.read_text(encoding="utf-8").strip()
            record = json.loads(line)
            self.assertEqual(record["event"], "UserPromptSubmit")
            self.assertEqual(record["git_root"], "/tmp/repo")
            self.assertEqual(record["hints"], ["hint"])


if __name__ == "__main__":
    unittest.main()
