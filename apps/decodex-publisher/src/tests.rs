use std::{fs, path::Path, thread};

use serde_json::Value;

use crate::{SocialReservePublishRequest, SocialTerminalizeSkipRequest, social_validation};

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
fn rejects_duplicate_social_outcome_windows() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let post = temp_dir.path().join("posts/post.json");
	let first = temp_dir.path().join("outcomes/one.json");
	let second = temp_dir.path().join("outcomes/two.json");

	fs::create_dir_all(first.parent().expect("fixture should have parent"))
		.expect("fixture directory should be created");
	fs::create_dir_all(post.parent().expect("fixture should have parent"))
		.expect("fixture directory should be created");
	fs::write(&post, valid_social_post().to_string()).expect("fixture should be written");
	let mut outcome = valid_social_outcome();
	outcome["social_post_ref"] = serde_json::json!(post.to_string_lossy());
	fs::write(&first, outcome.to_string()).expect("fixture should be written");
	fs::write(&second, outcome.to_string()).expect("fixture should be written");

	let error = crate::validate_social(&[temp_dir.path().to_path_buf()])
		.expect_err("duplicate outcome windows should fail")
		.to_string();

	assert!(error.contains("duplicate social_outcome cycle"));
}

#[test]
fn validates_bounded_strategy_and_rejects_duplicate_cycles() {
	let strategy = valid_social_strategy();

	assert_social_errors(&strategy, []);

	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let first = temp_dir.path().join("strategy/one.json");
	let second = temp_dir.path().join("strategy/two.json");
	fs::create_dir_all(first.parent().expect("fixture should have parent"))
		.expect("fixture directory should be created");
	fs::write(&first, strategy.to_string()).expect("fixture should be written");
	fs::write(&second, strategy.to_string()).expect("fixture should be written");

	let error = crate::validate_social(&[temp_dir.path().to_path_buf()])
		.expect_err("duplicate strategy cycle should fail")
		.to_string();
	assert!(error.contains("duplicate social_strategy cycle_key"));
}

#[test]
fn numerical_strategy_change_requires_three_distinct_valid_24h_outcomes() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let mut outcome_paths = Vec::new();

	for index in 1..=3 {
		let post_path = temp_dir.path().join(format!("posts/post-{index}.json"));
		let outcome_path = temp_dir.path().join(format!("outcomes/outcome-{index}.json"));
		fs::create_dir_all(post_path.parent().expect("fixture should have parent"))
			.expect("fixture directory should be created");
		fs::create_dir_all(outcome_path.parent().expect("fixture should have parent"))
			.expect("fixture directory should be created");

		let mut post = valid_social_post();
		post["slug"] = serde_json::json!(format!("post-{index}"));
		post["decision"]["idempotency_key"] =
			serde_json::json!(format!("x:decodexspace:operator_impact:post-{index}"));
		post["publication"]["published_urls"] =
			serde_json::json!([format!("https://x.com/decodexspace/status/{index}")]);
		fs::write(&post_path, post.to_string()).expect("fixture should be written");

		let mut outcome = valid_social_outcome();
		outcome["slug"] = serde_json::json!(format!("post-{index}-24h"));
		outcome["social_post_ref"] = serde_json::json!(post_path.to_string_lossy());
		outcome["published_url"] =
			serde_json::json!(format!("https://x.com/decodexspace/status/{index}"));
		fs::write(&outcome_path, outcome.to_string()).expect("fixture should be written");
		outcome_paths.push(outcome_path.to_string_lossy().into_owned());
	}

	let strategy_path = temp_dir.path().join("strategy/weekly.json");
	fs::create_dir_all(strategy_path.parent().expect("fixture should have parent"))
		.expect("fixture directory should be created");
	let mut strategy = valid_social_strategy();
	strategy["decisions"] = serde_json::json!([
		{
			"dimension": "topic_weight",
			"key": "operator_impact",
			"previous_value": 1.0,
			"next_value": 1.1,
			"reason": "Three source-backed posts produced qualified replies."
		},
		{
			"dimension": "no_change",
			"key": "weekly_editorial_benchmark",
			"previous_value": "completed",
			"next_value": "completed",
			"reason": "The bounded browser benchmark completed."
		}
	]);
	let benchmark_url = "https://x.com/CodexReleases/status/100";
	strategy["evidence_refs"] =
		serde_json::json!([outcome_paths[0].clone(), outcome_paths[1].clone(), benchmark_url]);
	fs::write(&strategy_path, strategy.to_string()).expect("fixture should be written");

	let error = crate::validate_social(&[temp_dir.path().to_path_buf()])
		.expect_err("two outcomes must not authorize a numerical strategy change")
		.to_string();
	assert!(error.contains("at least three"));

	strategy["evidence_refs"] = serde_json::json!([
		outcome_paths[0].clone(),
		outcome_paths[1].clone(),
		outcome_paths[2].clone(),
		benchmark_url
	]);
	fs::write(&strategy_path, strategy.to_string()).expect("fixture should be written");
	crate::validate_social(&[temp_dir.path().to_path_buf()])
		.expect("three distinct valid 24h outcomes should authorize the change");
}

#[test]
fn weekly_strategy_requires_a_bounded_editorial_benchmark() {
	let mut missing = valid_social_strategy();
	missing.as_object_mut().expect("strategy should be an object").remove("editorial_benchmark");
	assert_social_errors(&missing, ["editorial_benchmark is required for weekly strategies"]);

	let mut too_many = valid_social_strategy();
	let urls = (1..=13)
		.map(|index| format!("https://x.com/CodexReleases/status/{index}"))
		.collect::<Vec<_>>();
	too_many["editorial_benchmark"]["public_post_urls"] = serde_json::json!(urls);
	too_many["evidence_refs"] = too_many["editorial_benchmark"]["public_post_urls"].clone();
	assert_social_errors(
		&too_many,
		["editorial_benchmark.public_post_urls must contain at most 12 entries"],
	);

	let mut deferred = valid_social_strategy();
	deferred["editorial_benchmark"] = serde_json::json!({
		"status": "deferred",
		"reason_code": "lease_busy",
		"observations": ["The shared browser lease was already owned."]
	});
	deferred["evidence_refs"] = serde_json::json!(["benchmark:deferred:lease_busy"]);
	assert_social_errors(&deferred, []);

	deferred["editorial_benchmark"]["reason_code"] = serde_json::json!("Lease Busy");
	assert_social_errors(
		&deferred,
		["editorial_benchmark.reason_code must be a bounded reason code"],
	);
}

#[test]
fn daily_strategy_must_not_include_editorial_benchmark() {
	let mut daily = valid_social_strategy();
	daily["cadence"] = serde_json::json!("daily");
	daily["cycle_key"] = serde_json::json!("daily:2026-06-08");
	daily["next_review_at"] = serde_json::json!("2026-06-09T03:00:00Z");

	assert_social_errors(&daily, ["editorial_benchmark must be absent for daily strategies"]);
	daily.as_object_mut().expect("strategy should be an object").remove("editorial_benchmark");
	daily["decisions"] = serde_json::json!([{
		"dimension": "no_change",
		"key": "daily_action_review",
		"previous_value": "unchanged",
		"next_value": "unchanged",
		"reason": "No bounded strategy change is justified today."
	}]);
	assert_social_errors(&daily, []);
}

#[test]
fn social_outcome_requires_matching_published_post() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let post = temp_dir.path().join("posts/post.json");
	let outcome_path = temp_dir.path().join("outcomes/outcome.json");

	fs::create_dir_all(post.parent().expect("fixture should have parent"))
		.expect("fixture directory should be created");
	fs::create_dir_all(outcome_path.parent().expect("fixture should have parent"))
		.expect("fixture directory should be created");
	fs::write(&post, valid_social_post().to_string()).expect("fixture should be written");
	let mut outcome = valid_social_outcome();
	outcome["social_post_ref"] = serde_json::json!(post.to_string_lossy());
	fs::write(&outcome_path, outcome.to_string()).expect("fixture should be written");

	crate::validate_social(&[temp_dir.path().to_path_buf()])
		.expect("matching outcome and published post should pass");

	let mut mismatched = outcome;
	mismatched["published_url"] = serde_json::json!("https://x.com/decodexspace/status/2");
	fs::write(&outcome_path, mismatched.to_string()).expect("fixture should be written");
	let error = crate::validate_social(&[temp_dir.path().to_path_buf()])
		.expect_err("mismatched outcome URL should fail")
		.to_string();

	assert!(error.contains("published_url does not match referenced social_post"));

	let mut early = valid_social_outcome();
	early["social_post_ref"] = serde_json::json!(post.to_string_lossy());
	early["observed_at"] = serde_json::json!("2026-06-02T03:01:00Z");
	fs::write(&outcome_path, early.to_string()).expect("fixture should be written");
	let error = crate::validate_social(&[temp_dir.path().to_path_buf()])
		.expect_err("early outcome window should fail")
		.to_string();

	assert!(error.contains("outside its allowed observation interval"));
}

#[test]
fn social_reserve_publish_writes_active_reservation_once() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let lease = crate::acquire_social_browser_lease(&temp_dir.path().join("locks"), 3_600)
		.expect("browser lease should be acquired");
	let lease_token = lease.lease_token.expect("acquired lease should return token");
	let request = social_reserve_request(temp_dir.path(), false, &lease_token);
	let report = crate::reserve_social_publish(&request).expect("reservation should pass");
	let digest = crate::social_publish::idempotency_digest(&request.idempotency_key);

	assert_eq!(report.status, "reserved");
	assert!(
		temp_dir.path().join(format!("reservations/2026-06-02/{digest}.json")).exists(),
		"reservation should be written"
	);

	let duplicate = crate::reserve_social_publish(&request)
		.expect_err("duplicate reservation should fail closed")
		.to_string();

	assert!(duplicate.contains("idempotency_key already has an active reservation"));
}

#[test]
fn browser_lease_serializes_publishers_and_releases_exact_owner() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let locks = temp_dir.path().join("locks");
	let lease = crate::acquire_social_browser_lease(&locks, 3_600)
		.expect("first browser lease should be acquired");
	let lease_token = lease.lease_token.expect("acquired lease should return token");

	let busy = crate::acquire_social_browser_lease(&locks, 3_600)
		.expect_err("second browser lease should fail closed")
		.to_string();
	assert!(busy.contains("already active"));

	let wrong_owner = crate::release_social_browser_lease(&locks, "wrong-token")
		.expect_err("wrong owner must not release the browser lease")
		.to_string();
	assert!(wrong_owner.contains("does not match"));

	crate::verify_social_browser_lease(&locks, &lease_token)
		.expect("exact owner should verify the browser lease");
	let renewed = crate::renew_social_browser_lease(&locks, &lease_token, 7_200)
		.expect("exact owner should renew the browser lease");
	assert_eq!(renewed.status, "renewed");
	assert!(renewed.lease_token.is_none(), "renewal must not echo the lease token");
	let _ = crate::renew_social_browser_lease(&locks, "wrong-token", 3_600)
		.expect_err("wrong owner must not renew the browser lease");
	crate::release_social_browser_lease(&locks, &lease_token)
		.expect("exact owner should release the browser lease");
	crate::acquire_social_browser_lease(&locks, 3_600)
		.expect("browser lease should be reusable after release");
}

#[test]
fn concurrent_reservations_share_one_atomic_idempotency_path() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let lease = crate::acquire_social_browser_lease(&temp_dir.path().join("locks"), 3_600)
		.expect("browser lease should be acquired");
	let lease_token = lease.lease_token.expect("acquired lease should return token");
	let first = social_reserve_request(temp_dir.path(), false, &lease_token);
	let mut second = social_reserve_request(temp_dir.path(), false, &lease_token);
	second.slug = "different-slug-same-idempotency-key".into();

	let outcomes = thread::scope(|scope| {
		let first = scope.spawn(|| crate::reserve_social_publish(&first));
		let second = scope.spawn(|| crate::reserve_social_publish(&second));

		[
			first.join().expect("first reservation thread should finish"),
			second.join().expect("second reservation thread should finish"),
		]
	});
	let success_count = outcomes.iter().filter(|outcome| outcome.is_ok()).count();

	assert_eq!(success_count, 1, "only one concurrent reservation may succeed");
}

#[test]
fn quality_skip_terminalization_is_atomic_and_idempotent() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let candidates = temp_dir.path().join("candidates");
	let candidate_path = candidates.join("candidate.json");
	fs::create_dir_all(&candidates).expect("candidate directory should be created");
	let mut candidate = valid_social_candidate();
	candidate["decision"]["worthiness"] = serde_json::json!("skip");
	candidate["decision"]["reason"] =
		serde_json::json!("The source does not provide a concrete user action.");
	fs::write(&candidate_path, candidate.to_string()).expect("candidate should be written");
	let request = social_skip_request(temp_dir.path(), &candidate_path);

	let outcomes = thread::scope(|scope| {
		let first = scope.spawn(|| crate::terminalize_social_skip(&request));
		let second = scope.spawn(|| crate::terminalize_social_skip(&request));

		[
			first.join().expect("first skip thread should finish"),
			second.join().expect("second skip thread should finish"),
		]
	});
	let reports = outcomes
		.into_iter()
		.map(|outcome| outcome.expect("both idempotent calls should succeed"))
		.collect::<Vec<_>>();

	assert!(reports.iter().any(|report| report.status == "skipped"));
	assert!(reports.iter().any(|report| report.status == "already_skipped"));
	let posts = crate::collect_json_files(&[temp_dir.path().join("posts")])
		.expect("post files should be readable");
	assert_eq!(posts.len(), 1);
	let post = crate::load_json(&posts[0]).expect("skipped post should be readable");
	assert_eq!(post["status"], "skipped");
	assert_eq!(post["browser_touched"], false);
	assert!(post.get("browser_session").is_none());
	assert_social_errors(&post, []);

	let replay =
		crate::terminalize_social_skip(&request).expect("an exact replay should remain idempotent");
	assert_eq!(replay.status, "already_skipped");
}

#[test]
fn quality_skip_and_publish_reservation_share_one_atomic_state_lock() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let candidates = temp_dir.path().join("candidates");
	let candidate_path = candidates.join("candidate.json");
	fs::create_dir_all(&candidates).expect("candidate directory should be created");
	let mut candidate = valid_social_candidate();
	candidate["decision"]["worthiness"] = serde_json::json!("skip");
	candidate["decision"]["reason"] =
		serde_json::json!("The source does not provide a concrete user action.");
	fs::write(&candidate_path, candidate.to_string()).expect("candidate should be written");
	let skip = social_skip_request(temp_dir.path(), &candidate_path);
	let lease = crate::acquire_social_browser_lease(&temp_dir.path().join("locks"), 3_600)
		.expect("browser lease should be acquired");
	let lease_token = lease.lease_token.expect("acquired lease should return token");
	let reserve = social_reserve_request(temp_dir.path(), false, &lease_token);

	let outcomes = thread::scope(|scope| {
		let skip = scope.spawn(|| crate::terminalize_social_skip(&skip).map(|_| "skip"));
		let reserve = scope.spawn(|| crate::reserve_social_publish(&reserve).map(|_| "reserve"));

		[
			skip.join().expect("skip thread should finish"),
			reserve.join().expect("reservation thread should finish"),
		]
	});
	let success_count = outcomes.iter().filter(|outcome| outcome.is_ok()).count();

	assert_eq!(success_count, 1, "skip and reservation must not both succeed");
}

#[test]
fn quality_skip_terminalization_rejects_publish_candidates_and_external_paths() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let candidates = temp_dir.path().join("candidates");
	let candidate_path = candidates.join("candidate.json");
	fs::create_dir_all(&candidates).expect("candidate directory should be created");
	fs::write(&candidate_path, valid_social_candidate().to_string())
		.expect("candidate should be written");
	let request = social_skip_request(temp_dir.path(), &candidate_path);

	let error = crate::terminalize_social_skip(&request)
		.expect_err("publish candidate must not be terminalized as a skip")
		.to_string();
	assert!(error.contains("decision.worthiness must be skip"));

	let external_path = temp_dir.path().join("outside.json");
	let mut skipped = valid_social_candidate();
	skipped["decision"]["worthiness"] = serde_json::json!("skip");
	fs::write(&external_path, skipped.to_string()).expect("external candidate should be written");
	let external = social_skip_request(temp_dir.path(), &external_path);
	let error = crate::terminalize_social_skip(&external)
		.expect_err("candidate outside the configured directory must fail")
		.to_string();
	assert!(error.contains("configured candidates directory"));
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

	let mut deferred = candidate.clone();
	deferred["decision"]["worthiness"] = serde_json::json!("defer");
	assert_social_errors(&deferred, ["decision.worthiness must be one of ['publish', 'skip']"]);

	candidate["source_refs"]
		.as_object_mut()
		.expect("source refs should be object")
		.remove("upstream_impacts");

	assert_social_errors(
		&candidate,
		["source_refs.upstream_impacts must include the shared upstream_impact/v1 handoff"],
	);
}

#[test]
fn accepts_browser_outcome_and_rejects_non_browser_publication() {
	let outcome = valid_social_outcome();

	assert_social_errors(&outcome, []);

	let mut post = valid_social_post();

	post["publication"]["publisher"] = serde_json::json!("x_api");
	assert_social_errors(&post, ["publication.publisher must be chrome"]);

	post["publication"]["publisher"] = serde_json::json!("chrome");
	post["publication"]["published_urls"] = serde_json::json!(["https://x.com/hackink/status/1"]);
	assert_social_errors(
		&post,
		["publication.published_urls must contain only decodexspace X status URLs"],
	);

	post["publication"]["published_urls"] =
		serde_json::json!(["https://x.com/decodexspace/status/1"]);
	post["browser_session"]["restore_status"] = serde_json::json!("not_required");
	assert_social_errors(
		&post,
		[
			"browser_session.restore_status must be restored or failed when initial account is hackink",
		],
	);

	let mut ambiguous = valid_social_post();
	ambiguous["failure"] = serde_json::json!({
		"reason": "ambiguous",
		"details": "This payload must not mix terminal states."
	});
	assert_social_errors(&ambiguous, ["failure must be absent when status is published"]);
}

#[test]
fn validates_schema_shaped_non_published_status_payloads() {
	let mut blocked = valid_social_post();

	blocked["status"] = serde_json::json!("blocked");
	blocked.as_object_mut().expect("post should be an object").remove("publication");
	blocked["block"] = serde_json::json!({
		"reason": "duplicate",
		"operator_notice": "The live profile already contains this source URL."
	});
	blocked["decision"]["daily_count_after"] = blocked["decision"]["daily_count_before"].clone();
	assert_social_errors(&blocked, []);

	let mut failed = blocked.clone();

	failed["status"] = serde_json::json!("failed");
	failed.as_object_mut().expect("post should be an object").remove("block");
	failed["browser_touched"] = serde_json::json!(false);
	failed.as_object_mut().expect("post should be an object").remove("browser_session");
	failed["failure"] = serde_json::json!({
		"reason": "browser_control_unavailable",
		"details": "Chrome control could not be established before compose."
	});
	assert_social_errors(&failed, []);
}

#[test]
fn rejects_schema_extensions_that_could_hide_private_or_api_data() {
	let mut post = valid_social_post();

	post["browser_session"]["cookie"] = serde_json::json!("private");
	assert_social_errors(&post, ["browser_session.cookie is not allowed"]);

	let mut private_evidence = valid_social_post();
	private_evidence["evidence_notes"] =
		serde_json::json!([{"raw_api_payload": {"authorization": "private"}}]);
	assert_social_errors(&private_evidence, ["evidence_notes must be a non-empty list of strings"]);

	let mut outcome = valid_social_outcome();
	outcome["raw_api_payload"] = serde_json::json!({"views": 125});
	assert_social_errors(&outcome, ["social_outcome.raw_api_payload is not allowed"]);

	let mut candidate = valid_social_candidate();
	candidate["source_refs"]["raw_page"] = serde_json::json!("private");
	assert_social_errors(&candidate, ["source_refs.raw_page is not allowed"]);

	let mut reservation = valid_social_publish_reservation();
	reservation["owner"] = serde_json::json!({
		"automation_id": "decodex-x-browser-publisher",
		"cookie": "private"
	});
	assert_social_errors(&reservation, ["owner.cookie is not allowed"]);

	let mut ambiguous_reservation = valid_social_publish_reservation();
	ambiguous_reservation["release_reason"] = serde_json::json!("not active");
	assert_social_errors(
		&ambiguous_reservation,
		["release_reason must be absent when status is active"],
	);
}

#[test]
fn rejects_duplicate_skipped_terminal_post_idempotency_keys() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let first = temp_dir.path().join("posts/one.json");
	let second = temp_dir.path().join("posts/two.json");
	fs::create_dir_all(first.parent().expect("fixture should have parent"))
		.expect("fixture directory should be created");

	let mut skipped = valid_social_post();
	skipped["status"] = serde_json::json!("skipped");
	skipped.as_object_mut().expect("post should be an object").remove("publication");
	skipped["browser_touched"] = serde_json::json!(false);
	skipped.as_object_mut().expect("post should be an object").remove("browser_session");
	skipped["skip"] =
		serde_json::json!({"reason": "The candidate no longer met the quality gate."});
	skipped["decision"]["worthiness"] = serde_json::json!("skip");
	skipped["decision"]["daily_count_after"] = skipped["decision"]["daily_count_before"].clone();

	fs::write(&first, skipped.to_string()).expect("fixture should be written");
	fs::write(&second, skipped.to_string()).expect("fixture should be written");

	let error = crate::validate_social(&[temp_dir.path().to_path_buf()])
		.expect_err("duplicate skipped terminal records should fail")
		.to_string();
	assert!(error.contains("duplicate terminal social_post idempotency_key"));
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

fn social_reserve_request(
	root: &Path,
	dry_run: bool,
	browser_lease_token: &str,
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
		locks_dir: root.join("locks"),
		browser_lease_token: browser_lease_token.into(),
		automation_id: None,
		run_id: None,
		branch: None,
		daily_limit: 8,
		dry_run,
	}
}

fn social_skip_request(root: &Path, candidate_path: &Path) -> SocialTerminalizeSkipRequest {
	SocialTerminalizeSkipRequest {
		candidate_path: candidate_path.to_path_buf(),
		candidates_dir: root.join("candidates"),
		reservations_dir: root.join("reservations"),
		posts_dir: root.join("posts"),
		locks_dir: root.join("locks"),
		day: "2026-06-02".into(),
		timezone: "Asia/Shanghai".into(),
		daily_limit: 8,
		dry_run: false,
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
		"browser_touched": true,
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
		"browser_session": {
			"initial_account": "hackink",
			"target_account": "decodexspace",
			"target_account_verified": true,
			"switch_status": "switched",
			"restore_status": "restored"
		},
		"media_refs": ["https://x.com/decodexspace/status/1/photo/1"]
	})
}

fn valid_social_outcome() -> Value {
	serde_json::json!({
		"schema": "social_outcome/v1",
		"slug": "openai-codex-pr-22414-24h",
		"target_account": "decodexspace",
		"social_post_ref": ".agent/automations/decodex/cache/social/x/posts/2026-06-02/openai-codex-pr-22414.json",
		"published_url": "https://x.com/decodexspace/status/1",
		"observed_at": "2026-06-03T03:00:00Z",
		"window": "24h",
		"metrics": {
			"views": 125,
			"likes": 4,
			"replies": 1,
			"reposts": 2
		},
		"browser_session": {
			"initial_account": "hackink",
			"target_account": "decodexspace",
			"target_account_verified": true,
			"switch_status": "switched",
			"restore_status": "restored"
		},
		"notes": ["Metrics were read from the visible X post page."]
	})
}

fn valid_social_strategy() -> Value {
	serde_json::json!({
		"schema": "social_strategy/v1",
		"cycle_key": "weekly:2026-06-01",
		"cadence": "weekly",
		"reviewed_at": "2026-06-08T03:00:00Z",
		"evidence_refs": [
			".agent/automations/decodex/cache/social/x/outcomes/openai-codex-pr-22414-7d.json",
			"https://x.com/CodexReleases/status/100"
		],
		"decisions": [
			{
				"dimension": "no_change",
				"key": "insufficient_outcomes",
				"previous_value": "unchanged",
				"next_value": "unchanged",
				"reason": "Fewer than three valid 24-hour outcomes are available."
			},
			{
				"dimension": "no_change",
				"key": "weekly_editorial_benchmark",
				"previous_value": "completed",
				"next_value": "completed",
				"reason": "The bounded browser benchmark completed."
			}
		],
		"editorial_benchmark": {
			"status": "completed",
			"public_post_urls": [
				"https://x.com/CodexReleases/status/100"
			],
			"observations": [
				"Direct source links and concrete operator actions are easiest to scan."
			]
		},
		"guardrails": {
			"evidence_gate": "unchanged",
			"privacy_gate": "unchanged",
			"idempotency_gate": "unchanged",
			"account_gate": "unchanged",
			"publication_gate": "unchanged"
		},
		"next_review_at": "2026-06-15T03:00:00Z"
	})
}
