#!/usr/bin/env python3
"""Run one native fixture with a temporary HOME and the exact prepared runtime.

Use one HOME-owning fixture per invocation. Existing DECODEX_TEST_* mode flags
are passed to the fixture. --desktop explicitly launches an existing build in
the temporary profile. No application is installed or published.
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
    parser.add_argument("--filter", help="One exact Nextest fixture selector")
    parser.add_argument("--lock", type=Path, help="Explicit candidate lock for isolated component verification")
    parser.add_argument("--desktop", type=Path, help="Existing desktop executable for an isolated rehearsal")
    parser.add_argument("--provider-recovery", action="store_true", help="Rehearse the provider mismatch and fork flow")
    args = parser.parse_args()
    runtime = args.runtime.resolve(strict=True)
    selector = args.filter
    if args.provider_recovery:
        if not args.desktop or selector:
            parser.error("--provider-recovery requires --desktop and selects its own fixture")
        selector = "test(=account_launch::agent_process::native_tests::cold_settings::recap_socket::installed_recap_public_socket_preserves_parent_and_exact_request_identity)"
    if not selector:
        parser.error("Specify --filter or --provider-recovery")
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
        if args.lock:
            command = ["cargo", "+stable", "run", "--locked", "--quiet", "-p", "decodex-components", "--", "import-dev", str(args.lock.resolve(strict=True)), str(runtime), "--store", str(fixture / "components")]
        else:
            command = ["cargo", "+stable", "make", "prepare-codex"]
        result = subprocess.run(command, cwd=ROOT, env=preparation, check=False)
        if result.returncode:
            return result.returncode
        if args.desktop:
            environment["DECODEX_TEST_DESKTOP_APP"] = str(args.desktop.resolve(strict=True))
        if args.provider_recovery:
            environment["DECODEX_TEST_PROVIDER_RECOVERY"] = "1"
            print("Isolated provider rehearsal: quit the rehearsal app to finish and remove its temporary profile.", flush=True)
        return subprocess.run(
            [
                "cargo", "+stable", "nextest", "run", "--locked", "-p", "decodex-runtime",
                "--lib", "--all-features", "--run-ignored", "only", "-E", selector,
                "--test-threads", "1",
            ],
            cwd=ROOT,
            env=environment,
            check=False,
        ).returncode


if __name__ == "__main__":
    raise SystemExit(main())
