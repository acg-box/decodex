use std::{
	fs,
	path::{Path, PathBuf},
};

use clap::Parser as _;
use serde_json::{Value, json};

use crate::{
	SocialClock, SocialObserveDueRequest, SocialObserveXurlRequest, SocialPublishNextRequest,
	SocialPublishXurlRequest, SocialRecordCandidateRequest, SocialReservePublishRequest,
	SocialTerminalizeSkipRequest,
};

const RUN_ID: &str = "019fa400-0000-7000-8000-000000000001";
const SECOND_RUN_ID: &str = "019fa400-0000-7000-8000-000000000002";
const THIRD_RUN_ID: &str = "019fa400-0000-7000-8000-000000000003";
const FOURTH_RUN_ID: &str = "019fa400-0000-7000-8000-000000000004";
const FIFTH_RUN_ID: &str = "019fa400-0000-7000-8000-000000000005";
const POST_TEXT: &str = "Codex app-server now exposes a typed capability check before experimental calls, so operators can detect unsupported protocol surfaces before a workflow starts.";
const SOURCE_URL: &str = "https://github.com/openai/codex/pull/22414";

#[test]
fn content_evidence_requires_primary_sources_and_immutable_identity() {
	let candidate = valid_social_candidate();
	crate::validate_generated_social_artifact(&candidate).expect("valid source-backed candidate");
	crate::social_evidence::validate_source_evidence(&candidate).expect("resolved claims");

	let mut radar_only = candidate.clone();
	radar_only["source_kinds"][SOURCE_URL] = json!("radar_secondary");
	assert_social_error(&radar_only, "at least one official_codex or landed_decodex source");

	let mut unresolved = candidate.clone();
	unresolved["claims"][0]["evidence"] = json!("https://example.com/unbound");
	assert_social_error(&unresolved, "must exactly match one declared source reference");

	let mut tampered = candidate;
	tampered["candidate_text"][0] = json!(format!("{POST_TEXT} Verified."));
	let error = crate::validate_generated_social_artifact(&tampered)
		.expect_err("content changes must invalidate publication identity")
		.to_string();
	assert!(error.contains("immutable content evidence"), "{error}");
}

#[test]
fn record_candidate_is_atomic_idempotent_and_applies_backpressure() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let draft = candidate_draft("publish");
	let staging = write_staging(temp.path(), "first.json", &draft);
	let request = record_request(temp.path(), &staging, RUN_ID);
	let report = crate::record_social_candidate(&request).expect("first record");
	assert_eq!(report.status, "recorded");
	assert_eq!(report.decision, "publish");
	assert!(!staging.exists());
	let recorded = crate::load_json(Path::new(&report.path)).expect("recorded candidate");
	assert!(
		recorded["decision"]["idempotency_key"]
			.as_str()
			.is_some_and(|key| key.starts_with("content-publication:"))
	);

	let retry_staging = write_staging(temp.path(), "retry.json", &draft);
	let retry =
		crate::record_social_candidate(&record_request(temp.path(), &retry_staging, RUN_ID))
			.expect("exact retry");
	assert_eq!(retry.status, "already_recorded");

	let second_staging = write_staging(temp.path(), "second.json", &candidate_draft("no_op"));
	let error = crate::record_social_candidate(&record_request(
		temp.path(),
		&second_staging,
		SECOND_RUN_ID,
	))
	.expect_err("one unresolved candidate must block another")
	.to_string();
	assert!(error.contains("still pending"), "{error}");
}

#[test]
fn reserve_enforces_duplicate_and_one_post_per_day() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), "candidate.json", valid_social_candidate());
	let request = reserve_request(temp.path(), &candidate, RUN_ID);
	crate::reserve_social_publish(&request).expect("first reservation");
	let duplicate =
		crate::reserve_social_publish(&request).expect_err("duplicate reservation").to_string();
	assert!(duplicate.contains("idempotency_key already has"), "{duplicate}");

	let mut other = valid_social_candidate();
	other["slug"] = json!("another-change");
	rebind_identity(&mut other);
	let other = write_candidate(temp.path(), "other.json", other);
	let cap = crate::reserve_social_publish(&reserve_request(temp.path(), &other, SECOND_RUN_ID))
		.expect_err("daily cap")
		.to_string();
	assert!(cap.contains("daily publish cap exhausted"), "{cap}");
}

#[test]
fn no_op_terminalization_is_idempotent_and_has_no_x_effect() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), "candidate.json", candidate("no_op"));
	let request = skip_request(temp.path(), &candidate);
	let first = crate::terminalize_social_skip(&request).expect("first no-op");
	let second = crate::terminalize_social_skip(&request).expect("idempotent no-op");
	assert_eq!(first.status, "skipped");
	assert_eq!(second.status, "already_skipped");
	let post = crate::load_json(Path::new(&first.path)).expect("skip artifact");
	assert_eq!(post["status"], "skipped");
	assert!(post.get("publication").is_none());
}

#[cfg(unix)]
#[test]
fn xurl_publish_and_outcomes_verify_account_text_and_exact_effects() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), "candidate.json", valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	let log = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log, "decodexspace", "decodexspace", true);
	let publish = publish_request(temp.path(), Path::new(&reservation.path), RUN_ID);
	let report =
		crate::social_xurl::publish_with_test_binary(&publish, &xurl).expect("verified publish");
	assert_eq!(report.verified_account, "decodexspace");
	assert_eq!(report.publication_recorded_cost_ceiling_microusd, 30_000);
	assert_eq!(report.monthly_budget_microusd, 1_250_000);
	assert_eq!(report.published_url, "https://x.com/decodexspace/status/2000000000000000001");
	let post = crate::load_json(Path::new(&report.post_path)).expect("post evidence");
	assert_eq!(post["text"][0], POST_TEXT);
	assert_eq!(post["publication"]["post_id"], "2000000000000000001");
	assert_eq!(post["publication"]["verified_user_id"], "42");

	let retry = crate::social_xurl::publish_with_test_binary(&publish, &xurl)
		.expect("local idempotent retry");
	assert_eq!(retry.status, "already_published");

	let outcome_24h = crate::social_xurl::observe_with_test_binary(
		&observe_request(
			temp.path(),
			Path::new(&report.post_path),
			RUN_ID,
			"24h",
			"2026-07-28T12:02:00Z",
		),
		&xurl,
	)
	.expect("24-hour outcome");
	assert_eq!(outcome_24h.window, "24h");
	let outcome_7d = crate::social_xurl::observe_with_test_binary(
		&observe_request(
			temp.path(),
			Path::new(&report.post_path),
			SECOND_RUN_ID,
			"7d",
			"2026-08-03T12:02:00Z",
		),
		&xurl,
	)
	.expect("7-day outcome");
	assert_eq!(outcome_7d.window, "7d");

	let calls = fs::read_to_string(log).expect("xurl log");
	assert_eq!(calls.lines().filter(|call| *call == "post").count(), 1);
	assert_eq!(calls.lines().filter(|call| *call == "read").count(), 3);
}

#[cfg(unix)]
#[test]
fn content_create_obeys_the_actual_utc_effect_boundary() {
	assert_content_create_boundary("2026-07-27T23:57:59Z", true);
	assert_content_create_boundary("2026-07-27T23:58:00Z", false);
	assert_content_create_boundary("2026-07-28T00:00:01Z", false);
}

#[cfg(unix)]
#[test]
fn existing_post_recovery_requires_exact_attempt_bound_bytes() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), "candidate.json", valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	let reservation_path = Path::new(&reservation.path);
	let log = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log, "decodexspace", "decodexspace", true);
	let publish = publish_request(temp.path(), reservation_path, RUN_ID);
	let marker = temp.path().join("interrupt-after-post-write");
	fs::write(&marker, b"interrupt").expect("fault marker");

	let error = crate::social_xurl::publish_with_test_binary(&publish, &xurl)
		.expect_err("post-write interruption")
		.to_string();
	assert!(
		error.contains("simulated interruption after the durable social post write"),
		"{error}"
	);
	fs::remove_file(marker).expect("remove fault marker");

	let post_path = temp.path().join("posts").join(format!("{RUN_ID}.json"));
	let attempt_path = temp.path().join("attempts/2026-07").join(format!("{RUN_ID}.json"));
	let original_post = crate::load_json(&post_path).expect("durable post");
	let original_post_bytes = fs::read(&post_path).expect("durable post bytes");
	let reservation_bytes = fs::read(reservation_path).expect("active reservation bytes");
	let attempt_bytes = fs::read(&attempt_path).expect("verified attempt bytes");
	let xurl_log_bytes = fs::read(&log).expect("xurl log bytes");
	assert_eq!(crate::load_json(reservation_path).expect("reservation")["status"], "active");
	assert_eq!(crate::load_json(&attempt_path).expect("attempt")["status"], "verified");

	let mut tampered_post = original_post.clone();
	tampered_post["publication"]["verified_user_id"] = json!("43");
	crate::validate_generated_social_artifact(&tampered_post).expect("schema-valid damaged post");
	crate::replace_existing_json(&post_path, &original_post, &tampered_post)
		.expect("install damaged post");
	let error = crate::social_xurl::publish_with_test_binary(&publish, &xurl)
		.expect_err("damaged post must fail closed")
		.to_string();
	assert!(error.contains("does not match its durable xurl attempt"), "{error}");
	assert_eq!(fs::read(reservation_path).expect("reservation bytes"), reservation_bytes);
	assert_eq!(fs::read(&attempt_path).expect("attempt bytes"), attempt_bytes);
	assert_eq!(fs::read(&log).expect("xurl log bytes"), xurl_log_bytes);

	crate::replace_existing_json(&post_path, &tampered_post, &original_post)
		.expect("restore exact post");
	assert_eq!(fs::read(&post_path).expect("restored post bytes"), original_post_bytes);
	let recovered =
		crate::social_xurl::publish_with_test_binary(&publish, &xurl).expect("exact post recovery");
	assert_eq!(recovered.status, "already_published");
	assert_eq!(
		crate::load_json(reservation_path).expect("consumed reservation")["status"],
		"consumed"
	);
	let attempt = crate::load_json(&attempt_path).expect("published attempt");
	assert_eq!(attempt["status"], "published");
	assert!(attempt["reserved_cost_ceiling_microusd"].as_u64().is_some_and(|cost| cost <= 60_000));
	let calls = fs::read_to_string(log).expect("xurl log");
	assert_eq!(calls.lines().filter(|call| *call == "post").count(), 1);
}

#[cfg(unix)]
#[test]
fn xurl_rejects_wrong_account_before_create() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), "candidate.json", valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	let log = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log, "decodexspace", "hackink", true);
	let error = crate::social_xurl::publish_with_test_binary(
		&publish_request(temp.path(), Path::new(&reservation.path), RUN_ID),
		&xurl,
	)
	.expect_err("wrong paid identity")
	.to_string();
	assert!(error.contains("identity read did not verify @decodexspace"), "{error}");
	let calls = fs::read_to_string(log).expect("xurl log");
	assert!(!calls.lines().any(|call| call == "post"));
}

#[cfg(unix)]
#[test]
fn uncertain_create_is_never_retried_and_blocks_the_lineage() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), "candidate.json", valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	let log = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log, "decodexspace", "decodexspace", false);
	let publish = publish_request(temp.path(), Path::new(&reservation.path), RUN_ID);
	let _ = crate::social_xurl::publish_with_test_binary(&publish, &xurl)
		.expect_err("invalid create response is uncertain");
	let retry = crate::social_xurl::publish_with_test_binary(&publish, &xurl)
		.expect_err("unknown create cannot retry")
		.to_string();
	assert!(retry.contains("create outcome is unknown"), "{retry}");

	let mut later = reserve_request(temp.path(), &candidate, SECOND_RUN_ID);
	later.reserved_at = "2026-07-27T14:00:00Z".into();
	later.expires_at = "2026-07-27T15:00:00Z".into();
	let blocked = crate::reserve_social_publish(&later)
		.expect_err("uncertain effect survives reservation expiry")
		.to_string();
	assert!(blocked.contains("prior uncertain or verified public-write attempt"), "{blocked}");
	let calls = fs::read_to_string(log).expect("xurl log");
	assert_eq!(calls.lines().filter(|call| *call == "post").count(), 1);
}

#[cfg(unix)]
#[test]
fn monthly_budget_stops_before_public_write() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), "candidate.json", valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	for index in 0..41 {
		write_budget_attempt(temp.path(), index);
	}
	let log = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log, "decodexspace", "decodexspace", true);
	let error = crate::social_xurl::publish_with_test_binary(
		&publish_request(temp.path(), Path::new(&reservation.path), RUN_ID),
		&xurl,
	)
	.expect_err("monthly cap")
	.to_string();
	assert!(error.contains("monthly X budget exhausted"), "{error}");
	let calls = fs::read_to_string(log).unwrap_or_default();
	assert!(!calls.lines().any(|call| call == "post"));
}

#[cfg(unix)]
#[test]
fn high_level_publish_is_oldest_first_and_overdue_outcomes_remain_observable() {
	let temp = tempfile::tempdir().expect("temporary directory");
	write_auth_contract(temp.path());
	let oldest = write_candidate(
		temp.path(),
		"019fa400-0000-7000-8000-000000000010.json",
		valid_social_candidate(),
	);
	let mut newer = valid_social_candidate();
	newer["slug"] = json!("newer-upstream-change");
	rebind_identity(&mut newer);
	let newer = write_candidate(temp.path(), "019fa400-0000-7000-8000-000000000020.json", newer);
	let log = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log, "decodexspace", "decodexspace", true);

	let published = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(RUN_ID, "publish", None, "2026-07-27T12:02:00Z"),
		temp.path(),
		&xurl,
	)
	.expect("oldest candidate publish");
	assert_eq!(published.status, "published");
	let oldest_ref = crate::path_arg(&crate::repo_root().expect("repo root"), &oldest);
	assert_eq!(published.candidate_path.as_deref(), Some(oldest_ref.as_str()));
	assert!(newer.exists(), "newer candidate must remain pending");

	let overdue_24h = crate::social_workflow::observe_due_with_test_binary(
		&SocialObserveDueRequest {
			run_id: SECOND_RUN_ID.into(),
			observed_at: "2026-08-20T12:02:00Z".into(),
		},
		temp.path(),
		&xurl,
	)
	.expect("overdue 24-hour observation");
	assert_eq!(overdue_24h.status, "observed");
	assert_eq!(overdue_24h.window.as_deref(), Some("24h"));

	let overdue_7d = crate::social_workflow::observe_due_with_test_binary(
		&SocialObserveDueRequest {
			run_id: THIRD_RUN_ID.into(),
			observed_at: "2026-08-20T12:03:00Z".into(),
		},
		temp.path(),
		&xurl,
	)
	.expect("overdue 7-day observation");
	assert_eq!(overdue_7d.status, "observed");
	assert_eq!(overdue_7d.window.as_deref(), Some("7d"));
}

#[cfg(unix)]
#[test]
fn high_level_no_op_and_quality_skip_are_terminal_without_x_calls() {
	for (worth, decision, reason) in [
		("no_op", "publish", None),
		("publish", "skip", Some("The wording repeats a recently published topic.")),
	] {
		let temp = tempfile::tempdir().expect("temporary directory");
		let candidate = write_candidate(temp.path(), "candidate.json", candidate(worth));
		let log = temp.path().join("xurl.log");
		let xurl = fake_xurl(temp.path(), &log, "decodexspace", "decodexspace", true);
		let report = crate::social_workflow::publish_next_with_test_binary(
			&publish_next_request(RUN_ID, decision, reason, "2026-07-27T12:02:00Z"),
			temp.path(),
			&xurl,
		)
		.expect("terminal content decision");
		assert_eq!(report.status, "skipped");
		assert_eq!(report.candidate_path.as_deref(), Some(candidate.to_string_lossy().as_ref()));
		assert!(fs::read_to_string(&log).unwrap_or_default().is_empty());
	}
}

#[cfg(unix)]
#[test]
fn high_level_restart_recovers_readback_without_a_second_create() {
	let temp = tempfile::tempdir().expect("temporary directory");
	write_auth_contract(temp.path());
	write_candidate(temp.path(), "candidate.json", valid_social_candidate());
	let log = temp.path().join("xurl.log");
	let xurl = fake_xurl_with_initial_read_failures(temp.path(), &log);

	let first = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(RUN_ID, "publish", None, "2026-07-27T12:02:00Z"),
		temp.path(),
		&xurl,
	)
	.expect_err("first readback is interrupted")
	.to_string();
	assert!(first.contains("xurl"), "{first}");

	let recovered = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(SECOND_RUN_ID, "publish", None, "2026-07-27T12:10:00Z"),
		temp.path(),
		&xurl,
	)
	.expect("safe read recovery");
	assert_eq!(recovered.status, "recovered_interrupted_effect");
	let calls = fs::read_to_string(log).expect("xurl log");
	assert_eq!(calls.lines().filter(|call| *call == "post").count(), 1);
	assert_eq!(calls.lines().filter(|call| *call == "read").count(), 3);
}

#[cfg(unix)]
#[test]
fn high_level_terminal_identity_recovery_reaches_a_fresh_publish_path() {
	let temp = tempfile::tempdir().expect("temporary directory");
	write_auth_contract(temp.path());
	write_candidate(temp.path(), "candidate.json", valid_social_candidate());
	let log = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log, "decodexspace", "decodexspace", true);

	let interrupted = crate::social_workflow::publish_next_with_identity_interruption_for_test(
		&publish_next_request(RUN_ID, "publish", None, "2026-07-27T12:02:00Z"),
		temp.path(),
		&xurl,
	)
	.expect_err("identity read is interrupted after its durable reservation")
	.to_string();
	assert!(interrupted.contains("simulated interruption"), "{interrupted}");
	let attempt_path = temp.path().join("attempts/2026-07").join(format!("{RUN_ID}.json"));
	let attempt = crate::load_json(&attempt_path).expect("interrupted attempt");
	assert_eq!(attempt["status"], "identity_inflight");
	assert!(attempt.get("reconciliation").is_none());

	let recovered = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(SECOND_RUN_ID, "publish", None, "2026-07-27T12:10:00Z"),
		temp.path(),
		&xurl,
	)
	.expect("safe identity recovery");
	assert_eq!(recovered.status, "recovered_interrupted_effect");
	let attempt = crate::load_json(&attempt_path).expect("reconciled attempt");
	assert_eq!(attempt["status"], "identity_reconciled");
	assert_eq!(attempt["reconciliation"]["operation_id"], SECOND_RUN_ID);
	let terminal_attempt = fs::read(&attempt_path).expect("terminal attempt bytes");

	let published = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(THIRD_RUN_ID, "publish", None, "2026-07-27T12:20:00Z"),
		temp.path(),
		&xurl,
	)
	.expect("fresh publication after terminal identity recovery");
	assert_eq!(published.status, "published");
	assert_eq!(
		fs::read(&attempt_path).expect("old terminal attempt bytes"),
		terminal_attempt,
		"the old identity attempt must not be reconciled again"
	);
	let observed_24h = crate::social_workflow::observe_due_with_test_binary(
		&SocialObserveDueRequest {
			run_id: FOURTH_RUN_ID.into(),
			observed_at: "2026-07-28T12:20:00Z".into(),
		},
		temp.path(),
		&xurl,
	)
	.expect("24-hour observation");
	assert_eq!(observed_24h.status, "observed");
	let observed_7d = crate::social_workflow::observe_due_with_test_binary(
		&SocialObserveDueRequest {
			run_id: FIFTH_RUN_ID.into(),
			observed_at: "2026-08-03T12:20:00Z".into(),
		},
		temp.path(),
		&xurl,
	)
	.expect("7-day observation");
	assert_eq!(observed_7d.status, "observed");
	assert_eq!(
		fs::read(&attempt_path).expect("old terminal attempt after observations"),
		terminal_attempt
	);

	let reservations = crate::collect_json_files(&[temp.path().join("reservations")])
		.expect("publication reservations");
	let fresh = reservations
		.iter()
		.map(|path| crate::load_json(path).expect("reservation"))
		.find(|reservation| {
			reservation.pointer("/owner/run_id").and_then(Value::as_str) == Some(THIRD_RUN_ID)
		})
		.expect("fresh third-run reservation");
	assert_eq!(fresh["status"], "consumed");
	let attempts = crate::collect_json_files(&[temp.path().join("attempts")])
		.expect("complete xurl attempt lineage");
	let reserved = attempts
		.iter()
		.map(|path| crate::load_json(path).expect("xurl attempt"))
		.map(|attempt| {
			attempt["reserved_cost_ceiling_microusd"].as_u64().expect("reserved ceiling")
		})
		.sum::<u64>();
	assert_eq!(reserved, 60_000);
	let calls = fs::read_to_string(log).expect("xurl log");
	assert_eq!(calls.lines().filter(|call| *call == "/2/users/me").count(), 2);
	assert_eq!(calls.lines().filter(|call| *call == "post").count(), 1);
	assert_eq!(calls.lines().filter(|call| *call == "read").count(), 3);
}

#[cfg(unix)]
#[test]
fn reservation_only_restart_releases_old_owner_before_and_after_expiry() {
	for restart_at in ["2026-07-27T12:10:00Z", "2026-07-28T12:10:00Z"] {
		let temp = tempfile::tempdir().expect("temporary directory");
		write_auth_contract(temp.path());
		write_candidate(temp.path(), "candidate.json", valid_social_candidate());
		let log = temp.path().join("xurl.log");
		let xurl = fake_xurl(temp.path(), &log, "decodexspace", "decodexspace", true);

		let error = crate::social_workflow::publish_next_with_reservation_interruption_for_test(
			&publish_next_request(RUN_ID, "publish", None, "2026-07-27T12:02:00Z"),
			temp.path(),
			&xurl,
		)
		.expect_err("reservation boundary interruption")
		.to_string();
		assert!(error.contains("durable reservation"), "{error}");
		assert!(
			crate::collect_json_files(&[temp.path().join("attempts")])
				.expect("attempt files")
				.is_empty()
		);
		assert_no_paid_xurl_calls(&log);

		let published = crate::social_workflow::publish_next_with_test_binary(
			&publish_next_request(SECOND_RUN_ID, "publish", None, restart_at),
			temp.path(),
			&xurl,
		)
		.expect("orphaned reservation recovery");
		assert_eq!(published.status, "published");
		let repeated = crate::social_workflow::publish_next_with_test_binary(
			&publish_next_request(THIRD_RUN_ID, "publish", None, restart_at),
			temp.path(),
			&xurl,
		)
		.expect("idempotent restart");
		assert_eq!(repeated.status, "no_candidate");

		let reservations =
			crate::collect_json_files(&[temp.path().join("reservations")]).expect("reservations");
		let old = reservations
			.iter()
			.map(|path| crate::load_json(path).expect("reservation"))
			.find(|reservation| {
				reservation.pointer("/owner/run_id").and_then(Value::as_str) == Some(RUN_ID)
			})
			.expect("old reservation");
		assert_eq!(old["status"], "expired");
		assert_eq!(
			old["release_reason"],
			"Reservation owner ended before any durable xurl attempt."
		);
		let calls = fs::read_to_string(&log).expect("xurl log");
		assert_eq!(calls.lines().filter(|call| *call == "post").count(), 1);
	}
}

#[cfg(unix)]
#[test]
fn reserved_attempt_restart_terminalizes_no_call_before_and_after_expiry() {
	for (recovery_at, publish_at) in [
		("2026-07-27T12:10:00Z", "2026-07-27T12:20:00Z"),
		("2026-07-28T12:10:00Z", "2026-07-28T12:20:00Z"),
	] {
		let temp = tempfile::tempdir().expect("temporary directory");
		write_auth_contract(temp.path());
		write_candidate(temp.path(), "candidate.json", valid_social_candidate());
		let log = temp.path().join("xurl.log");
		let xurl = fake_xurl(temp.path(), &log, "decodexspace", "decodexspace", true);

		let error =
			crate::social_workflow::publish_next_with_reserved_attempt_interruption_for_test(
				&publish_next_request(RUN_ID, "publish", None, "2026-07-27T12:02:00Z"),
				temp.path(),
				&xurl,
			)
			.expect_err("reserved attempt boundary interruption")
			.to_string();
		assert!(error.contains("durable reserved attempt"), "{error}");
		let attempt_path = temp.path().join("attempts/2026-07").join(format!("{RUN_ID}.json"));
		let reserved = crate::load_json(&attempt_path).expect("reserved attempt");
		assert_eq!(reserved["status"], "reserved");
		assert_eq!(reserved["calls"], json!([]));
		assert_no_paid_xurl_calls(&log);

		let recovered = crate::social_workflow::publish_next_with_test_binary(
			&publish_next_request(SECOND_RUN_ID, "publish", None, recovery_at),
			temp.path(),
			&xurl,
		)
		.expect("no-call attempt recovery");
		assert_eq!(recovered.status, "recovered_interrupted_effect");
		let terminal_bytes = fs::read(&attempt_path).expect("terminal attempt bytes");
		let terminal: crate::social_xurl::model::XurlAttempt =
			serde_json::from_slice(&terminal_bytes).expect("terminal attempt");
		assert_eq!(terminal.status, crate::social_xurl::model::NO_CREATE_RELEASED_STATUS);
		assert_eq!(terminal.reserved_cost_ceiling_microusd, 0);
		crate::social_xurl::ledger::validate_publication_cost_record(&terminal)
			.expect("valid terminal attempt");
		assert_no_paid_xurl_calls(&log);

		let published = crate::social_workflow::publish_next_with_test_binary(
			&publish_next_request(THIRD_RUN_ID, "publish", None, publish_at),
			temp.path(),
			&xurl,
		)
		.expect("fresh publication after no-call terminalization");
		assert_eq!(published.status, "published");
		let repeated = crate::social_workflow::publish_next_with_test_binary(
			&publish_next_request(FOURTH_RUN_ID, "publish", None, publish_at),
			temp.path(),
			&xurl,
		)
		.expect("idempotent terminal restart");
		assert_eq!(repeated.status, "no_candidate");
		assert_eq!(fs::read(&attempt_path).expect("old attempt bytes"), terminal_bytes);
		let calls = fs::read_to_string(&log).expect("xurl log");
		assert_eq!(calls.lines().filter(|call| *call == "post").count(), 1);
	}
}

#[cfg(unix)]
#[test]
fn halted_identity_failures_recover_without_repeating_create() {
	for marker in ["identity-command-failure", "wrong-account"] {
		let temp = tempfile::tempdir().expect("temporary directory");
		write_auth_contract(temp.path());
		write_candidate(temp.path(), "candidate.json", valid_social_candidate());
		let log = temp.path().join("xurl.log");
		let xurl = faultable_fake_xurl(temp.path(), &log);
		fs::write(temp.path().join(marker), b"1").expect("fault marker");

		let _ = crate::social_workflow::publish_next_with_test_binary(
			&publish_next_request(RUN_ID, "publish", None, "2026-07-27T12:02:00Z"),
			temp.path(),
			&xurl,
		)
		.expect_err("initial identity failure");
		fs::remove_file(temp.path().join(marker)).expect("remove identity fault");
		let recovered = crate::social_workflow::publish_next_with_test_binary(
			&publish_next_request(SECOND_RUN_ID, "publish", None, "2026-07-27T12:10:00Z"),
			temp.path(),
			&xurl,
		)
		.expect("identity recovery");
		assert_eq!(recovered.status, "recovered_interrupted_effect");
		let published = crate::social_workflow::publish_next_with_test_binary(
			&publish_next_request(THIRD_RUN_ID, "publish", None, "2026-07-27T12:20:00Z"),
			temp.path(),
			&xurl,
		)
		.expect("publication after corrected identity");
		assert_eq!(published.status, "published");
		let reserved = total_reserved_cost(temp.path());
		assert_eq!(reserved, 50_000);
		assert!(reserved <= 60_000);
		assert!(reserved <= crate::SOCIAL_MONTHLY_BUDGET_MICROUSD);
		let calls = fs::read_to_string(&log).expect("xurl log");
		assert_eq!(calls.lines().filter(|call| *call == "post").count(), 1);
	}
}

#[cfg(unix)]
#[test]
fn exhausted_identity_recovery_terminalizes_and_allows_a_fresh_run() {
	let temp = tempfile::tempdir().expect("temporary directory");
	write_auth_contract(temp.path());
	write_candidate(temp.path(), "candidate.json", valid_social_candidate());
	let log = temp.path().join("xurl.log");
	let xurl = faultable_fake_xurl(temp.path(), &log);
	let marker = temp.path().join("identity-command-failure");
	fs::write(&marker, b"1").expect("identity fault marker");

	let _ = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(RUN_ID, "publish", None, "2026-07-27T12:02:00Z"),
		temp.path(),
		&xurl,
	)
	.expect_err("initial identity failure");
	let exhausted = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(SECOND_RUN_ID, "publish", None, "2026-07-27T12:10:00Z"),
		temp.path(),
		&xurl,
	)
	.expect("bounded identity exhaustion");
	assert_eq!(exhausted.status, "recovered_interrupted_effect");
	let old_attempt_path = temp.path().join("attempts/2026-07").join(format!("{RUN_ID}.json"));
	let old_attempt: crate::social_xurl::model::XurlAttempt =
		serde_json::from_value(crate::load_json(&old_attempt_path).expect("old attempt"))
			.expect("typed old attempt");
	assert_eq!(old_attempt.status, crate::social_xurl::model::IDENTITY_RECOVERY_EXHAUSTED_STATUS);
	crate::social_xurl::ledger::validate_publication_cost_record(&old_attempt)
		.expect("valid exhausted identity attempt");
	fs::remove_file(marker).expect("remove identity fault");
	let published = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(THIRD_RUN_ID, "publish", None, "2026-07-27T12:20:00Z"),
		temp.path(),
		&xurl,
	)
	.expect("fresh run after identity exhaustion");
	assert_eq!(published.status, "published");
	let calls = fs::read_to_string(&log).expect("xurl log");
	assert_eq!(calls.lines().filter(|call| *call == "post").count(), 1);
}

#[cfg(unix)]
#[test]
fn publication_recovery_stops_before_a_sixth_paid_call() {
	let temp = tempfile::tempdir().expect("temporary directory");
	write_auth_contract(temp.path());
	write_candidate(temp.path(), "candidate.json", valid_social_candidate());
	let log = temp.path().join("xurl.log");
	let xurl = fake_xurl_with_read_failure_count(temp.path(), &log, 3);

	let _ = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(RUN_ID, "publish", None, "2026-07-27T12:02:00Z"),
		temp.path(),
		&xurl,
	)
	.expect_err("initial and automatic read failures");
	let _ = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(SECOND_RUN_ID, "publish", None, "2026-07-27T12:10:00Z"),
		temp.path(),
		&xurl,
	)
	.expect_err("first reconciliation read failure");
	let terminal = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(THIRD_RUN_ID, "publish", None, "2026-07-27T12:20:00Z"),
		temp.path(),
		&xurl,
	)
	.expect("bounded read recovery exhaustion");
	assert_eq!(terminal.status, "recovered_interrupted_effect");

	let attempt_path = temp.path().join("attempts/2026-07").join(format!("{RUN_ID}.json"));
	let terminal_bytes = fs::read(&attempt_path).expect("terminal attempt bytes");
	let attempt: crate::social_xurl::model::XurlAttempt =
		serde_json::from_slice(&terminal_bytes).expect("terminal attempt");
	assert_eq!(attempt.status, crate::social_xurl::model::READ_RECOVERY_EXHAUSTED_STATUS);
	assert_eq!(attempt.calls.len(), 5);
	assert_eq!(attempt.reserved_cost_ceiling_microusd, 40_000);
	crate::social_xurl::ledger::validate_publication_cost_record(&attempt)
		.expect("valid five-call terminal attempt");
	let repeated = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(FOURTH_RUN_ID, "publish", None, "2026-07-28T12:20:00Z"),
		temp.path(),
		&xurl,
	)
	.expect("terminal lineage is skipped");
	assert_eq!(repeated.status, "no_candidate");
	assert_eq!(fs::read(&attempt_path).expect("unchanged attempt"), terminal_bytes);
	let calls = fs::read_to_string(&log).expect("xurl log");
	assert_eq!(calls.lines().filter(|call| *call == "post").count(), 1);
	assert_eq!(calls.lines().filter(|call| *call == "read").count(), 3);
}

#[cfg(unix)]
#[test]
fn outcome_failures_recover_or_terminalize_without_blocking_other_windows() {
	let recovered = tempfile::tempdir().expect("temporary directory");
	write_auth_contract(recovered.path());
	write_candidate(recovered.path(), "candidate.json", valid_social_candidate());
	let recovered_log = recovered.path().join("xurl.log");
	let recovered_xurl = faultable_fake_xurl(recovered.path(), &recovered_log);
	crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(RUN_ID, "publish", None, "2026-07-27T12:02:00Z"),
		recovered.path(),
		&recovered_xurl,
	)
	.expect("publication");
	let recovered_marker = recovered.path().join("read-failure");
	fs::write(&recovered_marker, b"1").expect("read fault marker");
	let _ = crate::social_workflow::observe_due_with_test_binary(
		&SocialObserveDueRequest {
			run_id: SECOND_RUN_ID.into(),
			observed_at: "2026-07-28T12:20:00Z".into(),
		},
		recovered.path(),
		&recovered_xurl,
	)
	.expect_err("initial outcome read failure");
	fs::remove_file(recovered_marker).expect("remove read fault");
	let report = crate::social_workflow::observe_due_with_test_binary(
		&SocialObserveDueRequest {
			run_id: THIRD_RUN_ID.into(),
			observed_at: "2026-07-28T12:30:00Z".into(),
		},
		recovered.path(),
		&recovered_xurl,
	)
	.expect("outcome read recovery");
	assert_eq!(report.status, "recovered_interrupted_effect");

	let exhausted = tempfile::tempdir().expect("temporary directory");
	write_auth_contract(exhausted.path());
	write_candidate(exhausted.path(), "candidate.json", valid_social_candidate());
	let exhausted_log = exhausted.path().join("xurl.log");
	let exhausted_xurl = faultable_fake_xurl(exhausted.path(), &exhausted_log);
	crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(RUN_ID, "publish", None, "2026-07-27T12:02:00Z"),
		exhausted.path(),
		&exhausted_xurl,
	)
	.expect("publication");
	let exhausted_marker = exhausted.path().join("read-failure");
	fs::write(&exhausted_marker, b"1").expect("read fault marker");
	for (run_id, observed_at) in
		[(SECOND_RUN_ID, "2026-07-28T12:20:00Z"), (THIRD_RUN_ID, "2026-07-28T12:30:00Z")]
	{
		let _ = crate::social_workflow::observe_due_with_test_binary(
			&SocialObserveDueRequest { run_id: run_id.into(), observed_at: observed_at.into() },
			exhausted.path(),
			&exhausted_xurl,
		)
		.expect_err("bounded outcome recovery failure");
	}
	let terminal = crate::social_workflow::observe_due_with_test_binary(
		&SocialObserveDueRequest {
			run_id: FOURTH_RUN_ID.into(),
			observed_at: "2026-07-28T12:40:00Z".into(),
		},
		exhausted.path(),
		&exhausted_xurl,
	)
	.expect("terminal outcome recovery");
	assert_eq!(terminal.status, "recovered_interrupted_effect");
	fs::remove_file(exhausted_marker).expect("remove read fault");
	let next_window = crate::social_workflow::observe_due_with_test_binary(
		&SocialObserveDueRequest {
			run_id: FIFTH_RUN_ID.into(),
			observed_at: "2026-08-03T12:20:00Z".into(),
		},
		exhausted.path(),
		&exhausted_xurl,
	)
	.expect("unrelated outcome window");
	assert_eq!(next_window.status, "observed");
	assert_eq!(next_window.window.as_deref(), Some("7d"));
	let reserved = total_reserved_cost(exhausted.path());
	assert_eq!(reserved, 50_000);
	assert!(reserved <= 60_000);
}

#[cfg(unix)]
#[test]
fn high_level_publish_enforces_account_and_monthly_budget_before_create() {
	let wrong_account = tempfile::tempdir().expect("temporary directory");
	write_auth_contract(wrong_account.path());
	write_candidate(wrong_account.path(), "candidate.json", valid_social_candidate());
	let account_log = wrong_account.path().join("xurl.log");
	let wrong_xurl = fake_xurl(wrong_account.path(), &account_log, "decodexspace", "hackink", true);
	let account_error = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(RUN_ID, "publish", None, "2026-07-27T12:02:00Z"),
		wrong_account.path(),
		&wrong_xurl,
	)
	.expect_err("wrong account")
	.to_string();
	assert!(account_error.contains("did not verify @decodexspace"), "{account_error}");
	assert!(
		!fs::read_to_string(account_log).expect("account log").lines().any(|call| call == "post")
	);

	let exhausted = tempfile::tempdir().expect("temporary directory");
	write_auth_contract(exhausted.path());
	write_candidate(exhausted.path(), "candidate.json", valid_social_candidate());
	for index in 0..41 {
		write_budget_attempt(exhausted.path(), index);
	}
	let budget_log = exhausted.path().join("xurl.log");
	let budget_xurl =
		fake_xurl(exhausted.path(), &budget_log, "decodexspace", "decodexspace", true);
	let budget_error = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(RUN_ID, "publish", None, "2026-07-27T12:02:00Z"),
		exhausted.path(),
		&budget_xurl,
	)
	.expect_err("monthly budget")
	.to_string();
	assert!(budget_error.contains("monthly X budget exhausted"), "{budget_error}");
	assert!(!fs::read_to_string(budget_log).unwrap_or_default().lines().any(|call| call == "post"));
}

#[cfg(unix)]
#[test]
fn high_level_uncertain_create_is_never_retried_after_restart() {
	let temp = tempfile::tempdir().expect("temporary directory");
	write_auth_contract(temp.path());
	let initial_staging = write_staging(temp.path(), "candidate.json", &candidate_draft("publish"));
	let initial =
		crate::record_social_candidate(&record_request(temp.path(), &initial_staging, RUN_ID))
			.expect("record initial candidate");
	assert_eq!(initial.status, "recorded");
	let log = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log, "decodexspace", "decodexspace", false);

	let _ = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(RUN_ID, "publish", None, "2026-07-27T12:02:00Z"),
		temp.path(),
		&xurl,
	)
	.expect_err("unknown create result");
	let restart = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(SECOND_RUN_ID, "publish", None, "2026-07-27T12:10:00Z"),
		temp.path(),
		&xurl,
	)
	.expect("unknown create must remain local to its lineage");
	assert_eq!(restart.status, "no_candidate");

	let mut unrelated = candidate_draft("publish");
	unrelated["slug"] = json!("unrelated-upstream-change");
	let unrelated_staging = write_staging(temp.path(), "unrelated.json", &unrelated);
	let unrelated = crate::record_social_candidate(&record_request(
		temp.path(),
		&unrelated_staging,
		THIRD_RUN_ID,
	))
	.expect("record unrelated candidate through production path");
	assert_eq!(unrelated.status, "recorded");
	let unrelated_path = PathBuf::from(&unrelated.path);

	let mut further = candidate_draft("publish");
	further["slug"] = json!("third-upstream-change");
	let further_staging = write_staging(temp.path(), "further.json", &further);
	let backpressure = crate::record_social_candidate(&record_request(
		temp.path(),
		&further_staging,
		FOURTH_RUN_ID,
	))
	.expect_err("the unrelated pending candidate restores backpressure")
	.to_string();
	assert!(backpressure.contains("still pending"), "{backpressure}");
	assert!(further_staging.exists());
	assert_eq!(
		fs::read_to_string(&log)
			.expect("xurl log before unrelated publication")
			.lines()
			.filter(|call| *call == "post")
			.count(),
		1
	);

	let valid_xurl = fake_xurl(temp.path(), &log, "decodexspace", "decodexspace", true);
	let published = crate::social_workflow::publish_next_with_test_binary(
		&publish_next_request(FIFTH_RUN_ID, "publish", None, "2026-07-28T12:10:00Z"),
		temp.path(),
		&valid_xurl,
	)
	.expect("unrelated candidate proceeds on a later day");
	assert_eq!(
		published.candidate_path.as_deref(),
		Some(unrelated_path.to_string_lossy().as_ref())
	);
	let calls = fs::read_to_string(log).expect("xurl log");
	assert_eq!(calls.lines().filter(|call| *call == "post").count(), 2);
	let original_attempt =
		crate::load_json(&temp.path().join("attempts/2026-07").join(format!("{RUN_ID}.json")))
			.expect("unknown create attempt");
	assert_eq!(
		original_attempt["calls"]
			.as_array()
			.expect("calls")
			.iter()
			.filter(|call| call["operation"] == "content_create")
			.count(),
		1
	);
	let attempts = crate::collect_json_files(&[temp.path().join("attempts")])
		.expect("publication attempts")
		.into_iter()
		.map(|path| crate::load_json(&path).expect("attempt"))
		.filter(|attempt| {
			attempt.get("schema").and_then(Value::as_str)
				== Some(crate::social_xurl::model::ATTEMPT_SCHEMA)
		})
		.map(|attempt| {
			serde_json::from_value::<crate::social_xurl::model::XurlAttempt>(attempt)
				.expect("typed publication attempt")
		})
		.collect::<Vec<_>>();
	assert_eq!(attempts.len(), 2);
	assert_ne!(attempts[0].publication_lineage_sha256, attempts[1].publication_lineage_sha256);
	assert!(attempts.iter().all(|attempt| {
		attempt.reserved_cost_ceiling_microusd
			<= crate::social_xurl::model::PUBLICATION_LINEAGE_BUDGET_MICROUSD
	}));
	let reserved = total_reserved_cost(temp.path());
	assert_eq!(reserved, 60_000);
	assert!(reserved <= crate::SOCIAL_MONTHLY_BUDGET_MICROUSD);
}

#[test]
fn cli_exposes_only_high_level_social_workflows() {
	for command in [
		"record-candidate",
		"publish-next",
		"observe-due",
		"probe-xurl",
		"refresh-pricing",
		"cost-report",
		"seal-xurl-auth",
	] {
		let mut args = vec!["decodex-publisher", "social", command];
		match command {
			"record-candidate" => args.extend(["--staging", "candidate.json", "--run-id", RUN_ID]),
			"publish-next" => args.extend(["--run-id", RUN_ID, "--decision", "publish"]),
			"observe-due" => args.extend(["--run-id", RUN_ID]),
			_ => {},
		}
		assert!(crate::cli::Cli::try_parse_from(args).is_ok(), "{command}");
	}
	assert!(crate::cli::Cli::try_parse_from(["decodex-publisher", "social", "unknown"]).is_err());
}

fn candidate_draft(worthiness: &str) -> Value {
	json!({
		"schema": "decodex/content-evidence/1",
		"slug": "openai-codex-pr-22414",
		"repo": "openai/codex",
		"channel": "x",
		"target_account": "decodexspace",
		"mode": "operator_impact",
		"priority": "high",
		"audience": "Codex operators",
		"candidate_text": [POST_TEXT],
		"source_refs": {"urls": [SOURCE_URL]},
		"source_kinds": {(SOURCE_URL): "official_codex"},
		"evidence_notes": ["The linked upstream change alters an app-server capability boundary."],
		"claims": [{
			"text": "The app-server exposes a typed capability check.",
			"evidence": SOURCE_URL,
			"confidence": "confirmed"
		}],
		"decision": {
			"worthiness": worthiness,
			"reason": if worthiness == "publish" {
				"The change has an operator-visible protocol consequence."
			} else {
				"The evidence does not justify a public update."
			}
		}
	})
}

fn candidate(worthiness: &str) -> Value {
	let mut candidate = candidate_draft(worthiness);
	crate::social_record::apply_publication_identity(&mut candidate).expect("publication identity");
	candidate
}

pub(crate) fn valid_social_candidate() -> Value {
	candidate("publish")
}

fn rebind_identity(candidate: &mut Value) {
	candidate["decision"].as_object_mut().expect("decision").remove("idempotency_key");
	crate::social_record::apply_publication_identity(candidate).expect("publication identity");
}

fn assert_social_error(candidate: &Value, expected: &str) {
	let errors = crate::social_validation::validate_social_artifact(candidate).errors;
	assert!(errors.iter().any(|error| error.contains(expected)), "{errors:?}");
}

fn write_candidate(root: &Path, name: &str, mut candidate: Value) -> PathBuf {
	rebind_identity(&mut candidate);
	let path = root.join("candidates").join(name);
	crate::write_new_json(&path, &candidate).expect("candidate write");
	path
}

fn write_staging(root: &Path, name: &str, value: &Value) -> PathBuf {
	let path = root.join("staging").join(name);
	crate::write_new_json(&path, value).expect("staging write");
	path
}

fn record_request(root: &Path, staging: &Path, run_id: &str) -> SocialRecordCandidateRequest {
	SocialRecordCandidateRequest {
		staging_path: staging.into(),
		staging_dir: root.join("staging"),
		candidates_dir: root.join("candidates"),
		posts_dir: root.join("posts"),
		attempts_dir: root.join("attempts"),
		locks_dir: root.join("locks"),
		run_id: run_id.into(),
	}
}

fn reserve_request(root: &Path, candidate: &Path, run_id: &str) -> SocialReservePublishRequest {
	SocialReservePublishRequest {
		candidate_path: candidate.into(),
		candidates_dir: root.join("candidates"),
		reserved_at: "2026-07-27T12:00:00Z".into(),
		expires_at: "2026-07-27T13:00:00Z".into(),
		day: "2026-07-27".into(),
		timezone: "UTC".into(),
		out_dir: root.join("reservations"),
		posts_dir: root.join("posts"),
		attempts_dir: root.join("attempts"),
		locks_dir: root.join("locks"),
		run_id: run_id.into(),
		daily_limit: 1,
		dry_run: false,
	}
}

fn publish_request(root: &Path, reservation: &Path, run_id: &str) -> SocialPublishXurlRequest {
	SocialPublishXurlRequest {
		reservation_path: reservation.into(),
		authorization_contract_path: write_auth_contract(root),
		reservations_dir: root.join("reservations"),
		candidates_dir: root.join("candidates"),
		posts_dir: root.join("posts"),
		attempts_dir: root.join("attempts"),
		locks_dir: root.join("locks"),
		run_id: run_id.into(),
		posted_at: "2026-07-27T12:02:00Z".into(),
		monthly_budget_microusd: 1_250_000,
	}
}

fn observe_request(
	root: &Path,
	post: &Path,
	run_id: &str,
	window: &str,
	observed_at: &str,
) -> SocialObserveXurlRequest {
	SocialObserveXurlRequest {
		run_id: run_id.into(),
		post_path: post.into(),
		authorization_contract_path: write_auth_contract(root),
		posts_dir: root.join("posts"),
		outcomes_dir: root.join("outcomes"),
		attempts_dir: root.join("attempts"),
		locks_dir: root.join("locks"),
		observed_at: observed_at.into(),
		window: window.into(),
		monthly_budget_microusd: 1_250_000,
	}
}

fn skip_request(root: &Path, candidate: &Path) -> SocialTerminalizeSkipRequest {
	SocialTerminalizeSkipRequest {
		candidate_path: candidate.into(),
		candidates_dir: root.join("candidates"),
		reservations_dir: root.join("reservations"),
		posts_dir: root.join("posts"),
		locks_dir: root.join("locks"),
		run_id: RUN_ID.into(),
		day: "2026-07-27".into(),
		timezone: "UTC".into(),
		daily_limit: 1,
		dry_run: false,
		reason: None,
	}
}

fn publish_next_request(
	run_id: &str,
	decision: &str,
	reason: Option<&str>,
	now: &str,
) -> SocialPublishNextRequest {
	let day = now.get(..10).expect("fixed RFC3339 date");
	SocialPublishNextRequest {
		run_id: run_id.into(),
		decision: decision.into(),
		reason: reason.map(str::to_owned),
		clock: SocialClock {
			now: now.into(),
			expires_at: format!("{day}T13:02:00Z"),
			day: day.into(),
		},
	}
}

fn write_auth_contract(root: &Path) -> PathBuf {
	let path = root.join("xurl-authorization-contract.json");
	if !path.exists() {
		crate::write_new_json(&path, &json!({
			"schema": "decodex/xurl-authorization-contract/1",
			"policy_id": "xurl-oauth-least-privilege/3",
			"target_account": "decodexspace",
			"xurl_app": "default",
			"required_operator_authorized_scopes": ["tweet.read", "users.read", "tweet.write", "offline.access"],
			"xurl_version": "1.3.1",
			"xurl_binary_sha256": "7b85a210009db7a3f2d6183684674441fbf81276f1101f73d36d0266ec9aa01e",
			"sealed_at": "2026-07-27T00:00:00Z"
		})).expect("authorization contract");
	}
	path
}

fn write_budget_attempt(root: &Path, index: u64) {
	let lineage = format!("{index:064x}");
	crate::write_new_json(
		&root.join("attempts/2026-07").join(format!("budget-{index}.json")),
		&json!({
			"schema": "decodex/xurl-publish-attempt/4",
			"run_id": format!("019fa400-1000-7000-8000-{index:012}"),
			"reservation_ref": format!("budget-reservation-{index}.json"),
			"candidate_ref": format!("budget-candidate-{index}.json"),
			"idempotency_key": format!("content-publication:{lineage}"),
			"publication_lineage_sha256": lineage,
			"billing_month": "2026-07",
			"target_account": "decodexspace",
			"status": "reserved",
			"created_at": "2026-07-01T00:00:00Z",
			"updated_at": "2026-07-01T00:00:00Z",
			"reserved_cost_ceiling_microusd": 30_000,
			"xurl_version": "1.3.1",
			"pricing_policy_id": "x-api-pay-per-usage/2026-07-27",
			"authorization_contract_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
			"calls": [],
			"verified_user_id": null,
			"post_id": null,
			"published_url": null
		}),
	).expect("budget attempt");
}

#[cfg(unix)]
fn assert_no_paid_xurl_calls(log: &Path) {
	let calls = fs::read_to_string(log).unwrap_or_default();
	assert!(
		!calls.lines().any(|call| { matches!(call, "/2/users/me" | "post" | "read") }),
		"unexpected paid xurl call: {calls}"
	);
}

fn total_reserved_cost(root: &Path) -> u64 {
	crate::collect_json_files(&[root.join("attempts")])
		.expect("attempts")
		.iter()
		.map(|path| crate::load_json(path).expect("attempt"))
		.map(|attempt| attempt["reserved_cost_ceiling_microusd"].as_u64().expect("cost"))
		.sum()
}

#[cfg(unix)]
fn assert_content_create_boundary(actual_now: &str, should_publish: bool) {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), "candidate.json", valid_social_candidate());
	let mut reserve = reserve_request(temp.path(), &candidate, RUN_ID);
	reserve.reserved_at = "2026-07-27T23:50:00Z".into();
	reserve.expires_at = "2026-07-28T00:50:00Z".into();
	let reservation = crate::reserve_social_publish(&reserve).expect("reservation");
	let log = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log, "decodexspace", "decodexspace", true);
	let mut publish = publish_request(temp.path(), Path::new(&reservation.path), RUN_ID);
	publish.posted_at = "2026-07-27T23:57:00Z".into();
	let actual_now =
		time::OffsetDateTime::parse(actual_now, &time::format_description::well_known::Rfc3339)
			.expect("actual UTC timestamp");
	let result = crate::social_clock::with_content_create_now_for_test(actual_now, || {
		crate::social_xurl::publish_with_test_binary(&publish, &xurl)
	});
	let calls = fs::read_to_string(&log).expect("xurl log");
	let attempt =
		crate::load_json(&temp.path().join("attempts/2026-07").join(format!("{RUN_ID}.json")))
			.expect("durable attempt");
	assert!(attempt["reserved_cost_ceiling_microusd"].as_u64().is_some_and(|cost| cost <= 60_000));
	if should_publish {
		assert_eq!(result.expect("safe create window").status, "published");
		assert_eq!(calls.lines().filter(|call| *call == "post").count(), 1);
		assert_eq!(attempt["status"], "published");
	} else {
		let error = result.expect_err("closed create window").to_string();
		assert!(error.contains("content create is closed"), "{error}");
		assert!(!calls.lines().any(|call| call == "post"));
		assert_eq!(attempt["status"], "identity_verified");
		assert!(
			!attempt["calls"]
				.as_array()
				.expect("attempt calls")
				.iter()
				.any(|call| call["operation"] == "content_create")
		);
	}
}

#[cfg(unix)]
fn fake_xurl(
	root: &Path,
	log: &Path,
	account_label: &str,
	identity: &str,
	valid_create: bool,
) -> PathBuf {
	use std::os::unix::fs::PermissionsExt as _;

	let path = root.join("xurl");
	let create = if valid_create {
		format!(r#"printf '%s\n' '{{"data":{{"id":"2000000000000000001","text":"{POST_TEXT}"}}}}'"#)
	} else {
		"printf '%s\n' '{\"data\":{\"text\":\"missing id\"}}'".into()
	};
	let script = format!(
		r#"#!/bin/sh
set -eu
if [ "$1" = "--version" ]; then
  echo "xurl version 1.3.1"
  exit 0
fi
echo "$3" >> '{log}'
if [ "$3" = "auth" ]; then
  echo "▸ default  [client_id: test]"
  echo "      oauth2: {account_label}"
elif [ "$3" = "/2/users/me" ]; then
  printf '%s\n' '{{"data":{{"id":"42","username":"{identity}"}}}}'
elif [ "$3" = "post" ]; then
  {create}
elif [ "$3" = "read" ]; then
  printf '%s\n' '{{"data":{{"id":"2000000000000000001","text":"{text}","author_id":"42","public_metrics":{{"impression_count":10,"like_count":1,"reply_count":0,"retweet_count":0,"bookmark_count":0}}}},"includes":{{"users":[{{"id":"42","username":"decodexspace"}}]}}}}'
else
  exit 2
fi
"#,
		log = log.display(),
		text = POST_TEXT,
	);
	fs::write(&path, script).expect("fake xurl");
	let mut permissions = fs::metadata(&path).expect("fake xurl metadata").permissions();
	permissions.set_mode(0o700);
	fs::set_permissions(&path, permissions).expect("fake xurl executable");
	path
}

#[cfg(unix)]
fn faultable_fake_xurl(root: &Path, log: &Path) -> PathBuf {
	use std::os::unix::fs::PermissionsExt as _;

	let path = root.join("xurl");
	let identity_failure = root.join("identity-command-failure");
	let wrong_account = root.join("wrong-account");
	let read_failure = root.join("read-failure");
	let script = format!(
		r#"#!/bin/sh
set -eu
if [ "$1" = "--version" ]; then
  echo "xurl version 1.3.1"
  exit 0
fi
echo "$3" >> '{log}'
if [ "$3" = "auth" ]; then
  echo "▸ default  [client_id: test]"
  echo "      oauth2: decodexspace"
elif [ "$3" = "/2/users/me" ]; then
  if [ -f '{identity_failure}' ]; then
    exit 1
  fi
  if [ -f '{wrong_account}' ]; then
    printf '%s\n' '{{"data":{{"id":"42","username":"hackink"}}}}'
  else
    printf '%s\n' '{{"data":{{"id":"42","username":"decodexspace"}}}}'
  fi
elif [ "$3" = "post" ]; then
  printf '%s\n' '{{"data":{{"id":"2000000000000000001","text":"{text}"}}}}'
elif [ "$3" = "read" ]; then
  if [ -f '{read_failure}' ]; then
    exit 1
  fi
  printf '%s\n' '{{"data":{{"id":"2000000000000000001","text":"{text}","author_id":"42","public_metrics":{{"impression_count":10,"like_count":1,"reply_count":0,"retweet_count":0,"bookmark_count":0}}}},"includes":{{"users":[{{"id":"42","username":"decodexspace"}}]}}}}'
else
  exit 2
fi
"#,
		log = log.display(),
		identity_failure = identity_failure.display(),
		wrong_account = wrong_account.display(),
		read_failure = read_failure.display(),
		text = POST_TEXT,
	);
	fs::write(&path, script).expect("faultable fake xurl");
	let mut permissions = fs::metadata(&path).expect("fake xurl metadata").permissions();
	permissions.set_mode(0o700);
	fs::set_permissions(&path, permissions).expect("fake xurl executable");
	path
}

#[cfg(unix)]
fn fake_xurl_with_initial_read_failures(root: &Path, log: &Path) -> PathBuf {
	fake_xurl_with_read_failure_count(root, log, 2)
}

#[cfg(unix)]
fn fake_xurl_with_read_failure_count(root: &Path, log: &Path, failure_count: u64) -> PathBuf {
	use std::os::unix::fs::PermissionsExt as _;

	let path = root.join("xurl");
	let counter = root.join("failed-read-count");
	let script = format!(
		r#"#!/bin/sh
set -eu
if [ "$1" = "--version" ]; then
  echo "xurl version 1.3.1"
  exit 0
fi
echo "$3" >> '{log}'
if [ "$3" = "auth" ]; then
  echo "▸ default  [client_id: test]"
  echo "      oauth2: decodexspace"
elif [ "$3" = "/2/users/me" ]; then
  printf '%s\n' '{{"data":{{"id":"42","username":"decodexspace"}}}}'
elif [ "$3" = "post" ]; then
  printf '%s\n' '{{"data":{{"id":"2000000000000000001","text":"{text}"}}}}'
elif [ "$3" = "read" ]; then
  count=0
  if [ -f '{counter}' ]; then
    count=$(cat '{counter}')
  fi
  count=$((count + 1))
  printf '%s\n' "$count" > '{counter}'
  if [ "$count" -le {failure_count} ]; then
    exit 1
  fi
  printf '%s\n' '{{"data":{{"id":"2000000000000000001","text":"{text}","author_id":"42","public_metrics":{{"impression_count":10,"like_count":1,"reply_count":0,"retweet_count":0,"bookmark_count":0}}}},"includes":{{"users":[{{"id":"42","username":"decodexspace"}}]}}}}'
else
  exit 2
fi
"#,
		log = log.display(),
		counter = counter.display(),
		text = POST_TEXT,
		failure_count = failure_count,
	);
	fs::write(&path, script).expect("transient fake xurl");
	let mut permissions = fs::metadata(&path).expect("fake xurl metadata").permissions();
	permissions.set_mode(0o700);
	fs::set_permissions(&path, permissions).expect("fake xurl executable");
	path
}
