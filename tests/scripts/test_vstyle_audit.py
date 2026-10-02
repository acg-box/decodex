"""Focused tests for the attested zero-finding Rust style boundary."""

from contextlib import redirect_stdout, redirect_stderr
from copy import deepcopy
from io import StringIO
import json
from pathlib import Path
from subprocess import CompletedProcess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch


REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

import vstyle_audit  # noqa: E402


RULE = "RUST-STYLE-SPACE-003"
MESSAGE = "Insert exactly one blank line between different statement types."
OUTPUT = f"""src/lib.rs:10:1: [{RULE}] {MESSAGE} (fixable)

Checked 1 file(s).

Found 1 style violation(s).
"""


class VstyleAuditTests(unittest.TestCase):
    """Require attested coverage and reject every finding without exemptions."""

    def setUp(self) -> None:
        self.contract = vstyle_audit.load_contract()
        self.contract["minimum_checked_files"] = 1
        self.contract["rust_rules"] = [RULE]

    def audit_result(self, output, returncode=0):
        tool = self.contract["tool"]
        version = f"vibe-style {tool['version']}-{tool['git_short']}-test-host"
        responses = [
            CompletedProcess([], 0, "host: test-host\n", ""),
            CompletedProcess([], 0, version, ""),
            CompletedProcess([], 0, f"{RULE}\timplemented\n", ""),
            CompletedProcess([], returncode, output, ""),
        ]
        with (
            patch.object(vstyle_audit, "load_contract", return_value=self.contract),
            patch.object(vstyle_audit, "run", side_effect=responses) as run,
            redirect_stdout(StringIO()),
        ):
            result = vstyle_audit.audit()
        self.assertEqual(
            run.call_args_list[-1].args[0],
            ["cargo", "vstyle", "curate", "--language", "rust", "--workspace", "--all-features"],
        )
        return result

    def test_zero_findings_accepts_current_and_explicit_zero_summaries(self):
        for output in ["Checked 1 file(s).\n", "Checked 1 file(s).\nFound 0 style violation(s).\n"]:
            with self.subTest(output=output):
                self.assertEqual(self.audit_result(output), 0)

    def test_any_finding_fails_without_a_baseline(self):
        self.assertEqual(self.audit_result(OUTPUT, 1), 1)
        manual = OUTPUT.replace(" (fixable)", "").replace(
            "Found 1", "1 violation(s) require manual fixes.\nFound 1"
        )
        self.assertEqual(self.audit_result(manual, 1), 1)

    def test_scope_shrink_and_inconsistent_exit_fail_closed(self):
        self.contract["minimum_checked_files"] = 2
        with self.assertRaises(vstyle_audit.AuditError):
            self.audit_result("Checked 1 file(s).\n")
        self.contract["minimum_checked_files"] = 1
        for output, status in [(OUTPUT, 0), ("Checked 1 file(s).\n", 1)]:
            with self.subTest(status=status):
                with self.assertRaises(vstyle_audit.AuditError):
                    self.audit_result(output, status)

    def test_version_and_rule_mismatches_fail_closed(self):
        with self.assertRaises(vstyle_audit.AuditError):
            vstyle_audit.validate_version("vibe-style wrong", self.contract, "test-host")
        with self.assertRaises(vstyle_audit.AuditError):
            vstyle_audit.validate_rules([RULE, "RUST-STYLE-NEW-001"], self.contract)
        with self.assertRaises(vstyle_audit.AuditError):
            vstyle_audit.parse_coverage(f"{RULE}\timplemented\n{RULE}\timplemented\n")

    def test_contract_rejects_legacy_exemptions_and_invalid_scope(self):
        for field, value in [
            ("schema", "decodex/vstyle-rust-audit/1"),
            ("baseline", []),
            ("accepted_baseline", {"findings": 1}),
            ("governance", {"review_by": "2099-01-01"}),
            ("minimum_checked_files", 0),
            ("minimum_checked_files", True),
        ]:
            with self.subTest(field=field, value=value), TemporaryDirectory() as directory:
                contract = deepcopy(self.contract)
                contract[field] = value
                path = Path(directory) / "contract.json"
                path.write_text(json.dumps(contract), encoding="utf-8")
                with self.assertRaises(vstyle_audit.AuditError):
                    vstyle_audit.load_contract(path)

    def test_finding_identity_and_counts_remain_location_independent(self):
        findings, summary = vstyle_audit.parse_curate(OUTPUT, {RULE})
        shifted, _ = vstyle_audit.parse_curate(OUTPUT.replace(":10:1", ":99:7"), {RULE})
        self.assertEqual(findings, shifted)
        self.assertEqual(summary, {"checked_files": 1, "total": 1, "manual": 0})

    def test_malformed_or_unattested_output_is_rejected(self):
        for output in [
            "warning: changed output contract", "", OUTPUT.replace("Found 1", "Found 2"),
            OUTPUT.replace("Found 1 style violation(s).", ""),
            OUTPUT.replace("src/lib.rs", "../lib.rs"),
            OUTPUT.replace(RULE, "RUST-STYLE-NEW-001"),
        ]:
            with self.subTest(output=output):
                with self.assertRaises(vstyle_audit.AuditError):
                    vstyle_audit.parse_curate(output, {RULE})

    def test_missing_tool_is_setup_failure(self):
        with patch.object(vstyle_audit, "run", side_effect=FileNotFoundError("cargo")):
            with redirect_stderr(StringIO()):
                self.assertEqual(vstyle_audit.main(), 2)


if __name__ == "__main__":
    unittest.main()
