use std::{fs, os::unix::fs::PermissionsExt as _, path::Path};

use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
	RadarCacheGcRequest, RadarContentEligibilityRequest, RadarContentPairCommitRequest,
	RadarReviewNextRequest,
	requests::CacheRetentionPolicy,
	tests::{env::TestEnvVars, fixtures},
};

const PATCH_ANCHOR_PATH: &str = "codex-rs/app-server/src/lib.rs";
const SECOND_IMPLEMENTATION_PATH: &str = "codex-rs/app-server/src/config.rs";

#[test]
fn commits_a_four_excerpt_bound_pair_as_exactly_two_owner_only_artifacts() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "run-1", None);
	let staging_raw = fs::read(&staging).expect("staging bytes should be readable");
	let staged = crate::load_json(&staging).expect("staging should be readable");

	assert_eq!(staged["bundle_evidence_receipt"]["file_count"], 4);
	assert_eq!(staged["bundle_evidence_receipt"]["patch_excerpt_count"], 4);
	assert_eq!(staged["patch_anchor"]["path"], PATCH_ANCHOR_PATH);
	let report =
		commit_staging(&cache_root, &staging).expect("a valid pair should commit atomically");
	let pair_dir = cache_root.join(&report.pair_dir);

	assert_eq!(report.schema, "radar_content_review_pair_commit/v1");
	assert_eq!(report.status, "committed");
	let pair_name = Path::new(&report.pair_dir)
		.file_name()
		.and_then(|value| value.to_str())
		.expect("pair directory should have a UTF-8 name");
	let pair_parts = pair_name.split("--").collect::<Vec<_>>();

	assert_eq!(pair_parts.len(), 3);
	assert!(!staging.exists());
	assert_eq!(fs::read_dir(&pair_dir).expect("pair directory should exist").count(), 2);
	for path in [cache_root.join(&report.review_path), cache_root.join(&report.impact_path)] {
		assert_eq!(fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
	}
	assert_eq!(fs::metadata(pair_dir).unwrap().permissions().mode() & 0o777, 0o700);
	let review_raw = fs::read(cache_root.join(&report.review_path)).unwrap();
	let impact_raw = fs::read(cache_root.join(&report.impact_path)).unwrap();
	let impact: Value = serde_json::from_slice(&impact_raw).unwrap();
	let committed_digest = impact["review_lineage"]["artifact_sha256"]
		.as_str()
		.expect("committed impact digest should be text");

	assert_eq!(committed_digest, digest_hex(&review_raw));
	assert_eq!(committed_digest, report.review_sha256);
	assert_ne!(committed_digest, review_digest_sentinel());
	assert_eq!(pair_parts[1], report.staging_sha256);
	assert_eq!(pair_parts[1], digest_hex(&staging_raw));
	assert_eq!(pair_parts[2], pair_digest(&review_raw, &impact_raw));
	assert_report_matches_schema(&report);
}

#[test]
fn exact_retry_recovers_idempotently_and_conflicting_retry_fails_closed() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "retry-1", None);
	let exact_payload = fs::read(&staging).expect("staging should be readable");
	let first = commit_staging(&cache_root, &staging).expect("the first commit should succeed");

	let exact_staging =
		cache_root.join(format!("github/content-review-staging/{}.json", staging_run_id(&staging)));
	let exact_value: Value =
		serde_json::from_slice(&exact_payload).expect("saved staging should parse");

	crate::write_json(&exact_staging, &exact_value).expect("exact staging should be restored");
	let recovered =
		commit_staging(&cache_root, &exact_staging).expect("the exact retry should recover");

	assert_eq!(recovered.status, "recovered");
	assert_eq!(recovered.pair_dir, first.pair_dir);
	assert!(!exact_staging.exists());

	let conflicting = exact_staging;
	let mut conflicting_value: Value =
		serde_json::from_slice(&exact_payload).expect("saved staging should parse again");
	conflicting_value["review"]["evidence"][0] = serde_json::json!(format!(
		"{PATCH_ANCHOR_PATH}: Different source-backed evidence for the same run."
	));
	crate::write_json(&conflicting, &conflicting_value)
		.expect("conflicting retry should be staged");
	let error =
		commit_staging(&cache_root, &conflicting).expect_err("a changed retry must fail closed");

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
fn changed_receipt_with_the_same_review_and_impact_conflicts_on_retry() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "receipt-effect-retry", None);
	let mut payload = crate::load_json(&staging).expect("staging should load");

	commit_staging(&cache_root, &staging).expect("the original staging effect should commit");
	let mut changed_bundle = four_excerpt_bundle();

	changed_bundle["files"][1]["patch_excerpt"] =
		serde_json::json!("+fn materially_different_receipt_bytes() {}");
	install_bundle_for_staging(&cache_root, &staging, &changed_bundle, &mut payload);
	crate::write_json(&staging, &payload).expect("changed-receipt retry should be staged");
	let error = commit_staging(&cache_root, &staging)
		.expect_err("a changed receipt must not recover an older staging effect");

	assert!(error.to_string().contains("run_id already has a conflicting"));
	assert_uncommitted_pair_exists(&cache_root, &staging);
}

#[test]
fn changed_anchor_with_the_same_review_and_impact_conflicts_on_retry() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "anchor-effect-retry", None);
	let mut payload = crate::load_json(&staging).expect("staging should load");
	let evidence = serde_json::json!([
		format!("{PATCH_ANCHOR_PATH}: endpoint behavior changes."),
		format!("{SECOND_IMPLEMENTATION_PATH}: configuration behavior changes.")
	]);

	payload["review"]["evidence"] = evidence.clone();
	payload["impact"]["evidence"] = evidence;
	crate::write_json(&staging, &payload).expect("multi-anchor evidence should be staged");
	payload = crate::load_json(&staging).expect("exact first effect should reload");
	commit_staging(&cache_root, &staging).expect("the original anchor effect should commit");

	payload["patch_anchor"]["path"] = serde_json::json!(SECOND_IMPLEMENTATION_PATH);
	crate::write_json(&staging, &payload).expect("changed-anchor retry should be staged");
	let error = commit_staging(&cache_root, &staging)
		.expect_err("a changed anchor must not recover an older staging effect");

	assert!(error.to_string().contains("run_id already has a conflicting"));
	assert_uncommitted_pair_exists(&cache_root, &staging);
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
	let first = queue["subjects"][0].clone();
	queue["subjects"] = serde_json::json!([first, second]);
	set_counts(&mut queue);
	write_queue(&cache_root, &queue);
	let staging = write_staging_for_queue(&cache_root, "advance-1", &queue, None);

	commit_staging(&cache_root, &staging).expect("the first subject pair should commit");
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
	let pair = cache_root.join(format!(
		"github/content-review-pairs/{}--{}--{pair_digest}",
		case_run_id("historical-1"),
		digest_hex(b"historical staging")
	));

	crate::write_json(&pair.join("review.json"), &review).expect("review should be written");
	crate::write_json(&pair.join("impact.json"), &impact).expect("impact should be written");
	let report = crate::review_next(&review_request(&cache_root))
		.expect("historical handled state should remain valid");

	assert_eq!(report.status, "no_eligible_item");
	assert_eq!(report.handled_count, 1);
	assert!(report.selected.is_none());
}

#[test]
fn durable_pair_scan_rejects_a_coherent_artifact_mutation_with_a_stale_suffix() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "mutated-pair", None);
	let committed = commit_staging(&cache_root, &staging).expect("the original pair should commit");
	let review_path = cache_root.join(&committed.review_path);
	let impact_path = cache_root.join(&committed.impact_path);
	let mut review = crate::load_json(&review_path).expect("committed review should load");
	let mut impact = crate::load_json(&impact_path).expect("committed impact should load");

	review["evidence"][0] = serde_json::json!(format!(
		"{PATCH_ANCHOR_PATH}: coherently rewritten implementation evidence."
	));
	let review_raw = pretty_bytes(&review);
	impact["review_lineage"]["artifact_sha256"] = serde_json::json!(digest_hex(&review_raw));
	crate::write_json(&review_path, &review).expect("mutated review should be written");
	crate::write_json(&impact_path, &impact).expect("mutated impact should be written");
	let error = crate::review_next(&review_request(&cache_root))
		.expect_err("a coherent pair mutation must invalidate the durable pair suffix");

	assert!(error.to_string().contains("pair digest does not match its artifacts"));
}

#[test]
fn durable_pair_scan_rejects_a_stale_pair_suffix() {
	let (_temp, cache_root) = fresh_cache();
	let review = fixtures::valid_upstream_review();
	let review_raw = pretty_bytes(&review);
	let mut impact = fixtures::valid_upstream_impact();

	impact["review_lineage"]["artifact_sha256"] = serde_json::json!(digest_hex(&review_raw));
	let pair = cache_root.join(format!(
		"github/content-review-pairs/{}--{}--{}",
		case_run_id("stale-suffix"),
		digest_hex(b"stale staging"),
		"f".repeat(64)
	));
	crate::write_json(&pair.join("review.json"), &review).expect("review should be written");
	crate::write_json(&pair.join("impact.json"), &impact).expect("impact should be written");
	let error = crate::review_next(&review_request(&cache_root))
		.expect_err("a stale pair suffix must fail closed");

	assert!(error.to_string().contains("pair digest does not match its artifacts"));
}

#[test]
fn handled_identity_survives_queue_head_changes_but_not_commit_changes() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "head-change-1", None);

	commit_staging(&cache_root, &staging).expect("the initial pair should commit");
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
	let pair = cache_root.join(format!(
		"github/content-review-pairs/{}--{}--{}",
		case_run_id("partial-pair"),
		"a".repeat(64),
		"b".repeat(64)
	));

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
	let committed = commit_staging(&cache_root, &staging).expect("the first pair should commit");
	let review = crate::load_json(&cache_root.join(&committed.review_path)).unwrap();
	let impact = crate::load_json(&cache_root.join(&committed.impact_path)).unwrap();
	let review_raw = pretty_bytes(&review);
	let impact_raw = pretty_bytes(&impact);
	let duplicate = cache_root.join(format!(
		"github/content-review-pairs/{}--{}--{}",
		case_run_id("second-run"),
		digest_hex(b"second staging"),
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
	let committed = commit_staging(&cache_root, &staging).expect("the pair should commit");
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
	let error = commit_staging(&cache_root, &staging).expect_err("stale queue lineage must fail");

	assert!(error.to_string().contains("queue_sha256 is not current"));
	assert!(staging.exists());
	assert!(!cache_root.join("github/content-review-pairs").exists());
}

#[test]
fn content_pair_commit_rejects_stale_run_replay_and_a_malformed_process_run_id() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "stale-run-a", None);
	let mismatch = commit_with_run(&cache_root, &staging, &case_run_id("current-run-b"))
		.expect_err("a self-consistent stale run must fail before committing");

	assert!(mismatch.to_string().contains("staging path must match CODEX_THREAD_ID"));
	assert_uncommitted(&cache_root, &staging);

	let malformed_error =
		commit_with_run(&cache_root, &staging, "AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA")
			.expect_err("a non-lowercase process run ID must fail closed");

	assert!(malformed_error.to_string().contains("lowercase UUID"));
	assert_uncommitted(&cache_root, &staging);
}

#[test]
fn staging_v1_is_retired_without_a_compatibility_reader() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "retired-v1", None);
	let mut payload = crate::load_json(&staging).expect("staging should load");

	payload["schema"] = serde_json::json!("radar_content_review_pair_staging/v1");
	crate::write_json(&staging, &payload).expect("retired staging should be written");
	let error = commit_staging(&cache_root, &staging)
		.expect_err("v1 staging must be rejected rather than migrated");

	assert!(error.to_string().contains("radar_content_review_pair_staging/v2"));
	assert_uncommitted(&cache_root, &staging);
}

#[test]
fn staging_rejects_a_missing_bundle_receipt() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "missing-receipt", None);
	let mut payload = crate::load_json(&staging).expect("staging should load");

	payload.as_object_mut().expect("staging should be an object").remove("bundle_evidence_receipt");
	crate::write_json(&staging, &payload).expect("staging without a receipt should be written");
	let error = commit_staging(&cache_root, &staging)
		.expect_err("a missing bundle receipt must fail closed");

	assert!(error.to_string().contains("bundle_evidence_receipt"));
	assert_uncommitted(&cache_root, &staging);
}

#[test]
fn staging_rejects_a_mismatched_bundle_receipt() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "mismatched-receipt", None);
	let mut payload = crate::load_json(&staging).expect("staging should load");

	payload["bundle_evidence_receipt"]["bundle_sha256"] = serde_json::json!("0".repeat(64));
	crate::write_json(&staging, &payload).expect("mismatched staging should be written");
	let error = commit_staging(&cache_root, &staging)
		.expect_err("a mismatched bundle receipt must fail closed");

	assert!(error.to_string().contains("receipt does not match the run bundle"));
	assert_uncommitted(&cache_root, &staging);
}

#[test]
fn staging_binds_the_run_bundle_to_the_selected_queue_subject() {
	for (case, mutate, expected) in [
		("wrong-bundle-repo", "repo", "run bundle repo must match"),
		("wrong-bundle-pr", "pr", "primary_pr.number must match"),
		("wrong-bundle-mode", "mode", "commit_only run bundle requires a commit"),
		("wrong-bundle-commits", "commits", "commit set must exactly match"),
	] {
		let (_temp, cache_root) = fresh_cache();
		let staging = write_staging(&cache_root, case, None);
		let mut payload = crate::load_json(&staging).expect("staging should load");
		let mut bundle = four_excerpt_bundle();

		match mutate {
			"repo" => bundle["repo"] = serde_json::json!("openai/not-codex"),
			"pr" => bundle["primary_pr"]["number"] = serde_json::json!(22_415),
			"mode" => bundle["analysis_mode"] = serde_json::json!("commit_only"),
			"commits" => {
				bundle["commits"][0]["sha"] =
					serde_json::json!("cccccccccccccccccccccccccccccccccccccccc");
			},
			_ => unreachable!("test mutation must be known"),
		}
		install_bundle_for_staging(&cache_root, &staging, &bundle, &mut payload);
		crate::write_json(&staging, &payload).expect("subject-mismatch staging should be written");
		let error = commit_staging(&cache_root, &staging)
			.expect_err("bundle and selected subject mismatch must fail closed");

		assert!(error.to_string().contains(expected), "unexpected error: {error:?}");
		assert_uncommitted(&cache_root, &staging);
	}
}

#[test]
fn staging_binds_the_exact_current_review_next_selection_not_another_queue_member() {
	let (_temp, cache_root) = fresh_cache();
	let mut queue = current_queue();
	let mut second = fixtures::valid_queue_subject();
	let second_commit = "cccccccccccccccccccccccccccccccccccccccc";

	second["subject_id"] = serde_json::json!("30000");
	second["title"] = serde_json::json!("Add a second operator-visible feature");
	second["url"] = serde_json::json!("https://github.com/openai/codex/pull/30000");
	second["commit_shas"] = serde_json::json!([second_commit]);
	second["review_priority"] = serde_json::json!("normal");
	second["attention_flags"] = serde_json::json!(["new_feature"]);
	let first = queue["subjects"][0].clone();
	queue["subjects"] = serde_json::json!([first, second]);
	set_counts(&mut queue);
	write_queue(&cache_root, &queue);
	let staging = write_staging_for_queue(&cache_root, "wrong-selected-subject", &queue, None);
	let mut payload = crate::load_json(&staging).expect("staging should load");
	let mut bundle = four_excerpt_bundle();

	configure_pr_pair(&mut payload, "30000", second_commit);
	bundle["primary_pr"]["number"] = serde_json::json!(30_000);
	bundle["commits"][0]["sha"] = serde_json::json!(second_commit);
	install_bundle_for_staging(&cache_root, &staging, &bundle, &mut payload);
	crate::write_json(&staging, &payload)
		.expect("coherent second-subject staging should be written");
	let error = commit_staging(&cache_root, &staging)
		.expect_err("a coherent non-selected queue member must be rejected");

	assert!(error.to_string().contains("exact current review-next selection"));
	assert_uncommitted(&cache_root, &staging);
}

#[test]
fn staging_rejects_a_noncurrent_selection_digest() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "wrong-selection-digest", None);
	let mut payload = crate::load_json(&staging).expect("staging should load");

	payload["selection_sha256"] = serde_json::json!("0".repeat(64));
	crate::write_json(&staging, &payload).expect("wrong selection digest should be staged");
	let error = commit_staging(&cache_root, &staging)
		.expect_err("a noncurrent selection digest must fail closed");

	assert!(error.to_string().contains("selection_sha256 is not current"));
	assert_uncommitted(&cache_root, &staging);
}

#[test]
fn staging_rejects_a_pair_slug_that_differs_from_the_exact_selection() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "wrong-selection-slug", None);
	let mut payload = crate::load_json(&staging).expect("staging should load");
	let slug = "openai-codex-pr-22414-wrong";

	payload["review"]["slug"] = serde_json::json!(slug);
	payload["impact"]["slug"] = serde_json::json!(slug);
	payload["impact"]["review_lineage"]["slug"] = serde_json::json!(slug);
	crate::write_json(&staging, &payload).expect("wrong-slug staging should be written");
	let error = commit_staging(&cache_root, &staging)
		.expect_err("a pair slug outside the selected subject must fail closed");

	assert!(error.to_string().contains("exact current review-next selection"));
	assert_uncommitted(&cache_root, &staging);
}

#[test]
fn commit_only_bundle_requires_one_matching_commit_subject() {
	for (case, subject_id, should_commit) in [
		("valid-commit-only", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", true),
		("wrong-commit-subject", "cccccccccccccccccccccccccccccccccccccccc", false),
	] {
		let (_temp, cache_root) = fresh_cache();
		let mut queue = current_queue();

		configure_commit_queue(&mut queue, subject_id);
		write_queue(&cache_root, &queue);
		let staging = write_staging_for_queue(&cache_root, case, &queue, None);
		let mut payload = crate::load_json(&staging).expect("staging should load");
		let mut bundle = four_excerpt_bundle();

		configure_commit_pair(&mut payload, subject_id);
		bundle["analysis_mode"] = serde_json::json!("commit_only");
		bundle.as_object_mut().expect("bundle should be an object").remove("primary_pr");
		install_bundle_for_staging(&cache_root, &staging, &bundle, &mut payload);
		crate::write_json(&staging, &payload).expect("commit-only staging should be written");

		if should_commit {
			commit_staging(&cache_root, &staging)
				.expect("matching commit-only staging should commit");
		} else {
			let error = commit_staging(&cache_root, &staging)
				.expect_err("commit subject_id mismatch must fail closed");

			assert!(error.to_string().contains("commit SHA must match"));
			assert_uncommitted(&cache_root, &staging);
		}
	}
}

#[test]
fn staging_rejects_a_missing_or_unknown_patch_anchor() {
	for (run_id, mutation, expected) in [
		("missing-anchor", "missing", "requires patch_anchor or a nonpublishable limitation"),
		("unknown-anchor-kind", "unknown-kind", "unknown variant"),
	] {
		let (_temp, cache_root) = fresh_cache();
		let staging = write_staging(&cache_root, run_id, None);
		let mut payload = crate::load_json(&staging).expect("staging should load");

		if mutation == "missing" {
			payload.as_object_mut().expect("staging should be an object").remove("patch_anchor");
		} else {
			payload["patch_anchor"]["kind"] = serde_json::json!("documentation");
		}
		crate::write_json(&staging, &payload).expect("invalid anchor staging should be written");
		let error = commit_staging(&cache_root, &staging)
			.expect_err("an invalid patch anchor must fail closed");

		assert!(error.to_string().contains(expected), "unexpected error: {error:?}");
		assert_uncommitted(&cache_root, &staging);
	}
}

#[test]
fn staging_rejects_a_patch_anchor_outside_the_bundle() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "wrong-anchor", None);
	let mut payload = crate::load_json(&staging).expect("staging should load");

	payload["patch_anchor"]["path"] = serde_json::json!("codex-rs/not-in-bundle.rs");
	crate::write_json(&staging, &payload).expect("wrong-anchor staging should be written");
	let error = commit_staging(&cache_root, &staging)
		.expect_err("an anchor outside the bundle must fail closed");

	assert!(error.to_string().contains("does not name a run bundle file"));
	assert_uncommitted(&cache_root, &staging);
}

#[test]
fn staging_rejects_a_bundle_file_without_a_patch_excerpt_as_the_anchor() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "empty-anchor", None);
	let run_id = staging_run_id(&staging);
	let mut payload = crate::load_json(&staging).expect("staging should load");
	let mut bundle = four_excerpt_bundle();

	bundle["files"][0]["patch_excerpt"] = Value::Null;
	let receipt =
		crate::install_bundle(&cache_root.join(format!("github/bundles/{run_id}.json")), &bundle)
			.expect("changed bundle should install with an exact receipt");
	payload["bundle_evidence_receipt"] =
		serde_json::to_value(receipt).expect("receipt should serialize");
	crate::write_json(&staging, &payload).expect("empty-anchor staging should be written");
	let error = commit_staging(&cache_root, &staging)
		.expect_err("a file without a patch excerpt cannot be the anchor");

	assert!(error.to_string().contains("has no non-empty patch excerpt"));
	assert_uncommitted(&cache_root, &staging);
}

#[test]
fn staging_requires_both_review_and_impact_evidence_to_cite_the_anchor() {
	for (run_id, field, label) in [
		("review-missing-anchor", "review", "Staged upstream review"),
		("impact-missing-anchor", "impact", "Staged upstream impact"),
	] {
		let (_temp, cache_root) = fresh_cache();
		let staging = write_staging(&cache_root, run_id, None);
		let mut payload = crate::load_json(&staging).expect("staging should load");

		payload[field]["evidence"] =
			serde_json::json!(["Source-backed evidence without the required file path."]);
		crate::write_json(&staging, &payload).expect("uncited-anchor staging should be written");
		let error = commit_staging(&cache_root, &staging)
			.expect_err("both staged artifacts must cite the exact anchor path");

		assert!(error.to_string().contains(label), "unexpected error: {error:?}");
		assert!(error.to_string().contains("exact '<patch_anchor.path>: <claim>' syntax"));
		assert_uncommitted(&cache_root, &staging);
	}
}

#[test]
fn staging_rejects_patch_anchor_path_collisions_in_evidence() {
	for (case, field) in [("review-path-collision", "review"), ("impact-path-collision", "impact")]
	{
		let (_temp, cache_root) = fresh_cache();
		let staging = write_staging(&cache_root, case, None);
		let mut payload = crate::load_json(&staging).expect("staging should load");

		payload[field]["evidence"] = serde_json::json!([format!(
			"prefix-{PATCH_ANCHOR_PATH}: this is not an exact path citation"
		)]);
		crate::write_json(&staging, &payload).expect("path-collision staging should be written");
		let error = commit_staging(&cache_root, &staging)
			.expect_err("a path substring must not satisfy anchor evidence");

		assert!(error.to_string().contains("exact '<patch_anchor.path>: <claim>' syntax"));
		assert_uncommitted(&cache_root, &staging);
	}
}

#[test]
fn staging_enforces_patch_anchor_kind_against_the_exact_path() {
	for (case, path, kind, expected) in [
		(
			"test-as-implementation",
			"codex-rs/app-server/tests/endpoint.rs",
			"implementation",
			"cannot use a test path",
		),
		("implementation-as-test", PATCH_ANCHOR_PATH, "test", "must use a conservative test path"),
	] {
		let (_temp, cache_root) = fresh_cache();
		let staging = write_staging(&cache_root, case, None);
		let mut payload = crate::load_json(&staging).expect("staging should load");

		set_patch_anchor(&mut payload, path, kind);
		crate::write_json(&staging, &payload).expect("kind-mismatch staging should be written");
		let error = commit_staging(&cache_root, &staging)
			.expect_err("anchor kind and path disagreement must fail closed");

		assert!(error.to_string().contains(expected), "unexpected error: {error:?}");
		assert_uncommitted(&cache_root, &staging);
	}
}

#[test]
fn implementation_anchor_rejects_conventional_test_and_fixture_paths() {
	for (case, path) in [
		("rust-tests-module", "src/tests.rs"),
		("rust-test-module", "src/test.rs"),
		("python-tests-module", "src/tests.py"),
		("java-test-suffix", "src/FooTest.java"),
		("swift-tests-suffix", "src/FooTests.swift"),
		("underscore-tests-dir", "src/__tests__/feature.ts"),
		("testing-dir", "src/testing/feature.rs"),
		("testing-file", "src/testing.rs"),
		("integration-tests-dir", "src/integration-tests/feature.rs"),
		("integration-tests-underscore-dir", "src/integration_tests/feature.rs"),
		("integration-test-dir", "src/integration-test/feature.rs"),
		("integration-test-compact-dir", "src/integrationtests/feature.rs"),
		("integration-test-file", "src/feature_integration_test.rs"),
		("integration-test-dot-suffix", "src/feature.integration.test.ts"),
		("e2e-dir", "src/e2e/feature.ts"),
		("end-to-end-dir", "src/end-to-end/feature.ts"),
		("e2e-file", "src/feature_e2e.rs"),
		("e2e-dot-suffix", "src/feature.e2e.ts"),
		("e2e-camel-suffix", "src/FeatureE2ETest.java"),
		("fixtures-dir", "src/fixtures/feature.json"),
		("fixture-file", "src/fixture.rs"),
		("fixtures-file", "src/fixtures.json"),
		("snapshots-dir", "src/snapshots/feature.rs"),
		("snapshot-file", "src/snapshot.rs"),
		("snapshots-file", "src/snapshots.json"),
		("testdata-dir", "src/testdata/feature.proto"),
		("testdata-file", "src/testdata.rs"),
		("underscore-test-suffix", "src/feature_test.rs"),
		("spec-suffix", "src/feature.spec.ts"),
	] {
		let (_temp, cache_root) = fresh_cache();
		let staging = write_staging(&cache_root, case, None);
		let mut payload = crate::load_json(&staging).expect("staging should load");
		let mut bundle = four_excerpt_bundle();

		bundle["files"][0]["path"] = serde_json::json!(path);
		set_patch_anchor(&mut payload, path, "implementation");
		install_bundle_for_staging(&cache_root, &staging, &bundle, &mut payload);
		crate::write_json(&staging, &payload).expect("test-path anchor should be staged");
		let error = commit_staging(&cache_root, &staging)
			.expect_err("a conventional test path must not pass as implementation");

		assert!(error.to_string().contains("cannot use a test path"), "{case}: {error:?}");
		assert_uncommitted(&cache_root, &staging);
	}
}

#[test]
fn implementation_anchor_rejects_bundle_documentation_and_example_refs() {
	for (case, field, path) in [
		("documentation-anchor", "docs_refs", "docs/receipt.md"),
		("example-anchor", "examples_refs", "src/example_config.rs"),
	] {
		let (_temp, cache_root) = fresh_cache();
		let staging = write_staging(&cache_root, case, None);
		let mut payload = crate::load_json(&staging).expect("staging should load");
		let mut bundle = four_excerpt_bundle();
		let files = bundle["files"].as_array_mut().expect("bundle files should be a list");

		files.push(serde_json::json!({
			"path": path,
			"status": "modified",
			"additions": 2,
			"deletions": 0,
			"patch_excerpt": "+documentation or example text"
		}));
		bundle[field] = serde_json::json!([path]);
		set_patch_anchor(&mut payload, path, "implementation");
		install_bundle_for_staging(&cache_root, &staging, &bundle, &mut payload);
		crate::write_json(&staging, &payload).expect("reference-anchor staging should be written");
		let error = commit_staging(&cache_root, &staging)
			.expect_err("documentation and example paths cannot be implementation anchors");

		assert!(error.to_string().contains("documentation or example paths"));
		assert_uncommitted(&cache_root, &staging);
	}
}

#[test]
fn patch_anchor_classification_is_allowlist_based_and_rejects_document_surfaces() {
	for (case, path, expected) in [
		("rst-anchor", "src/architecture.rst", "documentation or example"),
		("mdx-anchor", "src/architecture.mdx", "documentation or example"),
		("changelog-anchor", "CHANGELOG", "documentation or example"),
		("website-anchor", "website/src/feature.ts", "documentation or example"),
		("content-anchor", "src/content/feature.rs", "documentation or example"),
		("guide-anchor", "guide/setup.toml", "documentation or example"),
		("unknown-extension", "src/feature.unknown", "allowlisted source"),
	] {
		let (_temp, cache_root) = fresh_cache();
		let staging = write_staging(&cache_root, case, None);
		let mut payload = crate::load_json(&staging).expect("staging should load");
		let mut bundle = four_excerpt_bundle();

		bundle["files"][0]["path"] = serde_json::json!(path);
		set_patch_anchor(&mut payload, path, "implementation");
		install_bundle_for_staging(&cache_root, &staging, &bundle, &mut payload);
		crate::write_json(&staging, &payload).expect("classified anchor should be staged");
		let error = commit_staging(&cache_root, &staging)
			.expect_err("a non-allowlisted implementation anchor must fail closed");

		assert!(error.to_string().contains(expected), "unexpected error: {error:?}");
		assert_uncommitted(&cache_root, &staging);
	}
}

#[test]
fn unknown_positive_excerpt_can_be_handled_with_the_canonical_limitation() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "unknown-limitation", None);
	let mut payload = crate::load_json(&staging).expect("staging should load");
	let mut bundle = four_excerpt_bundle();

	bundle["files"] = serde_json::json!([{
		"path": "src/feature.unknown",
		"status": "modified",
		"additions": 4,
		"deletions": 0,
		"patch_excerpt": "+opaque source format"
	}]);
	set_nonpublishable_limitation(
		&mut payload,
		"skip",
		"The only excerpt uses an unclassified file format and cannot support a typed anchor.",
	);
	install_bundle_for_staging(&cache_root, &staging, &bundle, &mut payload);
	crate::write_json(&staging, &payload).expect("unknown-format limitation should be staged");

	commit_staging(&cache_root, &staging)
		.expect("an unknown positive excerpt should become a handled limitation pair");
}

#[test]
fn zero_excerpt_staging_requires_the_canonical_nonpublishable_limitation() {
	for (run_id, set_limitation, should_commit) in
		[("zero-with-anchor", false, false), ("zero-with-limitation", true, true)]
	{
		let (_temp, cache_root) = fresh_cache();
		let staging = write_staging(&cache_root, run_id, None);
		let actual_run_id = staging_run_id(&staging);
		let mut payload = crate::load_json(&staging).expect("staging should load");
		let mut bundle = fixtures::valid_bundle();

		bundle["docs_refs"] = serde_json::json!([]);
		bundle["examples_refs"] = serde_json::json!([]);
		let receipt = crate::install_bundle(
			&cache_root.join(format!("github/bundles/{actual_run_id}.json")),
			&bundle,
		)
		.expect("zero-excerpt bundle should install with an exact receipt");
		assert_eq!(receipt.patch_excerpt_count, 0);
		payload["bundle_evidence_receipt"] =
			serde_json::to_value(receipt).expect("receipt should serialize");
		payload["impact"]["public_signal_decision"] = serde_json::json!("defer");
		payload["impact"]["publisher_angle"] = serde_json::json!("none");
		if set_limitation {
			set_zero_excerpt_limitation(
				&mut payload,
				"The deterministic bundle contains no non-empty patch excerpts.",
			);
		}
		crate::write_json(&staging, &payload).expect("zero-excerpt staging should be written");

		if should_commit {
			commit_staging(&cache_root, &staging)
				.expect("zero-excerpt staging with the canonical limitation should commit");
			assert!(!staging.exists());
		} else {
			let error = commit_staging(&cache_root, &staging)
				.expect_err("zero-excerpt staging with an anchor must fail closed");

			assert!(error.to_string().contains("requires the no_patch_excerpts limitation"));
			assert_uncommitted(&cache_root, &staging);
		}
	}
}

#[test]
fn zero_excerpt_staging_rejects_a_publish_decision() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "zero-publish", None);
	let mut payload = crate::load_json(&staging).expect("staging should load");
	let mut bundle = fixtures::valid_bundle();

	bundle["docs_refs"] = serde_json::json!([]);
	bundle["examples_refs"] = serde_json::json!([]);
	install_bundle_for_staging(&cache_root, &staging, &bundle, &mut payload);
	set_zero_excerpt_limitation(
		&mut payload,
		"The deterministic bundle contains no non-empty patch excerpts.",
	);
	payload["impact"]["public_signal_decision"] = serde_json::json!("publish");
	payload["impact"]["publisher_angle"] = serde_json::json!("operator_impact");
	crate::write_json(&staging, &payload).expect("zero-publish staging should be written");
	let error = commit_staging(&cache_root, &staging)
		.expect_err("zero excerpts must never commit a publish decision");

	assert!(error.to_string().contains("limitation requires a defer or skip"));
	assert_uncommitted(&cache_root, &staging);
}

#[test]
fn positive_excerpt_staging_can_commit_a_precise_nonpublishable_limitation() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "positive-limitation", None);
	let mut payload = crate::load_json(&staging).expect("staging should load");

	set_nonpublishable_limitation(
		&mut payload,
		"skip",
		"The available excerpts do not expose a usable implementation or test behavior anchor.",
	);
	crate::write_json(&staging, &payload).expect("limitation staging should be written");
	let report = commit_staging(&cache_root, &staging)
		.expect("a precise nonpublishable limitation should become a handled pair");

	assert_eq!(report.status, "committed");
	assert!(!staging.exists());
}

#[test]
fn anchored_pairs_enforce_decision_and_publisher_angle_equivalence() {
	for (case, decision, angle, should_commit) in [
		("publish-angle", "publish", "operator_impact", true),
		("publish-none", "publish", "none", false),
		("defer-none", "defer", "none", true),
		("defer-angle", "defer", "operator_impact", false),
		("skip-none", "skip", "none", true),
		("skip-angle", "skip", "operator_impact", false),
	] {
		let (_temp, cache_root) = fresh_cache();
		let staging = write_staging(&cache_root, case, None);
		let mut payload = crate::load_json(&staging).expect("staging should load");

		payload["impact"]["public_signal_decision"] = serde_json::json!(decision);
		payload["impact"]["publisher_angle"] = serde_json::json!(angle);
		crate::write_json(&staging, &payload).expect("decision-angle staging should be written");
		let result = commit_staging(&cache_root, &staging);

		if should_commit {
			result.expect("valid decision-angle pair should commit");
		} else {
			let error = result.expect_err("invalid decision-angle pair must fail closed");
			assert!(error.to_string().contains("publisher_angle"), "{case}: {error:?}");
			assert_uncommitted(&cache_root, &staging);
		}
	}
}

#[test]
fn committed_pair_scans_reject_invalid_decision_and_angle_relationships() {
	let (_temp, cache_root) = fresh_cache();
	let staging = write_staging(&cache_root, "committed-angle-scan", None);
	let committed = commit_staging(&cache_root, &staging).expect("fixture pair should commit");
	let review_raw = fs::read(cache_root.join(committed.review_path)).expect("committed review");
	let original_impact: Value = serde_json::from_slice(
		&fs::read(cache_root.join(committed.impact_path)).expect("committed impact"),
	)
	.expect("committed impact should parse");

	for (decision, angle) in [("publish", "none"), ("defer", "operator_impact")] {
		let mut impact = original_impact.clone();
		impact["public_signal_decision"] = serde_json::json!(decision);
		impact["publisher_angle"] = serde_json::json!(angle);
		let impact_raw = pretty_bytes(&impact);
		let error =
			crate::content_pair::validate_committed_pair_artifacts(&review_raw, &impact_raw)
				.expect_err("committed scans must enforce the decision-angle relationship");

		assert!(error.to_string().contains("publisher_angle"), "unexpected error: {error:?}");
	}
}

#[test]
fn positive_excerpt_anchorless_staging_requires_nonpublishable_precise_evidence() {
	for (case, mutation, expected) in [
		("publish-limitation", "publish", "limitation requires a defer or skip"),
		(
			"missing-limitation",
			"missing-limitation",
			"requires patch_anchor or a nonpublishable limitation",
		),
		("missing-limitation-evidence", "missing-evidence", "canonical patch limitation"),
		("extra-limitation-evidence", "extra-evidence", "canonical patch limitation"),
		("wrong-limitation-angle", "wrong-angle", "publisher_angle none"),
	] {
		let (_temp, cache_root) = fresh_cache();
		let staging = write_staging(&cache_root, case, None);
		let mut payload = crate::load_json(&staging).expect("staging should load");

		set_nonpublishable_limitation(
			&mut payload,
			"skip",
			"No implementation or test excerpt supports a concrete behavior claim.",
		);
		match mutation {
			"publish" => {
				payload["impact"]["public_signal_decision"] = serde_json::json!("publish");
				payload["impact"]["publisher_angle"] = serde_json::json!("operator_impact");
			},
			"missing-limitation" => {
				payload
					.as_object_mut()
					.expect("staging should be an object")
					.remove("patch_anchor_limitation");
			},
			"missing-evidence" => {
				payload["impact"]["evidence"] = serde_json::json!(["A different limitation."]);
			},
			"extra-evidence" => {
				payload["review"]["evidence"]
					.as_array_mut()
					.expect("review evidence should be a list")
					.push(serde_json::json!("Fabricated implementation claim."));
			},
			"wrong-angle" => {
				payload["impact"]["publisher_angle"] = serde_json::json!("operator_impact");
			},
			_ => unreachable!("test mutation must be known"),
		}
		crate::write_json(&staging, &payload)
			.expect("invalid limitation staging should be written");
		let error = commit_staging(&cache_root, &staging)
			.expect_err("unsafe anchorless staging must fail closed");

		assert!(error.to_string().contains(expected), "unexpected error: {error:?}");
		assert_uncommitted(&cache_root, &staging);
	}
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
	let error = commit_staging(&cache_root, &staging)
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
	let error =
		commit_staging(&cache_root, &staging).expect_err("a missing sentinel must be rejected");

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
	let committed = commit_staging(&cache_root, &staging).expect("the pair should commit");
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
	run_label: &str,
	queue: &Value,
	changed_evidence: Option<&str>,
) -> std::path::PathBuf {
	let run_id = case_run_id(run_label);
	let now = crate::utc_now_iso().expect("timestamp should format");
	let mut review = fixtures::valid_upstream_review();
	let mut impact = fixtures::valid_upstream_impact();
	let bundle_path = cache_root.join(format!("github/bundles/{run_id}.json"));
	let receipt = crate::install_bundle(&bundle_path, &four_excerpt_bundle())
		.expect("run bundle should install with a receipt");

	review["reviewed_at"] = serde_json::json!(now);
	impact["reviewed_at"] = serde_json::json!(now);
	review["evidence"][0] =
		serde_json::json!(format!("{PATCH_ANCHOR_PATH}: endpoint behavior changes."));
	impact["evidence"][0] =
		serde_json::json!(format!("{PATCH_ANCHOR_PATH}: operators can use the endpoint."));
	if let Some(evidence) = changed_evidence {
		review["evidence"][0] = serde_json::json!(format!("{PATCH_ANCHOR_PATH}: {evidence}"));
	}
	impact["review_lineage"]["artifact_sha256"] = serde_json::json!(review_digest_sentinel());
	let queue_raw = pretty_bytes(queue);
	let selection = crate::review_next(&review_request(cache_root))
		.expect("the production selector should authorize fixture staging");
	let selection_sha256 =
		selection.selection_sha256.expect("fixture queue should have one eligible selection");
	let staging = serde_json::json!({
		"schema": "radar_content_review_pair_staging/v2",
		"run_id": run_id,
		"queue_sha256": digest_hex(&queue_raw),
		"selection_sha256": selection_sha256,
		"bundle_evidence_receipt": receipt,
		"patch_anchor": {
			"path": PATCH_ANCHOR_PATH,
			"kind": "implementation"
		},
		"review": review,
		"impact": impact,
	});
	let path = cache_root.join(format!("github/content-review-staging/{run_id}.json"));

	crate::write_json(&path, &staging).expect("staging should be written");
	path
}

fn four_excerpt_bundle() -> Value {
	let mut bundle = fixtures::valid_bundle();

	bundle["files"][0]["patch_excerpt"] =
		serde_json::json!("+pub fn connect_endpoint() -> Endpoint");
	let files = bundle["files"].as_array_mut().expect("fixture files should be a list");

	for (index, path) in [
		"codex-rs/app-server/src/config.rs",
		"codex-rs/app-server/tests/endpoint.rs",
		"codex-rs/app-server/tests/config.rs",
	]
	.into_iter()
	.enumerate()
	{
		files.push(serde_json::json!({
			"path": path,
			"status": "modified",
			"additions": 8,
			"deletions": 1,
			"patch_excerpt": format!("+fn endpoint_anchor_{index}() {{}}")
		}));
	}
	bundle["docs_refs"] = serde_json::json!([]);
	bundle["examples_refs"] = serde_json::json!([]);
	bundle
}

fn request(cache_root: &Path, staging: &Path) -> RadarContentPairCommitRequest {
	RadarContentPairCommitRequest {
		cache_root: cache_root.to_path_buf(),
		staging: staging.to_path_buf(),
		max_age_hours: 12,
	}
}

fn commit_staging(
	cache_root: &Path,
	staging: &Path,
) -> crate::prelude::Result<crate::RadarContentPairCommitReport> {
	commit_with_run(cache_root, staging, &staging_run_id(staging))
}

fn commit_with_run(
	cache_root: &Path,
	staging: &Path,
	current_run_id: &str,
) -> crate::prelude::Result<crate::RadarContentPairCommitReport> {
	let _env = TestEnvVars::set(&[("CODEX_THREAD_ID", Some(current_run_id))]);

	crate::commit_content_pair(&request(cache_root, staging))
}

fn case_run_id(label: &str) -> String {
	let digest = digest_hex(label.as_bytes());

	format!(
		"{}-{}-{}-{}-{}",
		&digest[0..8],
		&digest[8..12],
		&digest[12..16],
		&digest[16..20],
		&digest[20..32]
	)
}

fn staging_run_id(staging: &Path) -> String {
	staging
		.file_stem()
		.and_then(|value| value.to_str())
		.expect("staging path should contain a UTF-8 run ID")
		.to_owned()
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

fn install_bundle_for_staging(
	cache_root: &Path,
	staging: &Path,
	bundle: &Value,
	payload: &mut Value,
) {
	let receipt = crate::install_bundle(
		&cache_root.join(format!("github/bundles/{}.json", staging_run_id(staging))),
		bundle,
	)
	.expect("replacement run bundle should install with an exact receipt");

	payload["bundle_evidence_receipt"] =
		serde_json::to_value(receipt).expect("receipt should serialize");
}

fn configure_commit_queue(queue: &mut Value, subject_id: &str) {
	queue["subjects"][0]["subject_kind"] = serde_json::json!("commit");
	queue["subjects"][0]["subject_id"] = serde_json::json!(subject_id);
	queue["subjects"][0]["url"] =
		serde_json::json!(format!("https://github.com/openai/codex/commit/{subject_id}"));
}

fn configure_commit_pair(payload: &mut Value, subject_id: &str) {
	let slug = format!("openai-codex-commit-{subject_id}");
	let url = format!("https://github.com/openai/codex/commit/{subject_id}");

	payload["review"]["slug"] = serde_json::json!(slug.clone());
	payload["review"]["subject"]["subject_kind"] = serde_json::json!("commit");
	payload["review"]["subject"]["subject_id"] = serde_json::json!(subject_id);
	payload["review"]["source_refs"]["items"][0]["kind"] = serde_json::json!("commit");
	payload["review"]["source_refs"]["items"][0]["url"] = serde_json::json!(url.clone());
	payload["impact"]["slug"] = serde_json::json!(slug.clone());
	payload["impact"]["review_lineage"]["slug"] = serde_json::json!(slug);
	payload["impact"]["review_lineage"]["subject_kind"] = serde_json::json!("commit");
	payload["impact"]["review_lineage"]["subject_id"] = serde_json::json!(subject_id);
	payload["impact"]["source_refs"]["items"][0]["kind"] = serde_json::json!("commit");
	payload["impact"]["source_refs"]["items"][0]["url"] = serde_json::json!(url);
}

fn configure_pr_pair(payload: &mut Value, subject_id: &str, commit_sha: &str) {
	let slug = format!("openai-codex-pr-{subject_id}");
	let url = format!("https://github.com/openai/codex/pull/{subject_id}");

	payload["review"]["slug"] = serde_json::json!(slug.clone());
	payload["review"]["subject"]["subject_id"] = serde_json::json!(subject_id);
	payload["review"]["subject"]["commit_shas"] = serde_json::json!([commit_sha]);
	payload["review"]["source_refs"]["items"][0]["url"] = serde_json::json!(url.clone());
	payload["impact"]["slug"] = serde_json::json!(slug.clone());
	payload["impact"]["review_lineage"]["slug"] = serde_json::json!(slug);
	payload["impact"]["review_lineage"]["subject_id"] = serde_json::json!(subject_id);
	payload["impact"]["review_lineage"]["commit_shas"] = serde_json::json!([commit_sha]);
	payload["impact"]["source_refs"]["items"][0]["url"] = serde_json::json!(url);
}

fn set_patch_anchor(payload: &mut Value, path: &str, kind: &str) {
	payload["patch_anchor"] = serde_json::json!({"path": path, "kind": kind});
	let review_claim = format!("{path}: concrete source behavior changes.");
	let impact_claim = format!("{path}: concrete operator behavior changes.");

	payload["review"]["evidence"] = serde_json::json!([review_claim]);
	payload["impact"]["evidence"] = serde_json::json!([impact_claim]);
}

fn set_nonpublishable_limitation(payload: &mut Value, decision: &str, detail: &str) {
	payload.as_object_mut().expect("staging should be an object").remove("patch_anchor");
	payload["patch_anchor_limitation"] = serde_json::json!({
		"reason": "no_usable_implementation_or_test_anchor",
		"detail": detail
	});
	payload["impact"]["public_signal_decision"] = serde_json::json!(decision);
	payload["impact"]["publisher_angle"] = serde_json::json!("none");
	let evidence = serde_json::json!([format!("bundle patch limitation: {detail}")]);

	payload["review"]["evidence"] = evidence.clone();
	payload["impact"]["evidence"] = evidence;
}

fn set_zero_excerpt_limitation(payload: &mut Value, detail: &str) {
	payload.as_object_mut().expect("staging should be an object").remove("patch_anchor");
	payload["patch_anchor_limitation"] = serde_json::json!({
		"reason": "no_patch_excerpts",
		"detail": detail
	});
	payload["impact"]["public_signal_decision"] = serde_json::json!("defer");
	payload["impact"]["publisher_angle"] = serde_json::json!("none");
	let evidence = serde_json::json!([format!("bundle patch limitation: {detail}")]);

	payload["review"]["evidence"] = evidence.clone();
	payload["impact"]["evidence"] = evidence;
}

fn assert_uncommitted(cache_root: &Path, staging: &Path) {
	assert!(staging.exists(), "rejected staging must remain");
	assert!(!cache_root.join("github/content-review-pairs").exists());
}

fn assert_uncommitted_pair_exists(cache_root: &Path, staging: &Path) {
	assert!(staging.exists(), "rejected retry staging must remain");
	assert_eq!(
		fs::read_dir(cache_root.join("github/content-review-pairs"))
			.expect("the original pair collection should remain")
			.count(),
		1
	);
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
	assert!(
		schema["required"]
			.as_array()
			.expect("required should be a list")
			.contains(&serde_json::json!("bundle_evidence_receipt"))
	);
	assert_eq!(
		schema["properties"]["bundle_evidence_receipt"]["$ref"],
		"bundle_build_receipt.schema.json"
	);
	assert_eq!(
		schema["properties"]["patch_anchor"]["properties"]["kind"]["enum"],
		serde_json::json!(["implementation", "test"])
	);
	assert_eq!(schema["properties"]["schema"]["const"], "radar_content_review_pair_staging/v2");
	assert_eq!(
		schema["properties"]["patch_anchor_limitation"]["properties"]["reason"]["enum"],
		serde_json::json!(["no_patch_excerpts", "no_usable_implementation_or_test_anchor"])
	);
	assert_eq!(schema["oneOf"].as_array().expect("v2 branches should be a list").len(), 3);
}
