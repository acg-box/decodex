use std::{fs, os::unix::fs::PermissionsExt as _, path::Path};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
	RadarCacheGcRequest, RadarContentEligibilityRequest, RadarContentPairCommitRequest,
	RadarReviewNextRequest, requests::CacheRetentionPolicy, tests::fixtures,
};

#[test]
fn commits_exactly_two_owner_only_artifacts_and_removes_confirmed_staging() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "run-1", None);
	let report = crate::commit_content_pair(&request(&cache_root, &staging))
		.expect("a valid pair should commit atomically");
	let pair_dir = cache_root.join(&report.pair_dir);

	assert_eq!(report.schema, "radar_content_review_pair_commit/v1");
	assert_eq!(report.status, "committed");
	assert!(!staging.exists());
	assert_eq!(fs::read_dir(&pair_dir).expect("pair directory should exist").count(), 2);
	for path in [cache_root.join(&report.review_path), cache_root.join(&report.impact_path)] {
		assert_eq!(fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
	}
	assert_eq!(fs::metadata(pair_dir).unwrap().permissions().mode() & 0o777, 0o700);
	let review_raw = fs::read(cache_root.join(&report.review_path)).unwrap();
	let impact = crate::load_json(&cache_root.join(&report.impact_path)).unwrap();
	let committed_digest = impact["review_lineage"]["artifact_sha256"]
		.as_str()
		.expect("committed impact digest should be text");

	assert_eq!(committed_digest, digest_hex(&review_raw));
	assert_eq!(committed_digest, report.review_sha256);
	assert_ne!(committed_digest, review_digest_sentinel());
	assert_report_matches_schema(&report);
}

#[test]
fn exact_retry_recovers_idempotently_and_conflicting_retry_fails_closed() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "retry-1", None);
	let exact_payload = fs::read(&staging).expect("staging should be readable");
	let first = crate::commit_content_pair(&request(&cache_root, &staging))
		.expect("the first commit should succeed");

	let exact_staging = cache_root.join("github/content-review-staging/retry-1.json");
	let exact_value: Value =
		serde_json::from_slice(&exact_payload).expect("saved staging should parse");

	crate::write_json(&exact_staging, &exact_value).expect("exact staging should be restored");
	let recovered = crate::commit_content_pair(&request(&cache_root, &exact_staging))
		.expect("the exact retry should recover");

	assert_eq!(recovered.status, "recovered");
	assert_eq!(recovered.pair_dir, first.pair_dir);
	assert!(!exact_staging.exists());

	let conflicting = write_staging(
		&cache_root,
		"retry-1",
		Some("Different source-backed evidence for the same run."),
	);
	let error = crate::commit_content_pair(&request(&cache_root, &conflicting))
		.expect_err("a changed retry must fail closed");

	assert!(error.to_string().contains("run_id already has a conflicting"));
	assert!(conflicting.exists(), "unconfirmed staging must remain");
	assert_eq!(
		fs::read_dir(cache_root.join("github/content-review-pairs"))
			.expect("pair root should exist")
			.count(),
		1
	);
}

#[test]
fn handled_pair_advances_review_next_to_the_next_subject() {
	let (_temp, cache_root) = fresh_cache();
	let mut queue = current_queue();
	let mut second = fixtures::valid_queue_subject();

	second["subject_id"] = serde_json::json!("30000");
	second["title"] = serde_json::json!("Add a second operator-visible feature");
	second["url"] = serde_json::json!("https://github.com/openai/codex/pull/30000");
	second["commit_shas"] = serde_json::json!(["cccccccccccccccccccccccccccccccccccccccc"]);
	second["review_priority"] = serde_json::json!("normal");
	second["attention_flags"] = serde_json::json!(["new_feature"]);
	queue["subjects"] = serde_json::json!([fixtures::valid_queue_subject(), second]);
	set_counts(&mut queue);
	write_queue(&cache_root, &queue);
	let staging = write_staging_for_queue(&cache_root, "advance-1", &queue, None);

	crate::commit_content_pair(&request(&cache_root, &staging))
		.expect("the first subject pair should commit");
	let report =
		crate::review_next(&review_request(&cache_root)).expect("the selector should advance");

	assert_eq!(report.handled_count, 1);
	assert_eq!(report.handled_state_sha256.len(), 64);
	assert_eq!(report.selected.unwrap().subject_id, "30000");
	assert!(report.selection_sha256.is_some());
}

#[test]
fn valid_historical_pair_remains_handled_for_its_retention_lifetime() {
	let (_temp, cache_root) = fresh_cache();
	let mut review = fixtures::valid_upstream_review();
	let mut impact = fixtures::valid_upstream_impact();

	review["reviewed_at"] = serde_json::json!("2026-01-01T00:00:00Z");
	impact["reviewed_at"] = serde_json::json!("2026-01-01T00:00:00Z");
	let review_raw = pretty_bytes(&review);

	impact["review_lineage"]["artifact_sha256"] = serde_json::json!(digest_hex(&review_raw));
	let impact_raw = pretty_bytes(&impact);
	let pair_digest = pair_digest(&review_raw, &impact_raw);
	let pair = cache_root.join(format!("github/content-review-pairs/historical-1--{pair_digest}"));

	crate::write_json(&pair.join("review.json"), &review).expect("review should be written");
	crate::write_json(&pair.join("impact.json"), &impact).expect("impact should be written");
	let report = crate::review_next(&review_request(&cache_root))
		.expect("historical handled state should remain valid");

	assert_eq!(report.status, "no_eligible_item");
	assert_eq!(report.handled_count, 1);
	assert!(report.selected.is_none());
}

#[test]
fn handled_identity_survives_queue_head_changes_but_not_commit_changes() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "head-change-1", None);

	crate::commit_content_pair(&request(&cache_root, &staging))
		.expect("the initial pair should commit");
	let mut queue = current_queue();
	queue["source"]["upstream_head"] =
		serde_json::json!("dddddddddddddddddddddddddddddddddddddddd");
	write_queue(&cache_root, &queue);
	let handled = crate::review_next(&review_request(&cache_root))
		.expect("a queue-head-only change must keep the subject handled");

	assert_eq!(handled.status, "no_eligible_item");
	assert_eq!(handled.handled_count, 1);

	queue["subjects"][0]["commit_shas"] =
		serde_json::json!(["eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"]);
	set_counts(&mut queue);
	write_queue(&cache_root, &queue);
	let changed = crate::review_next(&review_request(&cache_root))
		.expect("a changed commit set must be eligible for a fresh source review");

	assert_eq!(changed.status, "needs_source_review");
	assert_eq!(changed.handled_count, 0);
	assert_eq!(
		changed.selected.unwrap().commit_shas,
		vec!["eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"]
	);
}

#[test]
fn malformed_or_ambiguous_handled_state_blocks_selection() {
	let (_temp, cache_root) = fresh_cache();
	let pair = cache_root.join(
		"github/content-review-pairs/run-1--\
		 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
	);

	write_queue(&cache_root, &current_queue());
	crate::write_json(&pair.join("review.json"), &fixtures::valid_upstream_review())
		.expect("malformed pair fixture should be written");
	let error = crate::review_next(&review_request(&cache_root))
		.expect_err("a partial committed pair must fail closed");

	assert!(error.to_string().contains("must contain exactly two artifacts"));
}

#[test]
fn duplicate_committed_subject_is_ambiguous_and_blocks_selection() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "first-run", None);
	let committed = crate::commit_content_pair(&request(&cache_root, &staging))
		.expect("the first pair should commit");
	let review = crate::load_json(&cache_root.join(&committed.review_path)).unwrap();
	let impact = crate::load_json(&cache_root.join(&committed.impact_path)).unwrap();
	let review_raw = pretty_bytes(&review);
	let impact_raw = pretty_bytes(&impact);
	let duplicate = cache_root.join(format!(
		"github/content-review-pairs/second-run--{}",
		pair_digest(&review_raw, &impact_raw)
	));

	crate::write_json(&duplicate.join("review.json"), &review).unwrap();
	crate::write_json(&duplicate.join("impact.json"), &impact).unwrap();
	let error = crate::review_next(&review_request(&cache_root))
		.expect_err("duplicate handled state must fail closed");

	assert!(error.to_string().contains("subject is duplicated"));
}

#[test]
fn committed_pair_is_the_content_eligibility_input() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "eligible-1", None);
	let committed = crate::commit_content_pair(&request(&cache_root, &staging))
		.expect("the pair should commit");
	let report = crate::content_eligibility(&RadarContentEligibilityRequest {
		queue: cache_root.join(crate::paths::REVIEW_QUEUE_RELATIVE_PATH),
		review: cache_root.join(committed.review_path),
		impact: cache_root.join(committed.impact_path),
		max_age_hours: 12,
	})
	.expect("the committed publish pair should be eligible");

	assert_eq!(report.subject_id, "22414");
	assert_eq!(report.review_sha256, committed.review_sha256);
	assert_eq!(report.impact_sha256, committed.impact_sha256);
}

#[test]
fn stale_queue_lineage_does_not_delete_staging_or_create_a_pair() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "wrong-queue", None);
	let mut payload: Value =
		serde_json::from_slice(&fs::read(&staging).unwrap()).expect("staging should parse");

	payload["queue_sha256"] = serde_json::json!("0".repeat(64));
	crate::write_json(&staging, &payload).expect("staging should be replaced");
	let error = crate::commit_content_pair(&request(&cache_root, &staging))
		.expect_err("stale queue lineage must fail");

	assert!(error.to_string().contains("queue_sha256 is not current"));
	assert!(staging.exists());
	assert!(!cache_root.join("github/content-review-pairs").exists());
}

#[test]
fn staging_requires_the_non_authoritative_review_digest_sentinel() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "precomputed-digest", None);
	let mut payload: Value = crate::load_json(&staging).expect("staging should load");
	let review_raw = pretty_bytes(&payload["review"]);

	payload["impact"]["review_lineage"]["artifact_sha256"] =
		serde_json::json!(digest_hex(&review_raw));
	crate::write_json(&staging, &payload).expect("precomputed staging should be written");
	let error = crate::commit_content_pair(&request(&cache_root, &staging))
		.expect_err("a precomputed authoritative digest must be rejected");

	assert!(error.to_string().contains("non-authoritative sentinel"));
	assert!(staging.exists());
	assert!(!cache_root.join("github/content-review-pairs").exists());
	assert_staging_schema_requires_sentinel();
}

#[test]
fn staging_rejects_a_missing_review_digest_sentinel() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "missing-digest", None);
	let mut payload: Value = crate::load_json(&staging).expect("staging should load");

	payload["impact"]["review_lineage"]
		.as_object_mut()
		.expect("review lineage should be an object")
		.remove("artifact_sha256");
	crate::write_json(&staging, &payload).expect("missing-sentinel staging should be written");
	let error = crate::commit_content_pair(&request(&cache_root, &staging))
		.expect_err("a missing sentinel must be rejected");

	assert!(
		error.to_string().contains("artifact_sha256")
			|| error.to_string().contains("non-authoritative sentinel")
	);
	assert!(staging.exists());
	assert!(!cache_root.join("github/content-review-pairs").exists());
}

#[test]
fn cache_gc_removes_a_committed_pair_as_one_unit() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "gc-1", None);
	let committed = crate::commit_content_pair(&request(&cache_root, &staging))
		.expect("the pair should commit");
	let policy =
		CacheRetentionPolicy { max_files_per_collection: 1, ..CacheRetentionPolicy::default() };
	let report = crate::cache_gc(&RadarCacheGcRequest {
		cache_root: cache_root.clone(),
		policy,
		now: std::time::SystemTime::now(),
	})
	.expect("pair retention should complete");

	assert_eq!(report.files_removed, 2);
	assert!(!cache_root.join(committed.pair_dir).exists());
	assert!(!cache_root.join(committed.review_path).exists());
	assert!(!cache_root.join(committed.impact_path).exists());
}

#[test]
fn cache_gc_recovers_an_interrupted_temporary_pair_directory() {
	let (_temp, cache_root) = fresh_cache();
	let temporary = cache_root.join("github/content-review-pairs/.radar-tmp-crashed-pair");

	crate::write_json(&temporary.join("review.json"), &fixtures::valid_upstream_review())
		.expect("temporary partial pair should be written");
	let report = crate::cache_gc(&RadarCacheGcRequest {
		cache_root,
		policy: CacheRetentionPolicy::default(),
		now: std::time::SystemTime::now(),
	})
	.expect("cache GC should recover the interrupted pair");

	assert_eq!(report.files_removed, 1);
	assert!(!temporary.exists());
}

fn fresh_cache() -> (crate::private_fs::PrivateTestDirectory, std::path::PathBuf) {
	let temp = crate::test_support::private_tempdir();
	let cache_root = temp.path().join(crate::DEFAULT_CACHE_ROOT);

	write_queue(&cache_root, &current_queue());
	(temp, cache_root)
}

fn current_queue() -> Value {
	let mut queue = fixtures::valid_review_queue();

	queue["generated_at"] =
		serde_json::json!(crate::utc_now_iso().expect("timestamp should format"));
	queue["subjects"][0]["attention_flags"] = serde_json::json!(["new_feature"]);
	queue
}

fn write_staging(
	cache_root: &Path,
	run_id: &str,
	changed_evidence: Option<&str>,
) -> std::path::PathBuf {
	let queue: Value = crate::load_json(&cache_root.join(crate::paths::REVIEW_QUEUE_RELATIVE_PATH))
		.expect("queue should load");

	write_staging_for_queue(cache_root, run_id, &queue, changed_evidence)
}

fn write_staging_for_queue(
	cache_root: &Path,
	run_id: &str,
	queue: &Value,
	changed_evidence: Option<&str>,
) -> std::path::PathBuf {
	let now = crate::utc_now_iso().expect("timestamp should format");
	let mut review = fixtures::valid_upstream_review();
	let mut impact = fixtures::valid_upstream_impact();

	review["reviewed_at"] = serde_json::json!(now);
	impact["reviewed_at"] = serde_json::json!(now);
	if let Some(evidence) = changed_evidence {
		review["evidence"][0] = serde_json::json!(evidence);
	}
	impact["review_lineage"]["artifact_sha256"] = serde_json::json!(review_digest_sentinel());
	let queue_raw = pretty_bytes(queue);
	let staging = serde_json::json!({
		"schema": "radar_content_review_pair_staging/v1",
		"run_id": run_id,
		"queue_sha256": digest_hex(&queue_raw),
		"review": review,
		"impact": impact,
	});
	let path = cache_root.join(format!("github/content-review-staging/{run_id}.json"));

	crate::write_json(&path, &staging).expect("staging should be written");
	path
}

fn request(cache_root: &Path, staging: &Path) -> RadarContentPairCommitRequest {
	RadarContentPairCommitRequest {
		cache_root: cache_root.to_path_buf(),
		staging: staging.to_path_buf(),
		max_age_hours: 12,
	}
}

fn review_request(cache_root: &Path) -> RadarReviewNextRequest {
	let queue_raw = fs::read(cache_root.join(crate::paths::REVIEW_QUEUE_RELATIVE_PATH))
		.expect("queue should be readable");

	RadarReviewNextRequest {
		cache_root: cache_root.to_path_buf(),
		expected_queue_sha256: digest_hex(&queue_raw),
		max_age_hours: 12,
	}
}

fn write_queue(cache_root: &Path, queue: &Value) {
	crate::write_json(&cache_root.join(crate::paths::REVIEW_QUEUE_RELATIVE_PATH), queue)
		.expect("queue should be written");
}

fn set_counts(queue: &mut Value) {
	let subjects = queue["subjects"].as_array().expect("subjects should be a list");
	let count = |priority: &str| {
		subjects
			.iter()
			.filter(|subject| subject["review_priority"].as_str() == Some(priority))
			.count()
	};
	let counts = (subjects.len(), count("critical"), count("high"), count("normal"), count("low"));

	queue["counts"]["subjects_queued"] = serde_json::json!(counts.0);
	queue["counts"]["critical"] = serde_json::json!(counts.1);
	queue["counts"]["high"] = serde_json::json!(counts.2);
	queue["counts"]["normal"] = serde_json::json!(counts.3);
	queue["counts"]["low"] = serde_json::json!(counts.4);
}

fn pretty_bytes(value: &Value) -> Vec<u8> {
	let mut bytes = serde_json::to_vec_pretty(value).expect("fixture should serialize");

	bytes.push(b'\n');
	bytes
}

fn digest_hex(payload: &[u8]) -> String {
	Sha256::digest(payload).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn review_digest_sentinel() -> &'static str {
	"0000000000000000000000000000000000000000000000000000000000000000"
}

fn pair_digest(review_raw: &[u8], impact_raw: &[u8]) -> String {
	let mut digest = Sha256::new();

	digest.update(b"radar-content-review-pair-v1");
	for payload in [review_raw, impact_raw] {
		digest.update(u64::try_from(payload.len()).unwrap().to_be_bytes());
		digest.update(payload);
	}

	digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn assert_report_matches_schema(report: &crate::RadarContentPairCommitReport) {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.and_then(Path::parent)
		.expect("Radar should be inside the workspace");
	let schema: Value = serde_json::from_slice(
		&fs::read(root.join(
			"automations/radar/scripts/github/content_review_pair_commit_report.schema.json",
		))
		.expect("commit report schema should be readable"),
	)
	.expect("commit report schema should parse");
	let required = schema["required"]
		.as_array()
		.expect("required should be a list")
		.iter()
		.map(|field| field.as_str().expect("required field should be text"))
		.collect::<std::collections::BTreeSet<_>>();
	let serialized = serde_json::to_value(report).expect("report should serialize");
	let actual = serialized
		.as_object()
		.expect("report should be an object")
		.keys()
		.map(String::as_str)
		.collect::<std::collections::BTreeSet<_>>();

	assert_eq!(actual, required);
}

fn assert_staging_schema_requires_sentinel() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.and_then(Path::parent)
		.expect("Radar should be inside the workspace");
	let schema: Value = serde_json::from_slice(
		&fs::read(
			root.join("automations/radar/scripts/github/content_review_pair_staging.schema.json"),
		)
		.expect("staging schema should be readable"),
	)
	.expect("staging schema should parse");

	assert_eq!(
		schema["properties"]["impact"]["allOf"][1]["properties"]["review_lineage"]["properties"]["artifact_sha256"]
			["const"],
		review_digest_sentinel()
	);
}
