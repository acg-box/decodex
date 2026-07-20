from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import unittest


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
