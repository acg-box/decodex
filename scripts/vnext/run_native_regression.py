#!/usr/bin/env python3
"""Run one native fixture with a temporary HOME and the exact prepared runtime.

Use one HOME-owning fixture per invocation. Existing DECODEX_TEST_* mode flags
are passed to the fixture. No desktop application is installed or launched.
"""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", required=True, type=Path)
    parser.add_argument("--filter", required=True, help="One exact Nextest fixture selector")
    args = parser.parse_args()
    runtime = args.runtime.resolve(strict=True)
    environment = os.environ.copy()
    environment["CARGO_HOME"] = environment.get("CARGO_HOME", str(Path.home() / ".cargo"))
    environment["RUSTUP_HOME"] = environment.get("RUSTUP_HOME", str(Path.home() / ".rustup"))
    environment.pop("CODEX_HOME", None)

    # Darwin local sockets need a short canonical path. Use system temporary storage.
    with tempfile.TemporaryDirectory(prefix="dx-native-", dir="/tmp") as directory:
        fixture = Path(directory).resolve()
        (fixture / ".decodex-recap-fixture").write_text("isolated-recap\n")
        (fixture / ".decodex-startup-config-fixture").write_text("isolated-native\n")
        environment.update(
            HOME=str(fixture),
            DECODEX_TEST_ACCOUNT_HOME=str(fixture),
            DECODEX_TEST_CODEX_BINARY=str(runtime / "CodexCLI.app/Contents/MacOS/codex"),
            DECODEX_CODEX_DEVELOPMENT_RUNTIME=str(runtime),
        )
        preparation = environment | {"CODEX_HOME": str(fixture / "preparation-native-home")}
        result = subprocess.run(
            ["cargo", "make", "prepare-codex"], cwd=ROOT, env=preparation, check=False
        )
        if result.returncode:
            return result.returncode
        return subprocess.run(
            [
                "cargo", "+stable", "nextest", "run", "--locked", "-p", "decodex-runtime",
                "--lib", "--all-features", "--run-ignored", "only", "-E", args.filter,
                "--test-threads", "1",
            ],
            cwd=ROOT,
            env=environment,
            check=False,
        ).returncode


if __name__ == "__main__":
    raise SystemExit(main())
