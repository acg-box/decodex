use std::{fs, io::Write as _};

use sha2::{Digest as _, Sha256};

use crate::{RadarContentEligibilityRequest, tests::fixtures};

#[test]
fn proves_one_fresh_reviewed_subject_is_content_eligible() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let (request, _, _, _) = write_fresh_artifacts(temp_dir.path());
	let report = crate::content_eligibility(&request)
		.expect("matching review and impact should be eligible");

	assert_eq!(report.repo, "openai/codex");
	assert_eq!(report.subject_kind, "pr");
	assert_eq!(report.subject_id, "22414");
	assert_eq!(report.slug, "openai-codex-pr-22414");
	assert_eq!(report.schema, "radar_content_eligibility/v1");
	assert_eq!(report.commit_shas, vec!["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned()]);
	assert_eq!(report.queue_sha256.len(), 64);
	assert_eq!(report.review_sha256.len(), 64);
	assert_eq!(report.impact_sha256.len(), 64);
	assert_eq!(report.lineage_sha256.len(), 64);
	let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
	let contract_path = manifest
		.parent()
		.and_then(std::path::Path::parent)
		.expect("Radar manifest should be inside the workspace")
		.join("automations/radar/scripts/github/content_eligibility_report.schema.json");
	let contract: serde_json::Value = serde_json::from_slice(
		&fs::read(contract_path).expect("eligibility report contract should be readable"),
	)
	.expect("eligibility report contract should parse");
	let required = contract["required"]
		.as_array()
		.expect("contract required fields should be an array")
		.iter()
		.map(|field| field.as_str().expect("required field should be a string"))
		.collect::<std::collections::BTreeSet<_>>();
	let serialized = serde_json::to_value(&report).expect("eligibility report should serialize");
	let actual = serialized
		.as_object()
		.expect("eligibility report should be an object")
		.keys()
		.map(String::as_str)
		.collect::<std::collections::BTreeSet<_>>();

	assert_eq!(actual, required);
}

#[test]
fn proves_production_cache_inputs_share_one_locked_snapshot() {
	let temp_dir = crate::test_support::private_tempdir();
	let (request, _, _, _) = write_fresh_private_artifacts(temp_dir.path());
	let report = crate::content_eligibility(&request)
		.expect("canonical private inputs should produce an eligibility receipt");

	assert_eq!(report.repo, "openai/codex");
	assert_eq!(report.lineage_sha256.len(), 64);
}

#[test]
fn rejects_mixed_private_and_external_inputs_instead_of_downgrading_private_reads() {
	let temp_dir = crate::test_support::private_tempdir();
	let (mut request, _, _, _) = write_fresh_private_artifacts(temp_dir.path());
	let external_review = temp_dir.path().join("external-review.json");

	fs::write(&external_review, fixtures::valid_upstream_review().to_string())
		.expect("external fixture should be written");
	request.review = external_review;
	let error = crate::content_eligibility(&request)
		.expect_err("mixed private and external inputs must fail before reading");

	assert!(error.to_string().contains("all share one Radar cache root or all be external"));
}

#[test]
fn rejects_queue_subject_without_required_review_to_impact_handoff() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let (request, _, review_path, impact_path) = write_fresh_artifacts(temp_dir.path());
	let mut review = fixtures::valid_upstream_review();
	let mut impact = fixtures::valid_upstream_impact();

	review["reviewed_at"] =
		serde_json::json!(crate::utc_now_iso().expect("current timestamp should format"));
	review["next_actions"][0]["type"] = serde_json::json!("none");
	fs::write(&review_path, review.to_string()).expect("review should be rewritten");
	impact["reviewed_at"] =
		serde_json::json!(crate::utc_now_iso().expect("current timestamp should format"));
	set_review_digest(&mut impact, &review_path);
	fs::write(impact_path, impact.to_string()).expect("impact lineage should be updated");

	let error = crate::content_eligibility(&request)
		.expect_err("review without upstream impact handoff must fail");

	assert!(error.to_string().contains("must request an upstream_impact next action"));
}

#[test]
fn rejects_missing_impact_artifact() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let (request, _, _, impact_path) = write_fresh_artifacts(temp_dir.path());

	fs::remove_file(impact_path).expect("impact fixture should be removed");

	let _error = crate::content_eligibility(&request)
		.expect_err("missing upstream impact must block content eligibility");
}

#[test]
fn rejects_structurally_invalid_impact_artifact() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let (request, _, _, impact_path) = write_fresh_artifacts(temp_dir.path());
	let mut impact = fixtures::valid_upstream_impact();

	impact["reviewed_at"] =
		serde_json::json!(crate::utc_now_iso().expect("current timestamp should format"));
	impact["evidence"] = serde_json::json!([]);
	fs::write(&impact_path, impact.to_string()).expect("invalid impact should be written");

	let error = crate::content_eligibility(&request)
		.expect_err("impact without evidence must block content eligibility");

	assert!(error.to_string().contains("Upstream impact validation failed"));
	assert!(error.to_string().contains("evidence must be a non-empty list"));
}

#[test]
fn rejects_stale_or_source_mismatched_impact() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let (request, _, _, impact_path) = write_fresh_artifacts(temp_dir.path());
	let mut impact = fixtures::valid_upstream_impact();

	impact["reviewed_at"] = serde_json::json!("2026-01-01T00:00:00Z");
	fs::write(&impact_path, impact.to_string()).expect("stale impact should be written");

	let stale_error =
		crate::content_eligibility(&request).expect_err("stale impact must fail freshness");

	assert!(stale_error.to_string().contains("source freshness limit"));

	impact["reviewed_at"] =
		serde_json::json!(crate::utc_now_iso().expect("current timestamp should format"));
	set_review_digest(&mut impact, &request.review);
	impact["source_refs"]["items"][0]["url"] =
		serde_json::json!("https://github.com/openai/codex/pull/99999");
	fs::write(&impact_path, impact.to_string()).expect("mismatched impact should be written");

	let mismatch_error = crate::content_eligibility(&request)
		.expect_err("impact for another source must not qualify");

	assert!(mismatch_error.to_string().contains("must cite the selected queue subject URL"));
}

#[test]
fn rejects_same_subject_url_at_a_different_upstream_head() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let (request, queue_path, _, _) = write_fresh_artifacts(temp_dir.path());
	let mut queue = fixtures::valid_review_queue();

	queue["generated_at"] =
		serde_json::json!(crate::utc_now_iso().expect("current timestamp should format"));
	queue["source"]["upstream_head"] =
		serde_json::json!("cccccccccccccccccccccccccccccccccccccccc");
	fs::write(queue_path, queue.to_string()).expect("different-head queue should be written");

	let error = crate::content_eligibility(&request)
		.expect_err("same URL at a different upstream head must fail");

	assert!(error.to_string().contains("upstream review head must match"));
}

#[test]
fn rejects_same_slug_when_the_review_artifact_digest_changes() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let (request, _, review_path, _) = write_fresh_artifacts(temp_dir.path());
	let mut review = fixtures::valid_upstream_review();

	review["reviewed_at"] =
		serde_json::json!(crate::utc_now_iso().expect("current timestamp should format"));
	review["evidence"][0] = serde_json::json!("Different source-backed evidence.");
	fs::write(review_path, review.to_string()).expect("changed review should be written");

	let error = crate::content_eligibility(&request)
		.expect_err("same slug with a different review digest must fail");

	assert!(error.to_string().contains("review_lineage.artifact_sha256"));
}

#[test]
fn rejects_review_with_a_different_normalized_commit_set() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let (request, _, review_path, impact_path) = write_fresh_artifacts(temp_dir.path());
	let mut review = fixtures::valid_upstream_review();
	let mut impact = fixtures::valid_upstream_impact();

	review["reviewed_at"] =
		serde_json::json!(crate::utc_now_iso().expect("current timestamp should format"));
	review["subject"]["commit_shas"] =
		serde_json::json!(["dddddddddddddddddddddddddddddddddddddddd"]);
	let review_raw = review.to_string();
	let review_digest = digest_hex(review_raw.as_bytes());

	impact["reviewed_at"] =
		serde_json::json!(crate::utc_now_iso().expect("current timestamp should format"));
	impact["review_lineage"]["artifact_sha256"] = serde_json::json!(review_digest);
	impact["review_lineage"]["commit_shas"] =
		serde_json::json!(["dddddddddddddddddddddddddddddddddddddddd"]);
	fs::write(review_path, review_raw).expect("different-commit review should be written");
	fs::write(impact_path, impact.to_string()).expect("matching impact should be written");

	let error = crate::content_eligibility(&request)
		.expect_err("review commit set must match the queue subject");

	assert!(error.to_string().contains("commit_shas must exactly match"));
}

#[test]
fn rejects_impact_with_a_different_commit_set_from_the_validated_review() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let (request, _, _, impact_path) = write_fresh_artifacts(temp_dir.path());
	let mut impact = fixtures::valid_upstream_impact();

	impact["reviewed_at"] =
		serde_json::json!(crate::utc_now_iso().expect("current timestamp should format"));
	set_review_digest(&mut impact, &request.review);
	impact["review_lineage"]["commit_shas"] =
		serde_json::json!(["dddddddddddddddddddddddddddddddddddddddd"]);
	fs::write(impact_path, impact.to_string()).expect("mismatched impact should be written");
	let error = crate::content_eligibility(&request)
		.expect_err("impact commit set must exactly match the validated review");

	assert!(
		error
			.to_string()
			.contains("review_lineage.commit_shas must exactly match the upstream review")
	);
}

#[test]
fn eligibility_lineage_digest_changes_when_a_valid_impact_is_tampered() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let (request, _, _, impact_path) = write_fresh_artifacts(temp_dir.path());
	let first =
		crate::content_eligibility(&request).expect("initial eligibility receipt should succeed");
	let mut impact: serde_json::Value =
		serde_json::from_slice(&fs::read(&impact_path).expect("impact fixture should be readable"))
			.expect("impact fixture should parse");

	impact["caveats"] = serde_json::json!(["A newly recorded caveat."]);
	fs::write(&impact_path, impact.to_string()).expect("tampered impact should be written");
	let second = crate::content_eligibility(&request)
		.expect("valid impact changes should produce a different receipt");

	assert_eq!(first.review_sha256, second.review_sha256);
	assert_ne!(first.impact_sha256, second.impact_sha256);
	assert_ne!(first.lineage_sha256, second.lineage_sha256);
}

#[test]
fn regular_content_read_stops_at_the_bound_when_the_file_grows_after_metadata() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let path = temp_dir.path().join("growing.json");

	fs::write(&path, b"1234").expect("fixture should be written");
	let append_path = path.clone();
	let error =
		crate::content_eligibility::read_regular_file_bounded_after_metadata(&path, 4, move || {
			let mut file = fs::OpenOptions::new()
				.append(true)
				.open(append_path)
				.expect("fixture should reopen for append");

			file.write_all(b"5").expect("fixture should grow after metadata");
			file.sync_all().expect("fixture growth should be visible");
		})
		.expect_err("a growing regular file must stop at max plus one bytes");

	assert!(error.to_string().contains("bounded read limit"));
}

fn write_fresh_artifacts(
	root: &std::path::Path,
) -> (RadarContentEligibilityRequest, std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
	let timestamp = crate::utc_now_iso().expect("current timestamp should format");
	let mut queue = fixtures::valid_review_queue();
	let mut review = fixtures::valid_upstream_review();
	let mut impact = fixtures::valid_upstream_impact();
	let queue_path = root.join("queue.json");
	let review_path = root.join("review.json");
	let impact_path = root.join("impact.json");

	queue["generated_at"] = serde_json::json!(timestamp);
	review["reviewed_at"] = serde_json::json!(timestamp);
	impact["reviewed_at"] = serde_json::json!(timestamp);
	fs::write(&queue_path, queue.to_string()).expect("queue should be written");
	let review_raw = review.to_string();
	let review_digest = digest_hex(review_raw.as_bytes());

	impact["review_lineage"]["artifact_sha256"] = serde_json::json!(review_digest);
	fs::write(&review_path, review_raw).expect("review should be written");
	fs::write(&impact_path, impact.to_string()).expect("impact should be written");

	(
		RadarContentEligibilityRequest {
			queue: queue_path.clone(),
			review: review_path.clone(),
			impact: impact_path.clone(),
			max_age_hours: 12,
		},
		queue_path,
		review_path,
		impact_path,
	)
}

fn write_fresh_private_artifacts(
	root: &std::path::Path,
) -> (RadarContentEligibilityRequest, std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
	let timestamp = crate::utc_now_iso().expect("current timestamp should format");
	let mut queue = fixtures::valid_review_queue();
	let mut review = fixtures::valid_upstream_review();
	let mut impact = fixtures::valid_upstream_impact();
	let cache = root.join(crate::DEFAULT_CACHE_ROOT);
	let queue_path = cache.join("github/review-queue/openai-codex-latest.json");
	let pair = cache.join(
		"github/content-review-pairs/fixture--\
		 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
	);
	let review_path = pair.join("review.json");
	let impact_path = pair.join("impact.json");

	queue["generated_at"] = serde_json::json!(timestamp);
	review["reviewed_at"] = serde_json::json!(timestamp);
	impact["reviewed_at"] = serde_json::json!(timestamp);
	crate::write_json(&queue_path, &queue).expect("private queue should be written");
	crate::write_json(&review_path, &review).expect("private review should be written");
	set_review_digest(&mut impact, &review_path);
	crate::write_json(&impact_path, &impact).expect("private impact should be written");

	(
		RadarContentEligibilityRequest {
			queue: queue_path.clone(),
			review: review_path.clone(),
			impact: impact_path.clone(),
			max_age_hours: 12,
		},
		queue_path,
		review_path,
		impact_path,
	)
}

fn digest_hex(payload: &[u8]) -> String {
	Sha256::digest(payload).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn set_review_digest(impact: &mut serde_json::Value, review_path: &std::path::Path) {
	let review = fs::read(review_path).expect("review bytes should be readable");

	impact["review_lineage"]["artifact_sha256"] = serde_json::json!(digest_hex(&review));
}
