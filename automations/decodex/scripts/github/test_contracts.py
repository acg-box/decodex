#!/usr/bin/env python3
"""Tests for GitHub automation contract validators."""

from __future__ import annotations

import copy
import unittest

from contracts import validate_social_post


def valid_social_post() -> dict:
    return {
        "schema": "social_post/v1",
        "slug": "openai-codex-pr-22414",
        "channel": "x",
        "target_account": "decodexspace",
        "controller_account": "hackink",
        "mode": "operator_impact",
        "status": "published",
        "audience": "Codex operators",
        "text": [
            "Remote Codex can now use Unix socket endpoints. Source: https://github.com/openai/codex/pull/22414"
        ],
        "source_refs": {
            "urls": ["https://github.com/openai/codex/pull/22414"],
        },
        "evidence_notes": ["PR #22414 changes remote endpoint handling."],
        "claims": [
            {
                "text": "Remote Codex can use Unix socket endpoints.",
                "evidence": "https://github.com/openai/codex/pull/22414",
                "confidence": "confirmed",
            }
        ],
        "decision": {
            "worthiness": "publish",
            "priority": "high",
            "idempotency_key": "x:decodexspace:operator_impact:openai-codex-pr-22414",
            "reason": "High-value Control Plane transport implication.",
            "daily_limit": 8,
            "daily_count_before": 2,
            "daily_count_after": 3,
            "day": "2026-06-02",
            "timezone": "Asia/Shanghai",
        },
        "publication": {
            "posted_at": "2026-06-02T03:00:00Z",
            "published_urls": ["https://x.com/decodexspace/status/1"],
            "publisher": "chrome",
            "account_verified": True,
            "made_with_ai": True,
        },
        "media_refs": ["https://x.com/decodexspace/status/1/photo/1"],
    }


class SocialPostContractTests(unittest.TestCase):
    def assert_text_rejected(self, text: str, expected_error: str) -> None:
        entry = copy.deepcopy(valid_social_post())
        entry["text"] = [text]

        result = validate_social_post(entry)

        self.assertFalse(result.ok)
        self.assertTrue(
            any(expected_error in error for error in result.errors),
            f"expected {expected_error!r} in {result.errors!r}",
        )

    def test_social_post_rejects_automation_attribution(self) -> None:
        self.assert_text_rejected(
            "Automated by @hackink: tracking this.",
            "must not include automation attribution",
        )

    def test_social_post_rejects_overpacked_text_without_source_url(self) -> None:
        self.assert_text_rejected(
            "Codex checkpoint " * 18,
            "longer than 260 characters",
        )

    def test_social_post_rejects_generic_copy_without_crashing(self) -> None:
        self.assert_text_rejected(
            "Watching this.",
            "must name a concrete source-backed",
        )


if __name__ == "__main__":
    unittest.main()
