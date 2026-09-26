#!/usr/bin/env python3
"""Qualify installed executor policy cwd handling with isolated, synthetic files."""

import argparse
import asyncio
import base64
import json
import os
from pathlib import Path
import subprocess
import tempfile


async def qualify_read_write(call, sandbox: dict, source: Path, root: Path) -> int:
    """Verify full-disk reads do not grant mutation rights on the installed executor."""
    policy = json.loads(json.dumps(sandbox))
    policy["permissions"]["file_system"]["entries"] = [
        {"path": {"type": "special", "value": {"kind": "root"}}, "access": "read"}
    ]
    read = await call("fs/readFile", {"path": source.as_uri(), "sandbox": policy})
    assert base64.b64decode(read["result"]["dataBase64"]) == b"allowed fixture", read
    destination = root / "must-not-exist"
    for method, params in [
        ("fs/writeFile", {"path": source.as_uri(), "dataBase64": base64.b64encode(b"changed").decode()}),
        ("fs/createDirectory", {"path": destination.as_uri()}),
        ("fs/remove", {"path": source.as_uri()}),
        ("fs/copy", {"sourcePath": source.as_uri(), "destinationPath": destination.as_uri(), "recursive": False}),
    ]:
        result = await call(method, {**params, "sandbox": policy})
        assert "error" in result, (method, "read-only mutation succeeded")
        message = result["error"]["message"].lower()
        assert "permission denied" in message or "operation not permitted" in message, (method, result)
        assert source.read_bytes() == b"allowed fixture"
        assert not destination.exists()
    return 5


async def qualify(binary: Path) -> None:
    """Read allowed/denied fixtures after removing only the empty policy directory."""
    with tempfile.TemporaryDirectory(prefix="decodex-policy-cwd-") as directory:
        root = Path(directory).resolve()
        selected = root / "selected"
        selected.mkdir()
        allowed = root / "allowed.txt"
        denied = root / "denied.txt"
        allowed.write_text("allowed fixture")
        denied.write_text("denied fixture")
        with (root / "stderr.log").open("wb") as stderr:
            process = await asyncio.create_subprocess_exec(
                str(binary), "exec-server", "--listen", "stdio",
                cwd=root,
                env={"HOME": str(root), "CODEX_HOME": str(root), "PATH": "/usr/bin:/bin"},
                stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE, stderr=stderr,
            )
            sequence = 0

            async def call(method: str, params: dict) -> dict:
                nonlocal sequence
                sequence += 1
                process.stdin.write((json.dumps({"jsonrpc": "2.0", "id": sequence,
                                                "method": method, "params": params}) + "\n").encode())
                await process.stdin.drain()
                while True:
                    line = await asyncio.wait_for(process.stdout.readline(), 10)
                    if not line:
                        raise RuntimeError("executor closed before response")
                    result = json.loads(line)
                    if result.get("id") == sequence:
                        return result

            try:
                initialized = await call("initialize", {"clientName": "decodex-policy-probe"})
                assert "result" in initialized, initialized
                process.stdin.write(b'{"jsonrpc":"2.0","method":"initialized","params":{}}\n')
                await process.stdin.drain()
                selected.rmdir()
                sandbox = {
                    "permissions": {"type": "managed", "file_system": {
                        "type": "restricted", "entries": [
                            {"path": {"type": "special", "value": {"kind": "root"}}, "access": "read"},
                            {"path": {"type": "path", "path": denied.as_uri()}, "access": "deny"},
                        ]}, "network": "restricted"},
                    "policyContext": {"cwd": selected.as_uri(), "workspaceRoots": []},
                    "windowsSandboxLevel": "disabled",
                }
                for legacy in (False, True):
                    policy = json.loads(json.dumps(sandbox))
                    if legacy:
                        policy.pop("policyContext")
                    result = await call("fs/readFile", {"path": allowed.as_uri(), "sandbox": policy})
                    assert "result" in result, result
                    assert base64.b64decode(result["result"]["dataBase64"]) == b"allowed fixture", result
                    result = await call("fs/readFile", {"path": denied.as_uri(), "sandbox": policy})
                    assert "error" in result, "explicit denial was bypassed"
                    message = result["error"]["message"].lower()
                    assert "permission denied" in message or "operation not permitted" in message, result
                dynamic = json.loads(json.dumps(sandbox))
                dynamic.pop("policyContext")
                dynamic["permissions"]["file_system"]["entries"][1]["path"] = {
                    "type": "glob_pattern", "pattern": "secret*"}
                result = await call("fs/readFile", {"path": allowed.as_uri(), "sandbox": dynamic})
                # Installed alpha.16 uses InvalidRequest; the fixed source uses InvalidParams.
                assert result["error"]["code"] in (-32600, -32602), result
                assert "requires cwd" in result["error"]["message"], result
                read_write_checks = await qualify_read_write(call, sandbox, allowed, root)
                assert not selected.exists()
                assert denied.read_text() == "denied fixture"
                version = initialized["result"].get("environmentInfo", {}).get("executorVersion", "unknown")
                cli_version = subprocess.check_output([str(binary), "--version"], text=True, timeout=5).strip()
                print(json.dumps({"cliVersion": cli_version, "executorVersion": version, "checks": 5 + read_write_checks, "result": "passed",
                                  "missingCwdCode": result["error"]["code"]}))
            finally:
                process.stdin.close()
                try:
                    await asyncio.wait_for(process.wait(), 5)
                except TimeoutError:
                    process.terminate()
                    await process.wait()


def main() -> None:
    """Require an explicit installed binary; do not use account credentials."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    if not args.binary.is_absolute() or not os.access(args.binary, os.X_OK):
        parser.error("--binary must be an absolute executable path")
    asyncio.run(asyncio.wait_for(qualify(args.binary), 60))


if __name__ == "__main__":
    main()
