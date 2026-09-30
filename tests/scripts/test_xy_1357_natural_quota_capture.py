from __future__ import annotations

import importlib.util
import json
import io
import os
import threading
import time
from types import SimpleNamespace
from pathlib import Path
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
CAPTURE_PATH = ROOT / "scripts/vnext/xy_1357_natural_quota_capture.py"
RECEIPT_PATH = (
    ROOT / "openwiki/evidence/fixtures/xy-1357-natural-quota-receipt.json"
)
SPEC = importlib.util.spec_from_file_location("xy_1357_natural_quota_capture", CAPTURE_PATH)
assert SPEC and SPEC.loader
CAPTURE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CAPTURE)


class NaturalQuotaCaptureTests(unittest.TestCase):
    def test_decoder_preserves_integer_decimal_and_exponent_lexemes(self) -> None:
        value = CAPTURE.decode_json_frame(
            b'{"integer":1780000000,"decimal":1780000000.123456,"exponent":1.78e9}'
        )

        self.assertIsInstance(value["integer"], CAPTURE.JsonNumberToken)
        self.assertEqual(value["integer"], "1780000000")
        self.assertEqual(value["decimal"], "1780000000.123456")
        self.assertEqual(value["exponent"], "1.78e9")

    def test_exact_conversion_never_rounds_or_truncates(self) -> None:
        exact = CAPTURE.convert_timestamp_token(
            CAPTURE.JsonNumberToken("1780000000.123456")
        )
        incompatible = CAPTURE.convert_timestamp_token(
            CAPTURE.JsonNumberToken("1780000000.1234567")
        )

        self.assertEqual(exact["status"], "exact")
        self.assertEqual(exact["utc_unix_microseconds"], 1_780_000_000_123_456)
        self.assertEqual(exact["exact_arithmetic"]["division_remainder"], 0)
        self.assertEqual(incompatible["status"], "precision_incompatible")
        self.assertEqual(incompatible["reason"], "would_round_or_truncate")
        self.assertNotEqual(incompatible["exact_arithmetic"]["division_remainder"], 0)

    def test_large_numeric_tokens_fail_closed_before_integer_conversion(self) -> None:
        for token in ("1e" + "9" * 5000, "1e-" + "9" * 5000, "9" * 5000):
            with self.subTest(prefix=token[:8]):
                result = CAPTURE.convert_timestamp_token(CAPTURE.JsonNumberToken(token))
                self.assertEqual(result["reason"], "unsupported_magnitude")
        zero_padded = CAPTURE.convert_timestamp_token(
            CAPTURE.JsonNumberToken("1e" + "0" * 5000 + "1")
        )
        self.assertEqual(zero_padded["utc_unix_microseconds"], 10_000_000)
        with self.assertRaisesRegex(CAPTURE.CaptureError, "invalid_window_duration"):
            CAPTURE.exact_integer(CAPTURE.JsonNumberToken("9" * 5000), "invalid_window_duration")

    def test_partial_frame_obeys_response_deadline(self) -> None:
        read_fd, write_fd = os.pipe()
        with os.fdopen(read_fd, "rb", buffering=0) as reader:
            os.write(write_fd, b"{")
            close = threading.Timer(0.3, os.close, args=(write_fd,))
            close.start()
            try:
                session = CAPTURE.AppServerSession(SimpleNamespace(stdout=reader), 0.05)
                with self.assertRaisesRegex(CAPTURE.CaptureError, "app_server_response_timeout"):
                    session.receive(time.monotonic() + 0.05)
            finally:
                close.join()

    def test_coalesced_frames_remain_available_without_more_pipe_data(self) -> None:
        read_fd, write_fd = os.pipe()
        with os.fdopen(read_fd, "rb", buffering=0) as reader:
            try:
                os.write(write_fd, b'{"id":1}\n{"id":2}\n')
                session = CAPTURE.AppServerSession(SimpleNamespace(stdout=reader), 0.1)
                self.assertEqual(session.receive(time.monotonic() + 0.1)["id"], "1")
                self.assertEqual(session.receive(time.monotonic() + 0.1)["id"], "2")
            finally:
                os.close(write_fd)

    def test_frame_limit_and_truncated_eof_remain_closed(self) -> None:
        for payload, limit, expected in (
            (b'{"id":1}\n', 9, None),
            (b'{"id":1}\n', 8, "app_server_frame_limit"),
            (b'{"id":', 64, "app_server_frame_limit"),
            (b'', 64, "app_server_exited"),
        ):
            with self.subTest(payload=payload, limit=limit):
                read_fd, write_fd = os.pipe()
                os.write(write_fd, payload)
                os.close(write_fd)
                with os.fdopen(read_fd, "rb", buffering=0) as reader:
                    session = CAPTURE.AppServerSession(SimpleNamespace(stdout=reader), 1)
                    with mock.patch.object(CAPTURE, "MAX_FRAME_BYTES", limit):
                        if expected:
                            with self.assertRaisesRegex(CAPTURE.CaptureError, expected):
                                session.receive(time.monotonic() + 1)
                        else:
                            self.assertEqual(session.receive(time.monotonic() + 1)["id"], "1")

    def test_capture_failure_receipt_preserves_attempted_requests(self) -> None:
        frames = (
            b'{"id":1,"error":{"code":-1}}\n',
            b'{"id":1,"result":{"userAgent":"fixture"}}\n'
            b'{"id":2,"result":{"rateLimits":{"primary":{"resetsAt":1780000000}}}}\n',
        )
        for index, frame in enumerate(frames):
            with self.subTest(stage=index):
                read_fd, write_fd = os.pipe()
                os.write(write_fd, frame)
                os.close(write_fd)
                with os.fdopen(read_fd, "rb", buffering=0) as reader:
                    process = SimpleNamespace(stdout=reader, stdin=io.BytesIO())
                    with (
                        mock.patch.object(CAPTURE, "resolve_executable", return_value=Path("fixture")),
                        mock.patch.object(CAPTURE, "sha256_file", return_value="0" * 64),
                        mock.patch.object(CAPTURE, "codex_version", return_value="fixture"),
                        mock.patch.object(CAPTURE, "attest_schema", return_value={}),
                        mock.patch.object(CAPTURE, "launch_app_server", return_value=process),
                        mock.patch.object(CAPTURE, "shutdown_process", side_effect=(
                            CAPTURE.CaptureError("app_server_cleanup_failed") if index else None
                        )),
                    ):
                        receipt = CAPTURE.capture("fixture", 1)
                    expected = ["initialize"] if index == 0 else list(CAPTURE.REQUEST_SEQUENCE)
                    self.assertEqual(receipt["capture"]["request_sequence"], expected)
                    self.assertEqual(receipt["capture"]["rate_limit_read_count"], index)
                    self.assertEqual(receipt["verdict"], "insufficient_evidence")
                    self.assertEqual(receipt["failure"]["code"],
                                     "initialize_rejected" if index == 0 else "app_server_cleanup_failed")

    def test_extraction_retains_only_allowlisted_opaque_evidence(self) -> None:
        result = CAPTURE.decode_json_frame(
            b'{"rateLimits":{"limitId":"private-limit-id","primary":'
            b'{"usedPercent":1,"windowDurationMins":300,"resetsAt":1780000000}},'
            b'"rateLimitsByLimitId":{"private-limit-id":{"secondary":'
            b'{"usedPercent":2,"windowDurationMins":10080,"resetsAt":1781000000}}},'
            b'"unrelated":{"email":"redacted-marker","accessToken":"redacted-marker"}}'
        )
        observations, limitations = CAPTURE.extract_observations(result)
        receipt = {
            "observations": observations,
            "limitations": limitations,
            "account_alias": "ambient-account-1",
        }
        encoded = json.dumps(receipt, sort_keys=True)

        self.assertEqual(len(observations), 2)
        self.assertEqual(limitations, [])
        self.assertNotIn("private-limit-id", encoded)
        self.assertNotIn("redacted-marker", encoded)
        self.assertEqual(
            CAPTURE.receipt_verdict(observations, limitations),
            "exact_microseconds_compatible",
        )

    def test_surface_contains_one_rate_read_and_no_turn(self) -> None:
        self.assertEqual(CAPTURE.REQUEST_SEQUENCE.count(CAPTURE.RATE_LIMIT_METHOD), 1)
        self.assertNotIn("turn/start", CAPTURE.REQUEST_SEQUENCE)
        self.assertNotIn("account/read", CAPTURE.REQUEST_SEQUENCE)
        self.assertNotIn("account/login/start", CAPTURE.REQUEST_SEQUENCE)

    def test_checked_in_receipt_recomputes_exactly_and_passes_allowlist(self) -> None:
        receipt = json.loads(RECEIPT_PATH.read_text(encoding="utf-8"))

        self.assertEqual(receipt["schema"], CAPTURE.RECEIPT_SCHEMA)
        self.assertEqual(receipt["verdict"], "exact_microseconds_compatible")
        self.assertEqual(receipt["capture"]["rate_limit_read_count"], 1)
        self.assertEqual(receipt["failure"], None)
        for observation in receipt["observations"]:
            recomputed = CAPTURE.convert_timestamp_token(
                CAPTURE.JsonNumberToken(observation["raw_json_token"])
            )
            self.assertEqual(recomputed, observation["conversion"])
        CAPTURE.assert_safe_receipt(receipt)


if __name__ == "__main__":
    unittest.main()
