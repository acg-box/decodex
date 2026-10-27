"""Check that an exact native CLI starts when its .env path is a directory."""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    binary = Path(os.environ["QUALIFIED_CODEX_BINARY"]).resolve()
    results = []
    for directory in (False, True):
        with tempfile.TemporaryDirectory(prefix="dx-dotenv-", dir="/tmp") as home:
            if directory:
                Path(home, ".env").mkdir()
            env = {"HOME": home, "CODEX_HOME": home, "PATH": "/usr/bin:/bin:/usr/sbin:/sbin"}
            try:
                result = subprocess.run(
                    [str(binary), "--version"], cwd=home, env=env,
                    capture_output=True, text=True, timeout=8, check=True,
                )
            except subprocess.TimeoutExpired as error:
                raise AssertionError("Native startup stalled with a directory at .env") from error
            assert result.stdout.startswith("codex-cli "), result.stdout
            results.append({"dotenv_directory": directory, "exit_code": result.returncode})
    receipt = {
        "source_commit": os.environ["QUALIFIED_CODEX_SOURCE_COMMIT"],
        "cli_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "cases": results,
        "limits": "CLI startup only; no provider, model, account, or installed-app acceptance.",
    }
    Path(os.environ["DECODEX_QUALIFICATION_OUTPUT"], "dotenv-startup-native.json").write_text(
        json.dumps(receipt, indent=2) + "\n"
    )
    print(json.dumps({"passed": len(results), "dotenv_directory_startup": "passed"}))


if __name__ == "__main__":
    main()
