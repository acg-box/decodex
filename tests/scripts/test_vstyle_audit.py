"""Focused tests for the deterministic vstyle audit boundary."""

from collections import Counter
from datetime import date
from pathlib import Path
import sys
import unittest


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
    """Prove provenance failure and stable baseline-delta behavior."""

    def setUp(self) -> None:
        self.contract = {
            "schema": "decodex/vstyle-rust-audit/1",
            "governance": {"review_by": "2026-08-15"},
            "tool": {"version": "0.2.3", "git_short": "3a0959e"},
            "accepted_baseline": {"checked_files": 1, "findings": 1, "manual": 0},
            "rust_rules": [RULE],
            "baseline": [
                {
                    "path": "src/lib.rs",
                    "rule": RULE,
                    "message": MESSAGE,
                    "fixable": True,
                    "count": 1,
                }
            ],
        }

    def test_version_and_rule_mismatches_fail_closed(self) -> None:
        with self.assertRaises(vstyle_audit.AuditError):
            vstyle_audit.validate_version(
                "vibe-style 0.2.3-wrong-aarch64-apple-darwin",
                self.contract,
                "aarch64-apple-darwin",
            )
        with self.assertRaises(vstyle_audit.AuditError):
            vstyle_audit.validate_rules([RULE, "RUST-STYLE-NEW-001"], self.contract)

    def test_governance_deadline_fails_closed(self) -> None:
        with self.assertRaises(vstyle_audit.AuditError):
            vstyle_audit.validate_governance(self.contract, date(2026, 8, 16))

    def test_exact_baseline_and_location_shift_have_no_delta(self) -> None:
        current, summary = vstyle_audit.parse_curate(OUTPUT, {RULE})
        baseline = vstyle_audit.baseline_counter(self.contract)
        self.assertEqual(summary, {"checked_files": 1, "total": 1, "manual": 0})
        self.assertEqual(vstyle_audit.compare_findings(current, baseline), (Counter(), Counter()))

        shifted = OUTPUT.replace("src/lib.rs:10:1", "src/lib.rs:99:7")
        current, _ = vstyle_audit.parse_curate(shifted, {RULE})
        self.assertEqual(vstyle_audit.compare_findings(current, baseline), (Counter(), Counter()))

    def test_new_regression_and_resolved_finding_are_distinct(self) -> None:
        doubled = OUTPUT.replace(
            "\n\nChecked",
            f"\nsrc/lib.rs:20:1: [{RULE}] {MESSAGE} (fixable)\n\nChecked",
        ).replace("Found 1", "Found 2")
        current, _ = vstyle_audit.parse_curate(doubled, {RULE})
        added, resolved = vstyle_audit.compare_findings(
            current,
            vstyle_audit.baseline_counter(self.contract),
        )
        self.assertEqual(sum(added.values()), 1)
        self.assertEqual(sum(resolved.values()), 0)

        current, _ = vstyle_audit.parse_curate(
            "Checked 1 file(s).\n\nFound 0 style violation(s).\n",
            {RULE},
        )
        added, resolved = vstyle_audit.compare_findings(
            current,
            vstyle_audit.baseline_counter(self.contract),
        )
        self.assertEqual(sum(added.values()), 0)
        self.assertEqual(sum(resolved.values()), 1)

    def test_unstructured_output_is_rejected(self) -> None:
        with self.assertRaises(vstyle_audit.AuditError):
            vstyle_audit.parse_curate("warning: changed output contract", {RULE})


if __name__ == "__main__":
    unittest.main()
