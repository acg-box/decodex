"""Verify native diagnostic storage routing without an account or provider call."""

import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path

from native_session import B, Session


def check_storage(case):
    with tempfile.TemporaryDirectory(prefix="dx-log-storage-", dir="/tmp") as home:
        root = Path(home)
        logs = root / "diagnostics"
        state = root / "state"
        config = 'cli_auth_credentials_store = "file"\n[analytics]\nenabled = false\n'
        environment = {}
        if case != "default":
            config = f"log_dir = {json.dumps(str(logs))}\n" + config
        if case == "environment_state":
            environment["CODEX_SQLITE_HOME"] = str(state)
        (root / "config.toml").write_text(config, encoding="utf-8")
        session = Session(home, environment=environment)
        try:
            session.call("config/read", {"includeLayers": False, "cwd": home})
            account = session.call("account/read", {"refreshToken": False})
            assert account["account"] is None, "Fixture must remain unauthenticated"
            expected = root if case == "default" else logs
            assert (expected / "logs_2.sqlite").is_file(), f"Wrong log database location: {case}"
            if expected != root:
                assert not (root / "logs_2.sqlite").exists(), "Created an unintended legacy log database"
            if case == "environment_state":
                assert list(state.glob("state_*.sqlite")), "Environment-selected state directory was ignored"
        finally:
            session.close()
    return {"case": case, "passed": True}


def check_login():
    with tempfile.TemporaryDirectory(prefix="dx-log-login-", dir="/tmp") as home:
        root = Path(home)
        blocked = root / "unavailable"
        blocked.write_text("not a directory", encoding="utf-8")
        (root / "config.toml").write_text(
            f"log_dir = {json.dumps(str(blocked))}\ncli_auth_credentials_store = \"file\"\n",
            encoding="utf-8",
        )
        result = subprocess.run(
            [str(B), "login", "status"],
            env={"HOME": home, "CODEX_HOME": home, "PATH": "/usr/bin:/bin:/usr/sbin:/sbin"},
            cwd=home,
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
        assert result.returncode == 1 and "Not logged in" in result.stderr, result.stderr
        assert blocked.read_text(encoding="utf-8") == "not a directory"
    return {"case": "unavailable_logs_login_status", "passed": True}


if __name__ == "__main__":
    cases = [check_storage(case) for case in ("default", "configured", "environment_state")]
    cases.append(check_login())
    receipt = {
        "source_commit": os.environ["QUALIFIED_CODEX_SOURCE_COMMIT"],
        "binary_sha256": hashlib.sha256(B.read_bytes()).hexdigest(),
        "cases": cases,
        "passed": len(cases),
        "failed": 0,
        "limits": "Local startup, configuration and unauthenticated account read only; managed storage precedence is covered by source tests. No NFS, migration, remote account or installed-app acceptance.",
    }
    output = Path(os.environ["DECODEX_QUALIFICATION_OUTPUT"])
    (output / "log_storage.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"passed": len(cases), "failed": 0}))
