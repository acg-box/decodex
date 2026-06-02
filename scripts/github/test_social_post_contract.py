#!/usr/bin/env python3
"""Exercise the Decodex social_post/v1 validator."""

from __future__ import annotations

from contracts import validate_social_post


def base_record() -> dict[str, object]:
    return {
        "schema": "social_post/v1",
        "slug": "openai-codex-pr-22414",
        "channel": "x",
        "target_account": "decodexspace",
        "controller_account": "hackink",
        "mode": "operator_impact",
        "status": "published",
        "audience": "Codex operators",
        "text": ["Remote Codex can now use Unix socket endpoints. Source: https://github.com/openai/codex/pull/22414"],
        "source_refs": {"urls": ["https://github.com/openai/codex/pull/22414"]},
        "evidence_notes": ["PR #22414 changes the remote app-server endpoint handling."],
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
            "image_template": "decodex_signal_card",
        },
        "media_refs": ["artifacts/social/x/images/openai-codex-pr-22414.png"],
    }


def assert_valid(record: dict[str, object]) -> None:
    validation = validate_social_post(record)
    assert validation.ok, validation.errors


def assert_invalid(record: dict[str, object], expected: str) -> None:
    validation = validate_social_post(record)
    assert not validation.ok
    assert any(expected in error for error in validation.errors), validation.errors


def main() -> None:
    published = base_record()
    assert_valid(published)

    blocked = base_record()
    blocked["status"] = "blocked"
    blocked.pop("publication")
    blocked["decision"] = {
        **blocked["decision"],  # type: ignore[arg-type]
        "worthiness": "block",
        "daily_count_before": 8,
        "daily_count_after": 8,
    }
    blocked["block"] = {
        "reason": "daily_cap_exceeded",
        "operator_notice": "Candidate blocked because @decodexspace already posted 8 times today.",
    }
    assert_valid(blocked)

    over_cap_published = base_record()
    over_cap_published["decision"] = {
        **over_cap_published["decision"],  # type: ignore[arg-type]
        "daily_limit": 9,
    }
    assert_invalid(over_cap_published, "daily_limit must be 8")

    print("OK")


if __name__ == "__main__":
    main()
