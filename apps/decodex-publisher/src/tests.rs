use std::{fs, path::Path};

use serde_json::Value;

use crate::{SocialReservePublishRequest, social_validation};

#[test]
fn validates_social_reservation_and_rejects_bad_timestamp() {
	let mut reservation = valid_social_publish_reservation();

	assert_social_errors(&reservation, []);

	reservation["reserved_at"] = serde_json::json!("not-a-date");

	assert_social_errors(&reservation, ["reserved_at must be an RFC3339 timestamp"]);
}

#[test]
fn rejects_duplicate_active_social_publish_reservation_idempotency_keys() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let first = temp_dir.path().join("reservations/one.json");
	let second = temp_dir.path().join("reservations/two.json");

	fs::create_dir_all(first.parent().expect("fixture should have parent"))
		.expect("fixture directory should be created");
	fs::write(&first, valid_social_publish_reservation().to_string())
		.expect("fixture should be written");
	fs::write(&second, valid_social_publish_reservation().to_string())
		.expect("fixture should be written");

	let error = crate::validate_social(&[temp_dir.path().to_path_buf()])
		.expect_err("duplicate active reservations should be rejected")
		.to_string();

	assert!(error.contains("duplicate active social_publish_reservation"));
}

#[test]
fn social_reserve_publish_writes_active_reservation_once() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let request = social_reserve_request(temp_dir.path(), false);
	let report = crate::reserve_social_publish(&request).expect("reservation should pass");

	assert_eq!(report.status, "reserved");
	assert!(
		temp_dir.path().join("reservations/2026-06-02/openai-codex-pr-22414.json").exists(),
		"reservation should be written"
	);

	let duplicate = crate::reserve_social_publish(&request)
		.expect_err("duplicate reservation should fail closed")
		.to_string();

	assert!(duplicate.contains("idempotency_key already has an active reservation"));
}

#[test]
fn social_post_rejects_low_quality_public_text() {
	let mut attribution = valid_social_post();

	attribution["text"] = serde_json::json!(["Automated by @hackink: tracking this."]);

	assert_social_errors(&attribution, ["text[0] must not include automation attribution"]);

	let mut generic = valid_social_post();

	generic["text"] = serde_json::json!(["Watching this."]);

	assert_social_errors(&generic, ["must name a concrete source-backed"]);
}

#[test]
fn accepts_valid_social_candidate_and_requires_shared_handoff_for_radar_inputs() {
	let mut candidate = valid_social_candidate();

	assert_social_errors(&candidate, []);

	candidate["source_refs"]
		.as_object_mut()
		.expect("source refs should be object")
		.remove("upstream_impacts");

	assert_social_errors(
		&candidate,
		["source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff"],
	);
}

fn assert_social_errors<const N: usize>(payload: &Value, expected: [&str; N]) {
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

fn social_reserve_request(root: &Path, dry_run: bool) -> SocialReservePublishRequest {
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

fn valid_social_candidate() -> Value {
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

fn valid_social_publish_reservation() -> Value {
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

fn valid_social_post() -> Value {
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
