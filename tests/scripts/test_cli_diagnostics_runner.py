"""Regression tests for the real CLI diagnostics runner."""

from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
RUNNER_PATH = ROOT / "scripts/vnext/cli_diagnostics_test.py"


def load_runner():
    spec = importlib.util.spec_from_file_location(
        "cli_diagnostics_test",
        RUNNER_PATH,
    )
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class CliDiagnosticsRunnerTests(unittest.TestCase):
    def test_cli_binary_uses_the_cargo_target_directory(self) -> None:
        runner = load_runner()
        target_directory = ROOT / ".test-target"

        with mock.patch.dict(
            os.environ,
            {"CARGO_TARGET_DIR": str(target_directory)},
        ):
            self.assertEqual(
                runner.cli_binary(),
                target_directory / "debug" / "decodex",
            )

    def test_relative_cargo_target_directory_is_rooted_at_workspace(self) -> None:
        runner = load_runner()

        with mock.patch.dict(
            os.environ,
            {"CARGO_TARGET_DIR": ".test-target"},
        ):
            self.assertEqual(
                runner.cli_binary(),
                ROOT / ".test-target/debug/decodex",
            )

    def test_cli_binary_defaults_to_the_workspace_target_directory(self) -> None:
        runner = load_runner()

        with mock.patch.dict(os.environ, {}, clear=True):
            self.assertEqual(
                runner.cli_binary(),
                ROOT / "target/debug/decodex",
            )


if __name__ == "__main__":
    unittest.main()
