use std::{
	fs,
	os::unix::fs::{PermissionsExt as _, symlink},
	path::{Path, PathBuf},
	time::{Duration as StdDuration, SystemTime},
};

use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
	GcFailure, GcJournalStep, GcPolicy, SocialGcRequest, digest_hex, gc_social_with,
	gc_social_with_hooks,
};
use crate::social_xurl::model::{XurlAttempt, XurlObservationAttempt};

const PUBLISH_RUN_ID: &str = "019fa400-0000-7000-8000-000000000001";
const OUTCOME_24H_RUN_ID: &str = "019fa400-0000-7000-8000-000000000002";
const OUTCOME_7D_RUN_ID: &str = "019fa400-0000-7000-8000-000000000003";
const MISMATCHED_PUBLISH_RUN_ID: &str = "019fa400-0000-7000-8000-000000000004";
const POST_ID: &str = "2000000000000000001";
const PUBLICATION_LINEAGE: &str =
	"e9efcaaa0b3eea16244c69fcffc22f97a21c0338f1071ee86d9b59cd9e2c1bd9";
const IDEMPOTENCY_KEY: &str =
	"radar-publication:e9efcaaa0b3eea16244c69fcffc22f97a21c0338f1071ee86d9b59cd9e2c1bd9";
const POST_TEXT: &str =
	"Codex app-server now exposes a typed capability check before experimental calls.";

#[derive(Clone, Copy)]
struct Timeline {
	day: &'static str,
	posted_at: &'static str,
	outcome_24h_at: &'static str,
	outcome_7d_at: &'static str,
	now: &'static str,
}

const OLD_TIMELINE: Timeline = Timeline {
	day: "2026-06-01",
	posted_at: "2026-06-01T12:00:00Z",
	outcome_24h_at: "2026-06-02T12:00:00Z",
	outcome_7d_at: "2026-06-08T12:00:00Z",
	now: "2026-07-01T12:00:00Z",
};

const CURRENT_MONTH_TIMELINE: Timeline = Timeline {
	day: "2026-08-01",
	posted_at: "2026-08-01T12:00:00Z",
	outcome_24h_at: "2026-08-02T12:00:00Z",
	outcome_7d_at: "2026-08-08T12:00:00Z",
	now: "2026-08-30T12:00:00Z",
};

struct Fixture {
	_temp: tempfile::TempDir,
	radar: RadarFixture,
	root: PathBuf,
	request: SocialGcRequest,
	candidate: PathBuf,
	reservation: PathBuf,
	post: PathBuf,
	outcome_24h: PathBuf,
	outcome_7d: PathBuf,
	publish_attempt: PathBuf,
	observation_24h_attempt: PathBuf,
	observation_7d_attempt: PathBuf,
}

struct RadarFixture {
	_temp: tempfile::TempDir,
	queue_ref: String,
	review_ref: String,
	impact_ref: String,
	queue_sha256: String,
	review_sha256: String,
	impact_sha256: String,
	lineage_sha256: String,
}

impl RadarFixture {
	fn create() -> Self {
		let repo_root = crate::repo_root().expect("repo root");
		let temp = crate::repo_local_test_directory("social-gc-radar-");
		let queue_dir = temp.path().join("github/review-queue");
		let pairs_dir = temp.path().join("github/content-review-pairs");
		crate::ensure_private_directory(&queue_dir).expect("private Radar queue collection");
		crate::ensure_private_directory(&pairs_dir).expect("private Radar pair collection");
		let queue_path = queue_dir.join("queue.json");
		let observed_at = OffsetDateTime::now_utc().format(&Rfc3339).expect("current timestamp");
		write(
			&queue_path,
			&json!({
				"schema": "upstream_review_queue/v1",
				"repo": "openai/codex",
				"generated_at": observed_at.as_str(),
				"source": {
					"upstream_head": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
				},
				"subjects": [{
					"subject_kind": "pr",
					"subject_id": "22414",
					"commit_shas": ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]
				}]
			}),
		);
		let review = json!({
			"schema": "upstream_review/v1",
			"slug": "openai-codex-pr-22414",
			"repo": "openai/codex",
			"upstream_head": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
			"subject": {
				"subject_kind": "pr",
				"subject_id": "22414",
				"commit_shas": ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]
			},
			"reviewed_at": observed_at.as_str(),
			"next_actions": [{"type": "upstream_impact"}]
		});
		let review_raw = pretty_json_bytes(&review);
		let review_sha256 = digest_hex(&review_raw);
		let impact = json!({
			"schema": "upstream_impact/v1",
			"slug": "openai-codex-pr-22414",
			"repo": "openai/codex",
			"reviewed_at": observed_at.as_str(),
			"review_lineage": {
				"artifact_sha256": review_sha256.as_str(),
				"slug": "openai-codex-pr-22414",
				"subject_kind": "pr",
				"subject_id": "22414",
				"upstream_head": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
				"commit_shas": ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]
			},
			"public_signal_decision": "publish",
			"publisher_angle": "operator_impact"
		});
		let impact_raw = pretty_json_bytes(&impact);
		let pair_digest = crate::social_record::radar_content_pair_sha256(&review_raw, &impact_raw);
		let pair_dir = pairs_dir.join(format!("gc-test--{pair_digest}"));
		crate::ensure_private_directory(&pair_dir).expect("private Radar pair directory");
		let review_path = pair_dir.join("review.json");
		let impact_path = pair_dir.join("impact.json");
		write(&review_path, &review);
		write(&impact_path, &impact);
		let queue_sha256 = crate::load_json_with_sha256(&queue_path).expect("Radar queue digest").1;
		let impact_sha256 =
			crate::load_json_with_sha256(&impact_path).expect("Radar impact digest").1;
		let queue_ref = crate::path_arg(&repo_root, &queue_path);
		let review_ref = crate::path_arg(&repo_root, &review_path);
		let impact_ref = crate::path_arg(&repo_root, &impact_path);
		let lineage_sha256 = crate::social_record::eligibility_lineage_sha256(
			"openai/codex",
			"pr",
			"22414",
			"openai-codex-pr-22414",
			"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
			&["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()],
			&queue_sha256,
			&review_sha256,
			&impact_sha256,
		);

		Self {
			_temp: temp,
			queue_ref,
			review_ref,
			impact_ref,
			queue_sha256,
			review_sha256,
			impact_sha256,
			lineage_sha256,
		}
	}
}

#[test]
fn complete_terminal_lineage_is_deleted_as_one_component() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	crate::validate_social(&[
		fixture.request.candidates_dir.clone(),
		fixture.request.reservations_dir.clone(),
		fixture.request.posts_dir.clone(),
		fixture.request.outcomes_dir.clone(),
	])
	.expect("fixture social artifacts");
	let now = OffsetDateTime::parse(OLD_TIMELINE.now, &Rfc3339).expect("now");
	let publish: XurlAttempt =
		serde_json::from_value(load(&fixture.publish_attempt)).expect("publish attempt shape");
	super::inventory::validate_publish_attempt(
		&publish,
		"2026-06",
		fixture.publish_attempt.file_name().expect("filename"),
		now,
	)
	.expect("publish attempt contract");
	for path in [&fixture.observation_24h_attempt, &fixture.observation_7d_attempt] {
		let observation: XurlObservationAttempt =
			serde_json::from_value(load(path)).expect("observation attempt shape");
		super::inventory::validate_observation_attempt(
			&observation,
			"2026-06",
			path.file_name().expect("filename"),
			now,
		)
		.expect("observation attempt contract");
	}

	let report = gc_social_with(&fixture.request, GcPolicy::default(), || {}).expect("social GC");

	assert_eq!(report.deleted_lineages, 1);
	assert_eq!(report.deleted_files, 8);
	assert_eq!(report.retained_lineages, 0);
	assert!(report.reason_codes.contains(&"terminal_lineages_pruned".into()));
	for path in [
		&fixture.candidate,
		&fixture.reservation,
		&fixture.post,
		&fixture.outcome_24h,
		&fixture.outcome_7d,
		&fixture.publish_attempt,
		&fixture.observation_24h_attempt,
		&fixture.observation_7d_attempt,
	] {
		assert!(!path.exists(), "{} should be deleted", path.display());
	}
	assert!(
		crate::social_xurl::publication_effect_conflict(
			&fixture.request.attempts_dir,
			PUBLICATION_LINEAGE,
			None,
		)
		.expect("durable public-effect lookup")
		.is_none()
	);
	let second =
		gc_social_with(&fixture.request, GcPolicy::default(), || {}).expect("idempotent second GC");
	assert_eq!(second.deleted_files, 0);
}

#[test]
fn mismatched_published_post_owner_fails_closed_without_gc_deletion() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	let mut post = load(&fixture.post);
	post["owner"]["run_id"] = json!(OUTCOME_24H_RUN_ID);
	rewrite(&fixture.post, &post);

	gc_social_with(&fixture.request, GcPolicy::default(), || {})
		.expect_err("mismatched published owner must make the store invalid");

	for path in fixture.lineage_files() {
		assert!(path.exists(), "{} must be retained", path.display());
	}
}

#[test]
fn mismatched_publish_attempt_owner_is_retained_by_gc_plan() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	let mut attempt = load(&fixture.publish_attempt);
	attempt["run_id"] = json!(MISMATCHED_PUBLISH_RUN_ID);
	let mismatched_attempt = fixture
		.publish_attempt
		.parent()
		.expect("attempt month")
		.join(format!("{MISMATCHED_PUBLISH_RUN_ID}.json"));
	write(&mismatched_attempt, &attempt);
	fs::remove_file(&fixture.publish_attempt).expect("remove original publish attempt");

	let report =
		gc_social_with(&fixture.request, GcPolicy::default(), || {}).expect("owner mismatch GC");

	assert_eq!(report.deleted_lineages, 0);
	assert_eq!(report.retained_lineages, 1);
	assert!(mismatched_attempt.exists());
	for path in [
		&fixture.candidate,
		&fixture.reservation,
		&fixture.post,
		&fixture.outcome_24h,
		&fixture.outcome_7d,
		&fixture.observation_24h_attempt,
		&fixture.observation_7d_attempt,
	] {
		assert!(path.exists(), "{} must be retained", path.display());
	}
}

#[test]
fn complete_quality_skip_lineage_is_deleted_as_one_component() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	fixture.remove_everything_except(&[&fixture.candidate]);
	let mut candidate = load(&fixture.candidate);
	candidate["decision"]["worthiness"] = json!("skip");
	candidate["decision"]["reason"] = json!("No material operator consequence.");
	rewrite(&fixture.candidate, &candidate);
	let skipped_post = fixture
		.request
		.posts_dir
		.join(format!("{}.json", crate::social_publish::idempotency_digest(IDEMPOTENCY_KEY)));
	write(
		&skipped_post,
		&skipped_post_payload(OLD_TIMELINE, &fixture.candidate_key(), &fixture.radar),
	);

	let report = gc_social_with(&fixture.request, GcPolicy::default(), || {}).expect("social GC");

	assert_eq!(report.deleted_lineages, 1);
	assert_eq!(report.deleted_files, 2);
	assert!(!fixture.candidate.exists());
	assert!(!skipped_post.exists());
}

#[test]
fn expired_strategy_is_pruned_before_lineage_reference_analysis() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	let old_strategy = fixture.write_strategy(
		"019fa400-0000-7000-8000-000000000010",
		"daily-2026-06-09",
		"2026-06-09T12:00:00Z",
		"2026-06-10T12:00:00Z",
		&fixture.post_key(),
	);
	let retained_strategy = fixture.write_strategy(
		"019fa400-0000-7000-8000-000000000011",
		"daily-2026-06-10",
		"2026-06-10T12:00:00Z",
		"2026-06-11T12:00:00Z",
		"https://github.com/openai/codex/releases",
	);
	let policy = GcPolicy { daily_strategy_keep: 1, ..GcPolicy::default() };

	let report = gc_social_with(&fixture.request, policy, || {}).expect("social GC");

	assert_eq!(report.deleted_strategies, 1);
	assert_eq!(report.deleted_lineages, 1);
	assert!(!old_strategy.exists());
	assert!(retained_strategy.exists());
}

#[test]
fn active_candidate_reservation_and_failed_post_are_preserved() {
	let candidate_only = Fixture::complete(OLD_TIMELINE);
	candidate_only.remove_everything_except(&[&candidate_only.candidate]);
	let report = gc_social_with(&candidate_only.request, GcPolicy::default(), || {})
		.expect("candidate-only GC");
	assert_eq!(report.deleted_lineages, 0);
	assert!(candidate_only.candidate.exists());

	let active = Fixture::complete(OLD_TIMELINE);
	active.remove_everything_except(&[&active.candidate, &active.reservation]);
	let mut reservation = load(&active.reservation);
	reservation["status"] = json!("active");
	reservation.as_object_mut().expect("object").remove("consumed_by_social_post");
	rewrite(&active.reservation, &reservation);
	let report = gc_social_with(&active.request, GcPolicy::default(), || {}).expect("active GC");
	assert_eq!(report.deleted_lineages, 0);
	assert!(active.reservation.exists());

	let failed = Fixture::complete(OLD_TIMELINE);
	failed.remove_everything_except(&[&failed.candidate, &failed.post]);
	let mut post = load(&failed.post);
	post["status"] = json!("failed");
	post["decision"]["daily_count_after"] = json!(0);
	post.as_object_mut().expect("object").remove("publication");
	post["failure"] = json!({"reason": "readback_failed", "details": "No trusted readback."});
	post["source_refs"].as_object_mut().expect("source refs").remove("reservations");
	rewrite(&failed.post, &post);
	let report =
		gc_social_with(&failed.request, GcPolicy::default(), || {}).expect("failed-post GC");
	assert_eq!(report.deleted_lineages, 0);
	assert!(failed.post.exists());
}

#[test]
fn failed_uncertain_and_inflight_attempts_preserve_the_lineage() {
	for state in ["halted", "create_uncertain", "create_inflight"] {
		let fixture = Fixture::complete(OLD_TIMELINE);
		let mut attempt = load(&fixture.publish_attempt);
		attempt["status"] = json!(state);
		attempt["calls"] = match state {
			"halted" => json!([call("identity_read", "failed", 10_000, None)]),
			"create_uncertain" => json!([
				call("identity_read", "succeeded", 10_000, Some('d')),
				call("content_create", "uncertain", 15_000, None),
			]),
			"create_inflight" => json!([
				call("identity_read", "succeeded", 10_000, Some('d')),
				call("content_create", "inflight", 15_000, None),
			]),
			_ => unreachable!(),
		};
		attempt["post_id"] = Value::Null;
		attempt["published_url"] = Value::Null;
		rewrite(&fixture.publish_attempt, &attempt);

		let report = gc_social_with(&fixture.request, GcPolicy::default(), || {})
			.expect("attempt must be retained");

		assert_eq!(report.deleted_lineages, 0, "{state}");
		assert!(fixture.candidate.exists(), "{state}");
		assert!(fixture.publish_attempt.exists(), "{state}");
	}
}

#[test]
fn legal_publication_and_observation_reconciliation_states_pass_gc() {
	let publication = Fixture::complete(OLD_TIMELINE);
	publication.remove_everything_except(&[
		&publication.candidate,
		&publication.reservation,
		&publication.publish_attempt,
	]);
	let mut reservation = load(&publication.reservation);
	reservation["status"] = json!("active");
	reservation.as_object_mut().expect("reservation").remove("consumed_by_social_post");
	rewrite(&publication.reservation, &reservation);
	let mut publish_attempt = load(&publication.publish_attempt);
	publish_attempt["status"] = json!("identity_reconciled");
	publish_attempt["reserved_cost_ceiling_microusd"] = json!(40_000);
	publish_attempt["calls"] = json!([
		call("identity_read", "uncertain", 10_000, None),
		recovery_call(
			"identity_read_reconcile",
			"succeeded",
			10_000,
			OUTCOME_24H_RUN_ID,
			"2026-06",
			'd'
		)
	]);
	publish_attempt["verified_user_id"] = Value::Null;
	publish_attempt["post_id"] = Value::Null;
	publish_attempt["published_url"] = Value::Null;
	rewrite(&publication.publish_attempt, &publish_attempt);
	let report = gc_social_with(&publication.request, GcPolicy::default(), || {})
		.expect("legal publication reconciliation state");
	assert_eq!(report.deleted_lineages, 0);
	assert!(publication.publish_attempt.exists());

	let observation = Fixture::complete(OLD_TIMELINE);
	fs::remove_file(&observation.outcome_7d).expect("remove 7d outcome");
	fs::remove_file(&observation.observation_7d_attempt).expect("remove 7d attempt");
	let mut observe_attempt = load(&observation.observation_24h_attempt);
	let recovery =
		recovery_call("outcome_read_reconcile", "succeeded", 5_000, PUBLISH_RUN_ID, "2026-06", 'c');
	observe_attempt["reserved_cost_ceiling_microusd"] = json!(10_000);
	observe_attempt["call"] = recovery.clone();
	observe_attempt["calls"] = json!([call("outcome_read", "uncertain", 5_000, None), recovery]);
	rewrite(&observation.observation_24h_attempt, &observe_attempt);
	let report = gc_social_with(&observation.request, GcPolicy::default(), || {})
		.expect("legal observation reconciliation state");
	assert_eq!(report.deleted_lineages, 0);
	assert!(observation.observation_24h_attempt.exists());
}

#[test]
fn malformed_reconciliation_cost_fails_gc_before_deletion() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	fs::remove_file(&fixture.outcome_7d).expect("remove 7d outcome");
	fs::remove_file(&fixture.observation_7d_attempt).expect("remove 7d attempt");
	let mut attempt = load(&fixture.observation_24h_attempt);
	let recovery =
		recovery_call("outcome_read_reconcile", "succeeded", 6_000, PUBLISH_RUN_ID, "2026-06", 'c');
	attempt["reserved_cost_ceiling_microusd"] = json!(11_000);
	attempt["call"] = recovery.clone();
	attempt["calls"] = json!([call("outcome_read", "uncertain", 5_000, None), recovery]);
	rewrite(&fixture.observation_24h_attempt, &attempt);

	let error = gc_social_with(&fixture.request, GcPolicy::default(), || {})
		.expect_err("malformed recovery cost must fail closed");
	assert_eq!(error.0, "social_gc_scan_invalid");
	assert!(fixture.candidate.exists());
	assert!(fixture.observation_24h_attempt.exists());
}

#[test]
fn malformed_reconciliation_sequence_fails_gc_before_deletion() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	fs::remove_file(&fixture.outcome_7d).expect("remove 7d outcome");
	fs::remove_file(&fixture.observation_7d_attempt).expect("remove 7d attempt");
	let mut attempt = load(&fixture.observation_24h_attempt);
	let first_recovery =
		recovery_call("outcome_read_reconcile", "succeeded", 5_000, PUBLISH_RUN_ID, "2026-06", 'c');
	let second_recovery = recovery_call(
		"outcome_read_reconcile",
		"succeeded",
		5_000,
		OUTCOME_7D_RUN_ID,
		"2026-06",
		'd',
	);
	attempt["reserved_cost_ceiling_microusd"] = json!(15_000);
	attempt["call"] = second_recovery.clone();
	attempt["calls"] =
		json!([call("outcome_read", "uncertain", 5_000, None), first_recovery, second_recovery]);
	rewrite(&fixture.observation_24h_attempt, &attempt);

	let error = gc_social_with(&fixture.request, GcPolicy::default(), || {})
		.expect_err("a recovery cannot follow an already successful recovery");
	assert_eq!(error.0, "social_gc_scan_invalid");
	assert!(fixture.candidate.exists());
	assert!(fixture.observation_24h_attempt.exists());
}

#[test]
fn missing_outcome_window_preserves_the_lineage() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	fs::remove_file(&fixture.outcome_7d).expect("remove 7d outcome");
	fs::remove_file(&fixture.observation_7d_attempt).expect("remove 7d attempt");

	let report = gc_social_with(&fixture.request, GcPolicy::default(), || {}).expect("social GC");

	assert_eq!(report.deleted_lineages, 0);
	assert!(fixture.post.exists());
}

#[test]
fn retained_strategy_preserves_the_lineage() {
	let strategy_fixture = Fixture::complete(OLD_TIMELINE);
	strategy_fixture.write_strategy(
		"019fa400-0000-7000-8000-000000000010",
		"daily-2026-06-20",
		"2026-06-20T12:00:00Z",
		"2026-06-21T12:00:00Z",
		&strategy_fixture.post_key(),
	);
	let report = gc_social_with(&strategy_fixture.request, GcPolicy::default(), || {})
		.expect("strategy-protected GC");
	assert_eq!(report.deleted_lineages, 0);
	assert!(report.reason_codes.contains(&"strategy_reference_retained".into()));
}

#[test]
fn current_billing_month_usage_preserves_the_whole_lineage() {
	let fixture = Fixture::complete(CURRENT_MONTH_TIMELINE);

	let report = gc_social_with(&fixture.request, GcPolicy::default(), || {}).expect("social GC");

	assert_eq!(report.deleted_lineages, 0);
	assert!(report.reason_codes.contains(&"current_billing_month_retained".into()));
	for path in fixture.lineage_files() {
		assert!(path.exists());
	}
}

#[test]
fn current_month_recovery_charge_is_reported_for_an_incomplete_older_lineage() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	fs::remove_file(&fixture.outcome_7d).expect("remove later outcome");
	fs::remove_file(&fixture.observation_7d_attempt).expect("remove later observation attempt");
	let mut attempt = load(&fixture.observation_24h_attempt);
	let recovery =
		recovery_call("outcome_read_reconcile", "succeeded", 5_000, PUBLISH_RUN_ID, "2026-07", 'c');
	attempt["reserved_cost_ceiling_microusd"] = json!(10_000);
	attempt["call"] = recovery.clone();
	attempt["calls"] = json!([call("outcome_read", "uncertain", 5_000, None), recovery]);
	rewrite(&fixture.observation_24h_attempt, &attempt);

	let report = gc_social_with(&fixture.request, GcPolicy::default(), || {})
		.expect("current-month recovery charge must protect the lineage");

	assert_eq!(report.deleted_lineages, 0);
	assert!(report.reason_codes.contains(&"current_billing_month_retained".into()));
	assert!(report.reason_codes.contains(&"nonterminal_lineage_retained".into()));
	for path in [
		&fixture.candidate,
		&fixture.reservation,
		&fixture.post,
		&fixture.outcome_24h,
		&fixture.publish_attempt,
		&fixture.observation_24h_attempt,
	] {
		assert!(path.exists());
	}
}

#[test]
fn malformed_component_fails_before_any_file_is_deleted() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	fs::write(&fixture.outcome_7d, b"{not-json\n").expect("corrupt outcome");
	fs::set_permissions(&fixture.outcome_7d, fs::Permissions::from_mode(0o600))
		.expect("private mode");

	let error = gc_social_with(&fixture.request, GcPolicy::default(), || {})
		.expect_err("malformed state must fail");

	assert_eq!(error.0, "social_gc_scan_invalid");
	assert!(fixture.candidate.exists());
	assert!(fixture.reservation.exists());
	assert!(fixture.post.exists());
}

#[test]
fn unknown_artifact_schema_fails_before_any_file_is_deleted() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	let mut outcome = load(&fixture.outcome_7d);
	outcome["schema"] = json!("social_outcome/v999");
	rewrite(&fixture.outcome_7d, &outcome);

	let error = gc_social_with(&fixture.request, GcPolicy::default(), || {})
		.expect_err("unknown schema must fail");

	assert_eq!(error.0, "social_gc_scan_invalid");
	assert!(fixture.candidate.exists());
	assert!(fixture.reservation.exists());
	assert!(fixture.post.exists());
}

#[test]
fn cross_pair_radar_sources_fail_gc_before_deletion() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	let repo_root = crate::repo_root().expect("repo root");
	let review_path = repo_root.join(&fixture.radar.review_ref);
	let impact_path = repo_root.join(&fixture.radar.impact_ref);
	let pair_dir = review_path.parent().expect("pair directory");
	let digest = pair_dir
		.file_name()
		.and_then(|name| name.to_str())
		.and_then(|name| name.rsplit_once("--"))
		.map(|(_, digest)| digest)
		.expect("pair digest");
	let alternate =
		pair_dir.parent().expect("pair collection").join(format!("alternate--{digest}"));
	crate::ensure_private_directory(&alternate).expect("alternate pair directory");
	write(&alternate.join("review.json"), &load(&review_path));
	write(&alternate.join("impact.json"), &load(&impact_path));
	let alternate_impact_ref = key(&alternate.join("impact.json"));
	let mut candidate = load(&fixture.candidate);
	candidate["radar_source_refs"]["impact"] = json!(alternate_impact_ref.as_str());
	candidate["source_refs"]["upstream_impacts"] = json!([alternate_impact_ref.as_str()]);
	let digests = candidate["evidence_digests"].as_object_mut().expect("evidence digests");
	digests.remove(&fixture.radar.impact_ref);
	digests.insert(alternate_impact_ref, json!(fixture.radar.impact_sha256.as_str()));
	rewrite(&fixture.candidate, &candidate);

	let error = gc_social_with(&fixture.request, GcPolicy::default(), || {})
		.expect_err("GC must reject review and impact from different canonical pairs");
	assert_eq!(error.0, "social_gc_scan_invalid");
	assert!(fixture.candidate.exists());
	assert!(fixture.publish_attempt.exists());
}

#[test]
fn symlink_entry_fails_closed_without_deleting_other_members() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	let original = fixture.root.join("candidate-original.json");
	fs::rename(&fixture.candidate, &original).expect("move candidate");
	symlink(&original, &fixture.candidate).expect("candidate symlink");

	let error = gc_social_with(&fixture.request, GcPolicy::default(), || {})
		.expect_err("symlink must fail");

	assert_eq!(error.0, "social_gc_scan_invalid");
	assert!(original.exists());
	assert!(fixture.post.exists());
}

#[test]
fn parent_replacement_race_fails_before_component_deletion() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	let original_candidates = fixture.root.join("candidates");
	let retained_candidates = fixture.root.join("retained-candidates");
	let outside = fixture.root.join("outside");
	fs::create_dir(&outside).expect("outside directory");
	fs::set_permissions(&outside, fs::Permissions::from_mode(0o700)).expect("outside mode");

	let error = gc_social_with(&fixture.request, GcPolicy::default(), || {
		fs::rename(&original_candidates, &retained_candidates).expect("move candidates");
		symlink(&outside, &original_candidates).expect("replace candidates path");
	})
	.expect_err("parent replacement must fail");

	assert_eq!(error.0, "social_gc_delete_race");
	assert!(retained_candidates.join(format!("{PUBLISH_RUN_ID}.json")).exists());
	assert!(fixture.post.exists());
	assert!(fs::read_dir(outside).expect("outside read").next().is_none());
}

#[test]
fn file_replacement_race_is_detected_before_component_deletion() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	let replacement = fixture.root.join("replacement.json");
	write(&replacement, &json!({"safe": true}));
	let candidate = fixture.candidate.clone();
	let outcome = fixture.outcome_7d.clone();

	let error = gc_social_with(&fixture.request, GcPolicy::default(), || {
		fs::remove_file(&outcome).expect("remove scanned outcome");
		symlink(&replacement, &outcome).expect("replace outcome");
	})
	.expect_err("replacement must fail preflight");

	assert_eq!(error.0, "social_gc_delete_race");
	assert!(candidate.exists());
	assert!(fixture.reservation.exists());
	assert!(fixture.post.exists());
}

#[test]
fn hard_entry_and_byte_bounds_fail_before_deletion() {
	let entry_fixture = Fixture::complete(OLD_TIMELINE);
	let entry_policy = GcPolicy { max_entries: 2, ..GcPolicy::default() };
	let error =
		gc_social_with(&entry_fixture.request, entry_policy, || {}).expect_err("entry limit");
	assert_eq!(error.0, "social_gc_scan_invalid");
	assert!(entry_fixture.candidate.exists());

	let byte_fixture = Fixture::complete(OLD_TIMELINE);
	let byte_policy = GcPolicy { max_bytes: 1, ..GcPolicy::default() };
	let error = gc_social_with(&byte_fixture.request, byte_policy, || {}).expect_err("byte limit");
	assert_eq!(error.0, "social_gc_scan_invalid");
	assert!(byte_fixture.candidate.exists());
}

#[test]
fn private_directory_enumeration_stops_at_the_requested_bound() {
	let temp = tempfile::tempdir().expect("temporary directory");
	fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).expect("private mode");
	for name in ["a.json", "b.json", "c.json"] {
		let path = temp.path().join(name);
		fs::write(&path, b"{}\n").expect("bounded entry");
		fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("entry mode");
	}
	let directory = crate::open_existing_exact_private_directory(temp.path())
		.expect("open private directory")
		.expect("private directory exists");

	assert!(directory.entries_bounded(2).is_err());
	assert_eq!(directory.entries_bounded(3).expect("exact bound").len(), 3);
}

#[test]
fn journal_temporary_files_are_cleaned_with_strict_bounded_matching() {
	let fixture = Fixture::complete(CURRENT_MONTH_TIMELINE);
	fs::create_dir_all(&fixture.request.locks_dir).expect("locks directory");
	fs::set_permissions(&fixture.request.locks_dir, fs::Permissions::from_mode(0o700))
		.expect("locks mode");
	let temporary = fixture
		.request
		.locks_dir
		.join(".social-gc-journal.json.0123456789abcdef0123456789abcdef.tmp");
	let unknown = fixture.request.locks_dir.join(".social-gc-journal.json.operator.tmp");
	for path in [&temporary, &unknown] {
		fs::write(path, b"{\"interrupted\":true}\n").expect("temporary journal");
		fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("temporary mode");
	}

	gc_social_with(&fixture.request, GcPolicy::default(), || {}).expect("temporary recovery");

	assert!(!temporary.exists());
	assert!(unknown.exists());
}

#[test]
fn malformed_recognized_journal_temporary_file_fails_closed() {
	for drift in ["mode", "future"] {
		let fixture = Fixture::complete(CURRENT_MONTH_TIMELINE);
		fs::create_dir_all(&fixture.request.locks_dir).expect("locks directory");
		fs::set_permissions(&fixture.request.locks_dir, fs::Permissions::from_mode(0o700))
			.expect("locks mode");
		let temporary = fixture
			.request
			.locks_dir
			.join(".social-gc-journal.json.0123456789abcdef0123456789abcdef.tmp");
		fs::write(&temporary, b"{}\n").expect("temporary journal");
		fs::set_permissions(
			&temporary,
			fs::Permissions::from_mode(if drift == "mode" { 0o640 } else { 0o600 }),
		)
		.expect("temporary mode");
		if drift == "future" {
			let file = fs::File::open(&temporary).expect("open temporary");
			let times = fs::FileTimes::new()
				.set_modified(SystemTime::now() + StdDuration::from_secs(10 * 60));
			file.set_times(times).expect("future temporary timestamp");
		}

		let error = gc_social_with(&fixture.request, GcPolicy::default(), || {})
			.expect_err("unsafe recognized temporary must stop recovery");

		assert_eq!(error.0, "social_gc_recovery_failed", "{drift}");
		assert!(temporary.exists(), "{drift}");
	}
}

#[test]
fn durable_journal_recovers_at_every_effect_boundary_and_preserves_unknown_files() {
	let reference = Fixture::complete(OLD_TIMELINE);
	let mut steps = Vec::new();
	gc_social_with_hooks(&reference.request, GcPolicy::default(), || {}, &mut |step| {
		steps.push(step);
		Ok(())
	})
	.expect("reference GC");
	assert_eq!(
		steps
			.iter()
			.filter(|step| matches!(step, GcJournalStep::BeforePlannedFileUnlink(_)))
			.count(),
		8
	);
	assert_eq!(
		steps
			.iter()
			.filter(|step| matches!(step, GcJournalStep::AfterPlannedFileUnlink(_)))
			.count(),
		8
	);
	assert_eq!(
		steps
			.iter()
			.filter(|step| matches!(step, GcJournalStep::BeforeDataDirectorySync(_)))
			.count(),
		5
	);
	assert_eq!(
		steps
			.iter()
			.filter(|step| matches!(step, GcJournalStep::AfterDataDirectorySync(_)))
			.count(),
		5
	);
	for required in [
		GcJournalStep::BeforeJournalFileSync,
		GcJournalStep::AfterJournalFileSync,
		GcJournalStep::BeforeJournalPublish,
		GcJournalStep::AfterJournalPublish,
		GcJournalStep::BeforeJournalPublishDirectorySync,
		GcJournalStep::AfterJournalPublishDirectorySync,
		GcJournalStep::BeforeJournalUnlink,
		GcJournalStep::AfterJournalUnlink,
		GcJournalStep::BeforeJournalRemovalDirectorySync,
		GcJournalStep::AfterJournalRemovalDirectorySync,
	] {
		assert!(steps.contains(&required), "missing fault boundary {required:?}");
	}

	for fail_at in 0..steps.len() {
		let fixture = Fixture::complete(OLD_TIMELINE);
		let unknown = fixture.request.locks_dir.join("operator-owned.json");
		fs::create_dir_all(&fixture.request.locks_dir).expect("locks directory");
		fs::set_permissions(&fixture.request.locks_dir, fs::Permissions::from_mode(0o700))
			.expect("locks mode");
		fs::write(&unknown, b"{\"owner\":\"operator\"}\n").expect("unknown lock file");
		fs::set_permissions(&unknown, fs::Permissions::from_mode(0o600))
			.expect("unknown file mode");
		let mut index = 0;
		let error =
			gc_social_with_hooks(&fixture.request, GcPolicy::default(), || {}, &mut |_step| {
				let current = index;
				index += 1;
				if current == fail_at { Err(GcFailure("social_gc_test_fault")) } else { Ok(()) }
			})
			.expect_err("fault must interrupt GC");
		assert_eq!(error.0, "social_gc_test_fault", "fault boundary {fail_at}");

		gc_social_with(&fixture.request, GcPolicy::default(), || {}).unwrap_or_else(|error| {
			panic!("restart must recover and finish at boundary {fail_at}: {error:?}")
		});
		for path in [
			&fixture.candidate,
			&fixture.reservation,
			&fixture.post,
			&fixture.outcome_24h,
			&fixture.outcome_7d,
			&fixture.publish_attempt,
			&fixture.observation_24h_attempt,
			&fixture.observation_7d_attempt,
		] {
			assert!(!path.exists(), "{} survived boundary {fail_at}", path.display());
		}
		assert!(unknown.exists(), "unknown file was deleted at boundary {fail_at}");
		assert!(
			!fixture.request.locks_dir.join("social-gc-journal.json").exists(),
			"journal survived boundary {fail_at}"
		);
	}
}

#[test]
fn journal_recovery_rejects_same_content_with_a_different_file_identity() {
	let fixture = Fixture::complete(OLD_TIMELINE);
	interrupt_after_journal_publish(&fixture);
	let candidate = load(&fixture.candidate);
	rewrite(&fixture.candidate, &candidate);

	let error = gc_social_with(&fixture.request, GcPolicy::default(), || {})
		.expect_err("replacement identity must block recovery");

	assert_eq!(error.0, "social_gc_recovery_failed");
	assert!(fixture.candidate.exists());
	assert!(fixture.post.exists());
	assert!(fixture.request.locks_dir.join("social-gc-journal.json").exists());
}

#[test]
fn journal_recovery_rejects_replaced_or_missing_parent_directories() {
	for replacement in ["empty_directory", "missing"] {
		let fixture = Fixture::complete(OLD_TIMELINE);
		interrupt_after_journal_publish(&fixture);
		let retained = fixture.root.join(format!("retained-candidates-{replacement}"));
		fs::rename(&fixture.request.candidates_dir, &retained).expect("retain original parent");
		if replacement == "empty_directory" {
			fs::create_dir(&fixture.request.candidates_dir).expect("replacement parent");
			fs::set_permissions(&fixture.request.candidates_dir, fs::Permissions::from_mode(0o700))
				.expect("replacement parent mode");
		}

		let error = gc_social_with(&fixture.request, GcPolicy::default(), || {})
			.expect_err("changed parent identity must block recovery");

		assert_eq!(error.0, "social_gc_recovery_failed", "{replacement}");
		assert!(retained.join(format!("{PUBLISH_RUN_ID}.json")).exists(), "{replacement}");
		assert!(fixture.request.locks_dir.join("social-gc-journal.json").exists(), "{replacement}");
	}
}

fn interrupt_after_journal_publish(fixture: &Fixture) {
	let error = gc_social_with_hooks(&fixture.request, GcPolicy::default(), || {}, &mut |step| {
		if step == GcJournalStep::AfterJournalPublish {
			Err(GcFailure("social_gc_test_fault"))
		} else {
			Ok(())
		}
	})
	.expect_err("fault after journal publication");
	assert_eq!(error.0, "social_gc_test_fault");
}

impl Fixture {
	fn complete(timeline: Timeline) -> Self {
		let temp = tempfile::tempdir().expect("temporary directory");
		let radar = RadarFixture::create();
		let root = temp.path().to_path_buf();
		let candidate = root.join("candidates").join(format!("{PUBLISH_RUN_ID}.json"));
		let reservation = root
			.join("reservations")
			.join(timeline.day)
			.join(format!("{}.json", crate::social_publish::idempotency_digest(IDEMPOTENCY_KEY)));
		let post = root.join("posts").join(format!("{PUBLISH_RUN_ID}.json"));
		let outcome_24h = root.join("outcomes").join(format!("{OUTCOME_24H_RUN_ID}.json"));
		let outcome_7d = root.join("outcomes").join(format!("{OUTCOME_7D_RUN_ID}.json"));
		let month = &timeline.day[..7];
		let publish_attempt =
			root.join("attempts").join(month).join(format!("{PUBLISH_RUN_ID}.json"));
		let candidate_key = key(&candidate);
		let reservation_key = key(&reservation);
		let post_key = key(&post);
		let published_url = format!("https://x.com/decodexspace/status/{POST_ID}");

		write(&candidate, &candidate_payload(&radar));
		write(&reservation, &reservation_payload(timeline, &candidate_key, &post_key));
		write(
			&post,
			&post_payload(timeline, &candidate_key, &reservation_key, &published_url, &radar),
		);
		write(
			&outcome_24h,
			&outcome_payload(
				"24h",
				OUTCOME_24H_RUN_ID,
				timeline.outcome_24h_at,
				&post_key,
				&published_url,
				'c',
			),
		);
		write(
			&outcome_7d,
			&outcome_payload(
				"7d",
				OUTCOME_7D_RUN_ID,
				timeline.outcome_7d_at,
				&post_key,
				&published_url,
				'e',
			),
		);
		write(
			&publish_attempt,
			&publish_attempt_payload(
				timeline,
				month,
				&candidate_key,
				&reservation_key,
				&published_url,
			),
		);
		let observation_24h_attempt = write_observation_attempt(
			&root,
			month,
			"24h",
			OUTCOME_24H_RUN_ID,
			timeline.outcome_24h_at,
			&post_key,
			'c',
		);
		let observation_7d_attempt = write_observation_attempt(
			&root,
			month,
			"7d",
			OUTCOME_7D_RUN_ID,
			timeline.outcome_7d_at,
			&post_key,
			'e',
		);
		let request = SocialGcRequest {
			candidates_dir: root.join("candidates"),
			reservations_dir: root.join("reservations"),
			posts_dir: root.join("posts"),
			outcomes_dir: root.join("outcomes"),
			attempts_dir: root.join("attempts"),
			strategies_dir: root.join("strategies"),
			locks_dir: root.join("locks"),
			now: timeline.now.into(),
		};

		Self {
			_temp: temp,
			radar,
			root,
			request,
			candidate,
			reservation,
			post,
			outcome_24h,
			outcome_7d,
			publish_attempt,
			observation_24h_attempt,
			observation_7d_attempt,
		}
	}

	fn lineage_files(&self) -> Vec<&Path> {
		vec![
			&self.candidate,
			&self.reservation,
			&self.post,
			&self.outcome_24h,
			&self.outcome_7d,
			&self.publish_attempt,
			&self.observation_24h_attempt,
			&self.observation_7d_attempt,
		]
	}

	fn remove_everything_except(&self, retained: &[&PathBuf]) {
		for path in self.lineage_files() {
			if !retained.iter().any(|candidate| candidate.as_path() == path) {
				fs::remove_file(path).expect("remove fixture file");
			}
		}
	}

	fn candidate_key(&self) -> String {
		key(&self.candidate)
	}

	fn post_key(&self) -> String {
		key(&self.post)
	}

	fn write_strategy(
		&self,
		run_id: &str,
		cycle_key: &str,
		reviewed_at: &str,
		next_review_at: &str,
		evidence_ref: &str,
	) -> PathBuf {
		let path = self.request.strategies_dir.join(format!("{run_id}.json"));
		write(
			&path,
			&json!({
				"schema": "social_strategy/v1",
				"cycle_key": cycle_key,
				"cadence": "daily",
				"reviewed_at": reviewed_at,
				"evidence_refs": [evidence_ref],
				"decisions": [{
					"dimension": "no_change",
					"key": "quality_gate",
					"previous_value": "strict",
					"next_value": "strict",
					"reason": "No evidence supports a threshold change."
				}],
				"guardrails": {
					"evidence_gate": "unchanged",
					"privacy_gate": "unchanged",
					"idempotency_gate": "unchanged",
					"account_gate": "unchanged",
					"publication_gate": "unchanged"
				},
				"next_review_at": next_review_at
			}),
		);
		path
	}
}

fn candidate_payload(radar: &RadarFixture) -> Value {
	json!({
		"schema": "social_candidate/v1",
		"slug": "openai-codex-pr-22414",
		"repo": "openai/codex",
		"channel": "x",
		"target_account": "decodexspace",
		"mode": "operator_impact",
		"priority": "high",
		"audience": "Codex operators",
		"candidate_text": [POST_TEXT],
		"text_segments": [{
			"kind": "claim",
			"claim_index": 0
		}],
		"radar_eligibility": {
			"schema": "radar_content_eligibility/v1",
			"repo": "openai/codex",
			"subject_kind": "pr",
			"subject_id": "22414",
			"slug": "openai-codex-pr-22414",
			"upstream_head": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
			"commit_shas": ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
			"queue_sha256": radar.queue_sha256.as_str(),
			"review_sha256": radar.review_sha256.as_str(),
			"impact_sha256": radar.impact_sha256.as_str(),
			"lineage_sha256": radar.lineage_sha256.as_str()
		},
		"radar_source_refs": {
			"queue": radar.queue_ref.as_str(),
			"review": radar.review_ref.as_str(),
			"impact": radar.impact_ref.as_str()
		},
		"source_refs": {
			"upstream_reviews": [radar.review_ref.as_str()],
			"upstream_impacts": [radar.impact_ref.as_str()]
		},
		"evidence_digests": {
			(radar.review_ref.as_str()): radar.review_sha256.as_str(),
			(radar.impact_ref.as_str()): radar.impact_sha256.as_str()
		},
		"evidence_notes": ["PR #22414 changes an app-server capability boundary."],
		"claims": [{
			"text": POST_TEXT,
			"evidence": radar.review_ref.as_str(),
			"confidence": "confirmed"
		}],
		"decision": {
			"worthiness": "publish",
			"reason": "The change affects app-server clients.",
			"idempotency_key": IDEMPOTENCY_KEY
		},
		"caveats": [],
		"next_steps": []
	})
}

fn skipped_post_payload(timeline: Timeline, candidate_ref: &str, radar: &RadarFixture) -> Value {
	json!({
		"schema": "social_post/v1",
			"slug": "openai-codex-pr-22414",
		"channel": "x",
		"target_account": "decodexspace",
		"owner": {
			"automation_id": "decodex-xurl-publisher",
			"run_id": PUBLISH_RUN_ID
		},
		"mode": "operator_impact",
		"status": "skipped",
		"audience": "Codex operators",
		"text": [POST_TEXT],
			"source_refs": {
				"social_candidates": [candidate_ref],
				"upstream_reviews": [radar.review_ref.as_str()],
				"upstream_impacts": [radar.impact_ref.as_str()]
			},
			"evidence_digests": {
				(radar.review_ref.as_str()): radar.review_sha256.as_str(),
				(radar.impact_ref.as_str()): radar.impact_sha256.as_str()
			},
			"evidence_notes": ["PR #22414 changes an app-server capability boundary."],
			"claims": [{
				"text": POST_TEXT,
				"evidence": radar.review_ref.as_str(),
			"confidence": "confirmed"
		}],
		"decision": {
			"worthiness": "skip",
			"priority": "high",
			"idempotency_key": IDEMPOTENCY_KEY,
			"reason": "No material operator consequence.",
			"daily_limit": 1,
			"daily_count_before": 0,
			"daily_count_after": 0,
			"day": timeline.day,
			"timezone": "UTC"
		},
		"skip": {"reason": "No material operator consequence."}
	})
}

fn reservation_payload(timeline: Timeline, candidate_ref: &str, post_ref: &str) -> Value {
	json!({
		"schema": "social_publish_reservation/v1",
			"slug": "openai-codex-pr-22414",
		"channel": "x",
		"target_account": "decodexspace",
		"mode": "operator_impact",
			"status": "consumed",
			"idempotency_key": IDEMPOTENCY_KEY,
			"publication_lineage_sha256": PUBLICATION_LINEAGE,
		"reserved_at": format!("{}T11:55:00Z", timeline.day),
		"expires_at": format!("{}T13:00:00Z", timeline.day),
		"day": timeline.day,
		"timezone": "UTC",
		"candidate_refs": {"social_candidates": [candidate_ref]},
		"duplicate_keys": ["gc-test", IDEMPOTENCY_KEY],
		"owner": {
			"automation_id": "decodex-xurl-publisher",
			"run_id": PUBLISH_RUN_ID
		},
		"consumed_by_social_post": post_ref
	})
}

fn post_payload(
	timeline: Timeline,
	candidate_ref: &str,
	reservation_ref: &str,
	published_url: &str,
	radar: &RadarFixture,
) -> Value {
	json!({
		"schema": "social_post/v1",
		"slug": "openai-codex-pr-22414",
		"channel": "x",
		"target_account": "decodexspace",
		"owner": {
			"automation_id": "decodex-xurl-publisher",
			"run_id": PUBLISH_RUN_ID
		},
		"mode": "operator_impact",
		"status": "published",
		"audience": "Codex operators",
		"text": [POST_TEXT],
			"source_refs": {
				"reservations": [reservation_ref],
				"social_candidates": [candidate_ref],
				"upstream_reviews": [radar.review_ref.as_str()],
				"upstream_impacts": [radar.impact_ref.as_str()]
			},
			"evidence_digests": {
				(radar.review_ref.as_str()): radar.review_sha256.as_str(),
				(radar.impact_ref.as_str()): radar.impact_sha256.as_str()
			},
			"evidence_notes": ["PR #22414 changes an app-server capability boundary."],
			"claims": [{
				"text": POST_TEXT,
				"evidence": radar.review_ref.as_str(),
			"confidence": "confirmed"
		}],
		"decision": {
			"worthiness": "publish",
			"priority": "high",
			"idempotency_key": IDEMPOTENCY_KEY,
			"reason": "The change affects app-server clients.",
			"daily_limit": 1,
			"daily_count_before": 0,
			"daily_count_after": 1,
			"day": timeline.day,
			"timezone": "UTC"
		},
		"publication": {
			"posted_at": timeline.posted_at,
			"published_urls": [published_url],
			"post_id": POST_ID,
			"publisher": "xurl",
			"xurl_version": "1.3.1",
			"xurl_app": "default",
			"verified_account": "decodexspace",
			"verified_user_id": "42",
			"account_verified": true,
			"made_with_ai": true,
			"identity_response_sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
			"create_response_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
				"read_response_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
				"publication_lineage_sha256": PUBLICATION_LINEAGE,
				"recorded_cost_ceiling_microusd": 30000
		}
	})
}

fn outcome_payload(
	window: &str,
	run_id: &str,
	observed_at: &str,
	post_ref: &str,
	published_url: &str,
	digest: char,
) -> Value {
	json!({
		"schema": "social_outcome/v1",
		"slug": format!("gc-test-{window}"),
		"target_account": "decodexspace",
		"owner": {
			"automation_id": "decodex-xurl-publisher",
			"run_id": run_id
		},
		"social_post_ref": post_ref,
		"published_url": published_url,
		"observed_at": observed_at,
		"window": window,
		"metrics": {"views": 100, "likes": 4, "replies": 1, "reposts": 2},
		"observation": {
			"reader": "xurl",
			"xurl_version": "1.3.1",
			"xurl_app": "default",
				"verified_account": "decodexspace",
				"publication_lineage_sha256": PUBLICATION_LINEAGE,
				"response_sha256": digest.to_string().repeat(64),
			"recorded_cost_ceiling_microusd": 5000
		},
		"notes": ["Metrics were read by post ID."]
	})
}

fn publish_attempt_payload(
	timeline: Timeline,
	month: &str,
	candidate_ref: &str,
	reservation_ref: &str,
	published_url: &str,
) -> Value {
	json!({
		"schema": "decodex/xurl-publish-attempt/4",
		"run_id": PUBLISH_RUN_ID,
		"reservation_ref": reservation_ref,
			"candidate_ref": candidate_ref,
			"idempotency_key": IDEMPOTENCY_KEY,
			"publication_lineage_sha256": PUBLICATION_LINEAGE,
		"billing_month": month,
		"target_account": "decodexspace",
		"status": "published",
		"created_at": timeline.posted_at,
		"updated_at": timeline.posted_at,
		"reserved_cost_ceiling_microusd": 30000,
			"xurl_version": "1.3.1",
			"pricing_policy_id": "x-api-pay-per-usage/2026-07-27",
			"authorization_contract_sha256":
				"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		"calls": [
			call("identity_read", "succeeded", 10000, Some('d')),
			call("content_create", "succeeded", 15000, Some('a')),
			call("post_read_initial", "succeeded", 5000, Some('b'))
		],
		"verified_user_id": "42",
		"post_id": POST_ID,
		"published_url": published_url
	})
}

fn write_observation_attempt(
	root: &Path,
	month: &str,
	window: &str,
	run_id: &str,
	observed_at: &str,
	post_ref: &str,
	digest: char,
) -> PathBuf {
	let attempt_key = digest_hex(format!("{post_ref}\0{window}").as_bytes());
	let path = root.join("attempts").join(month).join(format!("observe-{attempt_key}.json"));
	let call = call("outcome_read", "succeeded", 5000, Some(digest));
	write(
		&path,
		&json!({
			"schema": "decodex/xurl-observation-attempt/4",
			"run_id": run_id,
			"billing_month": month,
			"reserved_cost_ceiling_microusd": 5000,
			"status": "observed",
				"post_ref": post_ref,
				"post_id": POST_ID,
				"publication_lineage_sha256": PUBLICATION_LINEAGE,
			"window": window,
			"created_at": observed_at,
				"updated_at": observed_at,
				"pricing_policy_id": "x-api-pay-per-usage/2026-07-27",
				"authorization_contract_sha256":
					"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
			"call": call.clone(),
			"calls": [call]
		}),
	);
	path
}

fn call(operation: &str, status: &str, cost: u64, digest: Option<char>) -> Value {
	json!({
		"operation": operation,
		"status": status,
		"recorded_cost_ceiling_microusd": cost,
		"response_sha256": digest.map(|value| value.to_string().repeat(64))
	})
}

fn recovery_call(
	operation: &str,
	status: &str,
	cost: u64,
	operation_id: &str,
	billing_month: &str,
	digest: char,
) -> Value {
	json!({
		"operation": operation,
		"operation_id": operation_id,
		"billing_month": billing_month,
		"status": status,
		"recorded_cost_ceiling_microusd": cost,
		"response_sha256": digest.to_string().repeat(64)
	})
}

fn key(path: &Path) -> String {
	let root = crate::repo_root().expect("repo root");
	crate::path_arg(&root, path)
}

fn pretty_json_bytes(value: &Value) -> Vec<u8> {
	let mut bytes = serde_json::to_vec_pretty(value).expect("fixture JSON");
	bytes.push(b'\n');
	bytes
}

fn write(path: &Path, value: &Value) {
	crate::write_new_json(path, value).expect("write private JSON");
}

fn load(path: &Path) -> Value {
	crate::load_json(path).expect("load private JSON")
}

fn rewrite(path: &Path, value: &Value) {
	fs::remove_file(path).expect("remove prior JSON");
	write(path, value);
}
