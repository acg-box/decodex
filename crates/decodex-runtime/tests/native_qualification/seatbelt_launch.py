"""Check local command permissions against the prepared macOS Codex CLI."""

import json
import os
import tempfile
from pathlib import Path

from native_session import Session


def main():
    with tempfile.TemporaryDirectory(prefix="dx-seatbelt-", dir="/tmp") as directory:
        root = Path(directory).resolve()
        project = root / "project"
        (project / "granted").mkdir(parents=True)
        home = root / "codex-home"
        home.mkdir()
        (home / "config.toml").write_text(
            '[permissions.command-cwd.filesystem]\n":root" = "read"\n'
            '[permissions.command-cwd.filesystem.":workspace_roots"]\n"granted" = "write"\n'
        )
        marker = project / "granted" / "qualification-marker.txt"
        command = {
            "command": ["/bin/sh", "-c", "printf qualified > granted/qualification-marker.txt"],
            "cwd": str(project),
        }
        session = Session(str(home))
        try:
            denied = session.call("command/exec", command | {
                "sandboxPolicy": {"type": "readOnly", "networkAccess": False},
            })
            assert denied["exitCode"] != 0 and not marker.exists(), denied
            permitted = session.call("command/exec", command | {
                "permissionProfile": "command-cwd",
            })
            assert permitted["exitCode"] == 0, permitted
            assert marker.read_text() == "qualified"
        finally:
            session.close()

    print(json.dumps({
        "status": "passed",
        "cases": ["read-only refuses writing", "named command-cwd subtree permits writing"],
        "source_commit": os.environ["QUALIFIED_CODEX_SOURCE_COMMIT"],
    }))


if __name__ == "__main__":
    main()
