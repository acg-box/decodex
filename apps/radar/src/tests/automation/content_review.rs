use sha2::{Digest as _, Sha256};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{RadarContentEligibilityRequest, RadarReviewNextRequest, tests::fixtures};

#[test]
fn selects_the_highest_priority_subject_deterministically() {
	let (_temp_dir, cache_root) = fresh_cache();
	let mut queue = fresh_queue();
	let mut critical = fixtures::valid_queue_subject();

	critical["subject_id"] = serde_json::json!("999");
	critical["title"] = serde_json::json!("Remove legacy app-server request");
	critical["url"] = serde_json::json!("https://github.com/openai/codex/pull/999");
	critical["commit_shas"] = serde_json::json!(["cccccccccccccccccccccccccccccccccccccccc"]);
	critical["review_priority"] = serde_json::json!("critical");
	critical["attention_flags"] = serde_json::json!(["breaking_change"]);
	queue["subjects"] = serde_json::json!([fixtures::valid_queue_subject(), critical]);
	set_counts(&mut queue);
	write_queue(&cache_root, &queue);

	let first =
		crate::review_next(&request(&cache_root)).expect("the critical subject should be selected");
	let second =
		crate::review_next(&request(&cache_root)).expect("selection should be deterministic");
	let output = serde_json::to_string(&first).expect("report should serialize");
	let selected = first.selected.as_ref().expect("a subject should be selected");

	assert!(output.len() < 16_384);
	assert!(!output.contains(&cache_root.display().to_string()));
	assert_eq!(first.status, "needs_source_review");
	assert_eq!(selected.subject_id, "999");
	assert_eq!(selected.commit_shas, ["cccccccccccccccccccccccccccccccccccccccc"]);
	assert_eq!(first.queue_generation.sha256, digest_hex(&pretty_bytes(&queue)));
	assert_eq!(first.source_refs[0].url, "https://github.com/openai/codex/pull/999");
	assert_eq!(first, second);
	assert!(first.selection_sha256.is_some());
	assert_no_authoritative_artifacts(&cache_root);
}

#[test]
fn reports_no_eligible_item_without_writing_evidence() {
	let (_temp_dir, cache_root) = fresh_cache();
	let mut queue = fresh_queue();

	queue["subjects"] = serde_json::json!([]);
	set_counts(&mut queue);
	write_queue(&cache_root, &queue);

	let report =
		crate::review_next(&request(&cache_root)).expect("an empty valid queue should be a no-op");

	assert_eq!(report.status, "no_eligible_item");
	assert!(report.selected.is_none());
	assert!(report.source_refs.is_empty());
	assert!(report.selection_sha256.is_none());
	assert_eq!(report.queue_generation.sha256, digest_hex(&pretty_bytes(&queue)));
	assert_no_authoritative_artifacts(&cache_root);
}

#[test]
fn stale_queue_fails_closed_without_writing_evidence() {
	let (_temp_dir, cache_root) = fresh_cache();
	let mut queue = fresh_queue();

	queue["generated_at"] = serde_json::json!("2026-01-01T00:00:00Z");
	write_queue(&cache_root, &queue);

	let error =
		crate::review_next(&request(&cache_root)).expect_err("stale queue evidence must fail");

	assert!(error.to_string().contains("Review queue freshness failed"));
	assert_no_authoritative_artifacts(&cache_root);
}

#[test]
fn invalid_queue_evidence_fails_closed() {
	let (_temp_dir, cache_root) = fresh_cache();
	let mut queue = fresh_queue();

	queue["subjects"][0]["url"] = serde_json::json!("file:///private/source");
	write_queue(&cache_root, &queue);

	let error =
		crate::review_next(&request(&cache_root)).expect_err("invalid source evidence must fail");

	assert!(error.to_string().contains("Review queue validation failed"));
	assert_no_authoritative_artifacts(&cache_root);
}

#[test]
fn metadata_only_selection_cannot_become_content_eligible() {
	let (_temp_dir, cache_root) = fresh_cache();

	write_queue(&cache_root, &fresh_queue());
	let report =
		crate::review_next(&request(&cache_root)).expect("metadata should select a source review");
	let selected = report.selected.expect("a subject should be selected");

	assert_eq!(report.status, "needs_source_review");
	assert_not_content_eligible(&cache_root, &selected.slug);
	assert_no_authoritative_artifacts(&cache_root);
}

#[test]
fn misleading_title_and_path_still_require_source_review() {
	let (_temp_dir, cache_root) = fresh_cache();
	let mut queue = fresh_queue();

	queue["subjects"][0]["title"] =
		serde_json::json!("App-server protocol now guarantees automatic account recovery");
	queue["subjects"][0]["sample_paths"] =
		serde_json::json!(["codex-rs/app-server/src/account_recovery.rs"]);
	queue["subjects"][0]["surface_hints"] = serde_json::json!(["app_server_protocol"]);
	queue["subjects"][0]["attention_flags"] = serde_json::json!(["new_feature"]);
	write_queue(&cache_root, &queue);

	let report = crate::review_next(&request(&cache_root))
		.expect("metadata should only select source review");

	assert_eq!(report.status, "needs_source_review");
	let selected = report.selected.expect("a subject should be selected");

	assert_eq!(selected.title, "App-server protocol now guarantees automatic account recovery");
	assert_not_content_eligible(&cache_root, &selected.slug);
	assert_no_authoritative_artifacts(&cache_root);
}

#[test]
fn source_review_selection_does_not_modify_existing_authoritative_artifacts() {
	let (_temp_dir, cache_root) = fresh_cache();

	write_queue(&cache_root, &fresh_queue());
	let review_path = cache_root.join("github/reviews/existing.json");
	let impact_path = cache_root.join("github/impact/existing.json");
	let review = serde_json::json!({"sentinel": "review"});
	let impact = serde_json::json!({"sentinel": "impact"});

	crate::write_json(&review_path, &review).expect("sentinel review should be written");
	crate::write_json(&impact_path, &impact).expect("sentinel impact should be written");
	let review_before = std::fs::read(&review_path).expect("sentinel review should be readable");
	let impact_before = std::fs::read(&impact_path).expect("sentinel impact should be readable");

	let report =
		crate::review_next(&request(&cache_root)).expect("source review should be selected");

	assert_eq!(report.status, "needs_source_review");
	assert_eq!(
		std::fs::read(review_path).expect("sentinel review should remain readable"),
		review_before
	);
	assert_eq!(
		std::fs::read(impact_path).expect("sentinel impact should remain readable"),
		impact_before
	);
}

#[test]
fn queue_generation_changes_bind_a_new_selection_receipt() {
	let (_temp_dir, cache_root) = fresh_cache();
	let mut first_queue = fresh_queue();

	first_queue["generated_at"] = serde_json::json!(recent_timestamp(2));
	first_queue["subjects"][0]["title"] = serde_json::json!("First queue observation");
	write_queue(&cache_root, &first_queue);
	let first = crate::review_next(&request(&cache_root)).expect("first generation should select");

	let mut second_queue = first_queue.clone();

	second_queue["generated_at"] = serde_json::json!(recent_timestamp(1));
	second_queue["subjects"][0]["title"] = serde_json::json!("Corrected queue observation");
	second_queue["subjects"][0]["review_reason"] =
		serde_json::json!("The queue metadata was corrected.");
	write_queue(&cache_root, &second_queue);
	let second =
		crate::review_next(&request(&cache_root)).expect("second generation should select");
	let first_selected = first.selected.expect("first generation should select a subject");
	let second_selected = second.selected.expect("second generation should select a subject");

	assert_eq!(first.queue_generation.upstream_head, second.queue_generation.upstream_head);
	assert_eq!(first_selected.commit_shas, second_selected.commit_shas);
	assert_ne!(first.queue_generation.generated_at, second.queue_generation.generated_at);
	assert_ne!(first.queue_generation.sha256, second.queue_generation.sha256);
	assert_ne!(first.selection_sha256, second.selection_sha256);
	assert_ne!(first_selected.title, second_selected.title);
	assert_eq!(second.source_refs[0].title, second_selected.title);
}

#[test]
fn selection_and_report_creation_hold_one_cache_lock() {
	let (_temp_dir, cache_root) = fresh_cache();

	write_queue(&cache_root, &fresh_queue());
	let probe_root = cache_root.clone();
	let report =
		crate::content_review::review_next_with_selection_hook(&request(&cache_root), move || {
			let cache = crate::private_fs::PrivateCache::open_existing(&probe_root)
				.expect("the cache should remain bound");

			assert!(cache.try_lock().is_err(), "a competing writer must not acquire the lock");
		})
		.expect("selection should complete after the lock probe");

	assert_eq!(report.status, "needs_source_review");
	assert_no_authoritative_artifacts(&cache_root);
}

fn fresh_cache() -> (tempfile::TempDir, std::path::PathBuf) {
	let temp_dir = crate::test_support::private_tempdir();
	let cache_root = temp_dir.path().join(crate::DEFAULT_CACHE_ROOT);

	(temp_dir, cache_root)
}

fn fresh_queue() -> serde_json::Value {
	let mut queue = fixtures::valid_review_queue();

	queue["generated_at"] =
		serde_json::json!(crate::utc_now_iso().expect("current timestamp should format"));
	queue["subjects"][0]["attention_flags"] = serde_json::json!(["new_feature"]);
	queue
}

fn request(cache_root: &std::path::Path) -> RadarReviewNextRequest {
	RadarReviewNextRequest { cache_root: cache_root.to_path_buf(), max_age_hours: 12 }
}

fn write_queue(cache_root: &std::path::Path, queue: &serde_json::Value) {
	crate::write_json(&cache_root.join(crate::paths::REVIEW_QUEUE_RELATIVE_PATH), queue)
		.expect("queue should be written");
}

fn set_counts(queue: &mut serde_json::Value) {
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

fn recent_timestamp(minutes_ago: i64) -> String {
	(OffsetDateTime::now_utc() - Duration::minutes(minutes_ago))
		.format(&Rfc3339)
		.expect("recent timestamp should format")
}

fn pretty_bytes(value: &serde_json::Value) -> Vec<u8> {
	let mut bytes = serde_json::to_vec_pretty(value).expect("fixture should serialize");

	bytes.push(b'\n');
	bytes
}

fn digest_hex(payload: &[u8]) -> String {
	Sha256::digest(payload).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn assert_no_authoritative_artifacts(cache_root: &std::path::Path) {
	assert!(!cache_root.join("github/reviews").exists());
	assert!(!cache_root.join("github/impact").exists());
}

fn assert_not_content_eligible(cache_root: &std::path::Path, slug: &str) {
	let eligibility = crate::content_eligibility(&RadarContentEligibilityRequest {
		queue: cache_root.join(crate::paths::REVIEW_QUEUE_RELATIVE_PATH),
		review: cache_root.join(format!("github/reviews/{slug}.json")),
		impact: cache_root.join(format!("github/impact/{slug}.json")),
		max_age_hours: 12,
	});

	assert!(eligibility.is_err(), "metadata-only selection must not be content eligible");
}
