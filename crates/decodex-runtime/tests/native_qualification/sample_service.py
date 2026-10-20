"""Qualify native sampling through the public service with an explicit runtime package."""

import argparse
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, required=True)
    args = parser.parse_args()
    runtime = args.runtime.resolve(strict=True)
    binary = runtime / "CodexCLI.app/Contents/MacOS/codex"
    assert binary.is_file(), "An exact prepared macOS runtime package is required."
    root = Path(__file__).resolve().parents[4]
    env = os.environ.copy()
    host_home = Path.home()
    env.setdefault("CARGO_HOME", str(host_home / ".cargo"))
    env.setdefault("RUSTUP_HOME", str(host_home / ".rustup"))
    for key in (
        "CODEX_HOME", "CODEX_ACCESS_TOKEN", "CODEX_API_KEY", "OPENAI_API_KEY",
        "OPENAI_FEDERATION_RULE_ID", "OPENAI_IDENTITY_TOKEN_FILE",
        "OPENAI_WORKLOAD_IDENTITY_CONTEXT",
    ):
        env.pop(key, None)
    # Keep the Unix socket path below macOS's limit, including runtime suffixes.
    with tempfile.TemporaryDirectory(prefix="dx-sample-", dir="/tmp") as temporary:
        home = Path(temporary).resolve()
        (home / ".decodex-recap-fixture").write_text("isolated-recap\n")
        env.update(
            HOME=str(home),
            DECODEX_TEST_ACCOUNT_HOME=str(home),
            DECODEX_TEST_CODEX_BINARY=str(binary),
            DECODEX_CODEX_DEVELOPMENT_RUNTIME=str(runtime),
            DECODEX_TEST_NATIVE_SAMPLE="1",
        )
        (home / "preparation-codex").mkdir(mode=0o700)
        preparation = env.copy()
        preparation["CODEX_HOME"] = str(home / "preparation-codex")
        subprocess.run(
            ["cargo", "+stable", "make", "prepare-codex"],
            cwd=root, env=preparation, check=True,
        )
        assert not (home / ".codex").exists(), "Keep native fixture setup separate."
        subprocess.run(
            [
                "cargo", "+stable", "nextest", "run", "--locked",
                "-p", "decodex-runtime", "--lib", "--run-ignored", "only",
                "-E", "test(installed_recap_public_socket_preserves_parent_and_exact_request_identity)",
            ],
            cwd=root, env=env, check=True,
        )


if __name__ == "__main__":
    main()
