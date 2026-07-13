import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout


ROOT = Path(__file__).resolve().parents[2]
PROBE_PATH = ROOT / "scripts/vnext/codex_app_server_probe.py"
QUOTA_PATH = ROOT / "openwiki/evidence/fixtures/xy-1262-quota-matrix.json"


def load_probe():
    spec = importlib.util.spec_from_file_location("codex_app_server_probe", PROBE_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


class CodexAppServerProbeTests(unittest.TestCase):
    def test_account_selection_does_not_emit_credentials(self):
        probe = load_probe()
        secret = "header.payload.signature"
        account_id = "account-private-id"
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "accounts.jsonl"
            path.write_text(
                json.dumps(
                    {
                        "email": "account-a@example.invalid",
                        "tokens": {
                            "access_token": secret,
                            "account_id": account_id,
                            "plan_type": "test",
                        },
                    }
                )
                + "\n"
            )
            stdout = io.StringIO()
            stderr = io.StringIO()
            with redirect_stdout(stdout), redirect_stderr(stderr):
                selected = probe.account_login(
                    probe.read_accounts(path), "account-a@example.invalid"
                )

        self.assertEqual(selected["access_token"], secret)
        self.assertEqual(selected["account_id"], account_id)
        self.assertNotIn(secret, stdout.getvalue() + stderr.getvalue())
        self.assertNotIn(account_id, stdout.getvalue() + stderr.getvalue())

    def test_quota_matrix_is_duration_typed_and_covers_required_states(self):
        probe = load_probe()
        fixture = json.loads(QUOTA_PATH.read_text())
        cases = {case["id"]: case for case in fixture["cases"]}

        self.assertEqual(
            fixture["window_identity"],
            {
                "five_hour": {"duration_minutes": 300},
                "seven_day": {"duration_minutes": 10080},
            },
        )
        self.assertFalse(fixture["rules"]["positional_fields_are_identity"])
        self.assertLessEqual(
            {
                "available_both_windows",
                "depleted_five_hour",
                "depleted_seven_day",
                "unknown_missing_five_hour",
                "stale_observation",
                "reversed_positional_fields",
                "depletion_reset_elapsed",
                "auth_failed",
                "all_accounts_depleted",
            },
            cases.keys(),
        )

        reversed_case = cases["reversed_positional_fields"]
        self.assertEqual(
            reversed_case["expect"]["classified"],
            {"five_hour_source": "secondary", "seven_day_source": "primary"},
        )
        self.assertEqual(reversed_case["expect"]["ready_at"], 9000)

        all_depleted = cases["all_accounts_depleted"]["expect"]
        self.assertEqual(all_depleted["account_ready_at"], {"A": 9000, "B": 3000})
        self.assertEqual(all_depleted["earliest_ready_at"], 3000)
        for case in fixture["cases"]:
            actual = probe.classify_quota_case(case)
            for key, value in case["expect"].items():
                self.assertEqual(actual.get(key), value, case["id"])

    def test_checked_receipts_are_redacted_and_keep_failed_verdict(self):
        probe = load_probe()

        result = probe.validate_checked_receipts(ROOT)

        self.assertTrue(result["live_bundle_redacted"])
        self.assertEqual(result["quota_cases_validated"], 9)
        self.assertEqual(result["inventory_accounts_validated"], 6)
        self.assertFalse(result["overall_acceptance"])

    def test_missing_selector_error_does_not_repeat_identity(self):
        probe = load_probe()
        identity = "private-account@example.invalid"

        with self.assertRaises(probe.ProtocolError) as raised:
            probe.account_login([], identity)

        self.assertNotIn(identity, str(raised.exception))

    def test_schema_digest_ignores_json_object_order(self):
        probe = load_probe()
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first.json"
            second = Path(directory) / "second.json"
            first.write_text('{"b":2,"a":{"d":4,"c":3}}')
            second.write_text('{"a":{"c":3,"d":4},"b":2}')

            self.assertEqual(
                probe.sha256_canonical_json(first), probe.sha256_canonical_json(second)
            )

    def test_tree_digest_can_exclude_transient_runtime_root(self):
        probe = load_probe()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stable = root / "cache" / "plugin.json"
            transient = root / ".plugin-appserver" / "codex"
            stable.parent.mkdir()
            transient.parent.mkdir()
            stable.write_text("stable")
            transient.write_text("first")
            before = probe.sha256_tree(root, (".plugin-appserver",))
            transient.write_text("second")

            self.assertEqual(
                probe.sha256_tree(root, (".plugin-appserver",)), before
            )

    def test_thread_summary_keeps_only_normalization_fields(self):
        probe = load_probe()
        summary = probe.summarize_thread(
            {
                "id": "thread-1",
                "name": "owned",
                "cwd": "/repo",
                "ephemeral": False,
                "source": "vscode",
                "threadSource": "decodex.xy1262.probe",
                "historyMode": "legacy",
                "parentThreadId": None,
                "turns": [
                    {
                        "items": [
                            {"type": "userMessage", "text": "private input"},
                            {"type": "agentMessage", "text": "private output"},
                        ]
                    }
                ],
            }
        )

        self.assertEqual(summary["turn_item_types"], ["agentMessage", "userMessage"])
        self.assertEqual(summary["turn_count"], 1)
        self.assertNotIn("private input", json.dumps(summary))
        self.assertNotIn("private output", json.dumps(summary))

    def test_quota_state_requires_both_duration_typed_windows(self):
        probe = load_probe()

        self.assertEqual(
            probe.quota_state(
                [
                    {"duration_minutes": 300, "used_percent": 10, "resets_at": 2000},
                    {"duration_minutes": 10080, "used_percent": 90, "resets_at": 9000},
                ],
                1000,
            ),
            "available",
        )
        self.assertEqual(
            probe.quota_state(
                [
                    {"duration_minutes": 300, "used_percent": 100, "resets_at": 2000},
                    {"duration_minutes": 10080, "used_percent": 90, "resets_at": 9000},
                ],
                1000,
            ),
            "depleted",
        )
        self.assertEqual(
            probe.quota_state(
                [{"duration_minutes": 10080, "used_percent": 0, "resets_at": 9000}],
                1000,
            ),
            "unknown",
        )
        self.assertEqual(
            probe.quota_state(
                [
                    {"duration_minutes": 300, "used_percent": 0, "resets_at": 999},
                    {"duration_minutes": 10080, "used_percent": 0, "resets_at": 9000},
                ],
                1000,
            ),
            "unknown",
        )
        self.assertEqual(
            probe.quota_state(
                [
                    {"duration_minutes": 300, "used_percent": 0, "resets_at": 2000},
                    {"duration_minutes": 10080, "used_percent": 0, "resets_at": 9000},
                    {"duration_minutes": 10080, "used_percent": 100, "resets_at": 8000},
                ],
                1000,
            ),
            "depleted",
        )
        self.assertEqual(
            probe.quota_state(
                [
                    {"duration_minutes": 300, "used_percent": None, "resets_at": 2000},
                    {"duration_minutes": 10080, "used_percent": 0, "resets_at": 9000},
                ],
                1000,
            ),
            "unknown",
        )
        self.assertEqual(
            probe.quota_state(
                [
                    {"duration_minutes": 300, "used_percent": 100, "resets_at": 999},
                    {"duration_minutes": 10080, "used_percent": 0, "resets_at": 9000},
                ],
                1000,
            ),
            "unknown",
        )

    def test_redaction_rejects_identity_selector_and_credential_keys(self):
        probe = load_probe()

        for key in (
            "account_id",
            "account_identity",
            "account_selector",
            "credential_value",
        ):
            with self.subTest(key=key), self.assertRaises(probe.ProtocolError):
                probe.check_redaction({key: "opaque-private-value"})
        probe.check_redaction(
            {
                "identities_emitted": False,
                "selectors_emitted": False,
                "credentials_emitted": False,
            }
        )

    def test_inventory_validation_requires_explicit_integrity_facts(self):
        probe = load_probe()
        names = (
            "xy-1262-live-receipt.json",
            "xy-1262-native-collaboration.json",
            "xy-1262-quota-matrix.json",
            "xy-1262-gate-reconciliation.json",
        )
        with tempfile.TemporaryDirectory() as directory:
            fixture_dir = Path(directory) / "openwiki/evidence/fixtures"
            fixture_dir.mkdir(parents=True)
            source_dir = ROOT / "openwiki/evidence/fixtures"
            for name in names:
                (fixture_dir / name).write_text((source_dir / name).read_text())
            path = fixture_dir / "xy-1262-gate-reconciliation.json"
            receipt = json.loads(path.read_text())
            receipt["normal_state_unchanged"] = {}
            path.write_text(json.dumps(receipt))

            with self.assertRaises(probe.ProtocolError):
                probe.validate_checked_receipts(Path(directory))

    def test_generic_resume_error_is_not_labeled_denied(self):
        probe = load_probe()

        self.assertEqual(
            probe.classify_resume_error(probe.ProtocolError("transport timeout")),
            "probe_error",
        )


if __name__ == "__main__":
    unittest.main()
