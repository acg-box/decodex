"""Exercise owned executor cleanup without provider or account access."""

import asyncio
import importlib.util
from pathlib import Path
import signal
import sys
import unittest

SPEC = importlib.util.spec_from_file_location(
    "codex_exec_policy_probe",
    Path(__file__).resolve().parents[2] / "scripts/vnext/codex_exec_policy_probe.py",
)
PROBE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROBE)


class ExecutorCleanupTests(unittest.IsolatedAsyncioTestCase):
    async def test_reaps_executor_that_ignores_eof_and_sigterm(self) -> None:
        process = await asyncio.create_subprocess_exec(
            sys.executable, "-c",
            "import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); "
            "print('ready',flush=True); time.sleep(60)",
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.DEVNULL,
        )
        try:
            self.assertEqual(await asyncio.wait_for(process.stdout.readline(), 5), b"ready\n")
            await asyncio.wait_for(PROBE.shutdown_executor(process), 20)
            self.assertEqual(process.returncode, -signal.SIGKILL)
        finally:
            if process.returncode is None:
                process.kill()
            await process.wait()

    async def test_eof_exit_is_reaped_without_termination(self) -> None:
        process = await asyncio.create_subprocess_exec(
            sys.executable, "-c", "import sys; sys.stdin.read()",
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.DEVNULL,
            stderr=asyncio.subprocess.DEVNULL,
        )
        try:
            await asyncio.wait_for(PROBE.shutdown_executor(process), 10)
            self.assertEqual(process.returncode, 0)
        finally:
            if process.returncode is None:
                process.kill()
            await process.wait()


if __name__ == "__main__":
    unittest.main()
