from __future__ import annotations

import importlib.util
import io
import ssl
import unittest
import urllib.error
from pathlib import Path
from unittest import mock

MODULE_PATH = Path(__file__).resolve().with_name("build_release_delta.py")
MODULE_SPEC = importlib.util.spec_from_file_location("build_release_delta", MODULE_PATH)
if MODULE_SPEC is None or MODULE_SPEC.loader is None:
    raise RuntimeError(f"Unable to load {MODULE_PATH}")
build_release_delta = importlib.util.module_from_spec(MODULE_SPEC)
MODULE_SPEC.loader.exec_module(build_release_delta)


class FakeResponse(io.StringIO):
    def __init__(self, body: str):
        super().__init__(body)
        self.headers: dict[str, str] = {}

    def __enter__(self) -> "FakeResponse":
        return self

    def __exit__(self, exc_type, exc, tb) -> bool:
        self.close()
        return False


class GithubRequestTests(unittest.TestCase):
    def test_retries_transient_ssl_eof(self) -> None:
        attempts: list[object] = [
            urllib.error.URLError(ssl.SSLEOFError("EOF occurred in violation of protocol")),
            FakeResponse('{"ok": true}'),
        ]

        def fake_urlopen(_request):
            result = attempts.pop(0)
            if isinstance(result, Exception):
                raise result
            return result

        with (
            mock.patch.object(
                build_release_delta.urllib.request,
                "urlopen",
                side_effect=fake_urlopen,
            ) as urlopen_mock,
            mock.patch.object(build_release_delta.time, "sleep") as sleep_mock,
        ):
            payload = build_release_delta.github_request("https://api.github.com/repos/openai/codex/releases", "token")

        self.assertEqual(payload, {"ok": True})
        self.assertEqual(urlopen_mock.call_count, 2)
        sleep_mock.assert_called_once_with(build_release_delta.GITHUB_REQUEST_BACKOFF_SECONDS)

    def test_non_retryable_url_error_still_fails(self) -> None:
        with (
            mock.patch.object(
                build_release_delta.urllib.request,
                "urlopen",
                side_effect=urllib.error.URLError("name resolution failed"),
            ),
            mock.patch.object(build_release_delta.time, "sleep") as sleep_mock,
        ):
            with self.assertRaises(SystemExit) as exc_info:
                build_release_delta.github_request("https://api.github.com/repos/openai/codex/releases", "token")

        self.assertIn("name resolution failed", str(exc_info.exception))
        sleep_mock.assert_not_called()


if __name__ == "__main__":
    unittest.main()
