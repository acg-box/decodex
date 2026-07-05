use std::path::Path;

use serde_json::Value;

use crate::{SocialReservePublishRequest, social_validation};

pub(in crate::tests) fn assert_social_errors<const N: usize>(payload: &Value, expected: [&str; N]) {
	let errors = social_validation::validate_social_artifact(payload).errors;

	for expected in &expected {
		assert!(
			errors.iter().any(|error| error.contains(expected)),
			"expected {expected:?} in {errors:?}"
		);
	}

	if expected.is_empty() {
		assert!(errors.is_empty(), "unexpected validation errors: {errors:?}");
	}
}

pub(in crate::tests) fn social_reserve_request(
	root: &Path,
	dry_run: bool,
) -> SocialReservePublishRequest {
	SocialReservePublishRequest {
		slug: "openai-codex-pr-22414".into(),
		mode: "operator_impact".into(),
		idempotency_key: "x:decodexspace:operator_impact:openai-codex-pr-22414".into(),
		reserved_at: "2026-06-02T03:00:00Z".into(),
		expires_at: "2026-06-02T03:15:00Z".into(),
		day: "2026-06-02".into(),
		timezone: "Asia/Shanghai".into(),
		candidate_paths: vec![root.join("candidate.json")],
		urls: Vec::new(),
		duplicate_keys: vec!["openai-codex-pr-22414".into()],
		out_dir: root.join("reservations"),
		posts_dir: root.join("posts"),
		automation_id: None,
		run_id: None,
		branch: None,
		daily_limit: 8,
		dry_run,
	}
}

pub(in crate::tests) fn valid_social_candidate() -> Value {
	serde_json::json!({
		"schema": "social_candidate/v1",
		"slug": "openai-codex-pr-22414",
		"repo": "openai/codex",
		"channel": "x",
		"target_account": "decodexspace",
		"mode": "operator_impact",
		"priority": "high",
		"audience": "Codex operators",
		"candidate_text": [
			"Remote Codex can use Unix socket endpoints. Source: https://github.com/openai/codex/pull/22414"
		],
		"source_refs": {
			"upstream_reviews": [".agent/automations/radar/cache/github/reviews/openai-codex-pr-22414.review.json"],
			"upstream_impacts": [".agent/automations/radar/cache/github/impact/openai-codex-pr-22414.json"],
			"signals": [".agent/automations/radar/cache/site-content/signals/openai-codex-pr-22414.json"]
		},
		"evidence_notes": ["PR #22414 changes remote endpoint handling."],
		"claims": [
			{
				"text": "Remote Codex can use Unix socket endpoints.",
				"evidence": "https://github.com/openai/codex/pull/22414",
				"confidence": "confirmed"
			}
		],
		"decision": {
			"worthiness": "publish",
			"idempotency_key": "x:decodexspace:operator_impact:openai-codex-pr-22414",
			"reason": "High-value Control Plane transport implication."
		}
	})
}

pub(in crate::tests) fn valid_social_publish_reservation() -> Value {
	serde_json::json!({
		"schema": "social_publish_reservation/v1",
		"slug": "openai-codex-pr-22414",
		"channel": "x",
		"target_account": "decodexspace",
		"controller_account": "hackink",
		"mode": "operator_impact",
		"status": "active",
		"idempotency_key": "x:decodexspace:operator_impact:openai-codex-pr-22414",
		"reserved_at": "2026-06-02T03:00:00Z",
		"expires_at": "2026-06-02T03:15:00Z",
		"day": "2026-06-02",
		"timezone": "Asia/Shanghai",
		"candidate_refs": {
			"social_candidates": [
				".agent/automations/decodex/cache/social/x/candidates/openai-codex-pr-22414.json"
			]
		},
		"duplicate_keys": ["openai-codex-pr-22414"]
	})
}

pub(in crate::tests) fn valid_social_post() -> Value {
	serde_json::json!({
		"schema": "social_post/v1",
		"slug": "openai-codex-pr-22414",
		"channel": "x",
		"target_account": "decodexspace",
		"controller_account": "hackink",
		"mode": "operator_impact",
		"status": "published",
		"audience": "Codex operators",
		"text": [
			"Remote Codex can use Unix socket endpoints. Source: https://github.com/openai/codex/pull/22414"
		],
		"source_refs": {
			"urls": ["https://github.com/openai/codex/pull/22414"]
		},
		"evidence_notes": ["PR #22414 changes remote endpoint handling."],
		"claims": [
			{
				"text": "Remote Codex can use Unix socket endpoints.",
				"evidence": "https://github.com/openai/codex/pull/22414",
				"confidence": "confirmed"
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
			"timezone": "Asia/Shanghai"
		},
		"publication": {
			"posted_at": "2026-06-02T03:00:00Z",
			"published_urls": ["https://x.com/decodexspace/status/1"],
			"publisher": "chrome",
			"account_verified": true,
			"made_with_ai": true
		},
		"media_refs": ["https://x.com/decodexspace/status/1/photo/1"]
	})
}
