#!/usr/bin/env python3
"""Run the real CLI against the fixed owner-only same-UID Unix endpoint."""

from __future__ import annotations

import os
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def cli_binary() -> Path:
    """Return the CLI built by Cargo for this validation process."""
    target_directory = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    if not target_directory.is_absolute():
        target_directory = ROOT / target_directory
    return target_directory.resolve() / "debug" / "decodex"


def main() -> None:
    subprocess.run(
        [
            "cargo",
            "build",
            "-p",
            "decodex-cli",
            "--all-features",
            "--all-targets",
        ],
        cwd=ROOT,
        check=True,
    )
    environment = os.environ.copy()
    environment["DECODEX_TEST_CLI_BINARY"] = str(cli_binary())
    subprocess.run(
        [
            "cargo",
            "test",
            "-p",
            "decodex-runtime",
            "--test",
            "cli_diagnostics",
            "--all-features",
            "--",
            "--ignored",
        ],
        cwd=ROOT,
        env=environment,
        check=True,
    )


if __name__ == "__main__":
    main()
