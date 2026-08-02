use std::{
	fs,
	ops::Deref,
	path::{Path, PathBuf},
};

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{
	SocialObserveXurlRequest, SocialPublishXurlRequest, SocialReconcileXurlRequest,
	SocialReservePublishRequest, SocialSealXurlAuthRequest, SocialTerminalizeSkipRequest,
	social_validation::SocialValidationState,
};

const RUN_ID: &str = "019fa400-0000-7000-8000-000000000001";
const SECOND_RUN_ID: &str = "019fa400-0000-7000-8000-000000000002";
const THIRD_RUN_ID: &str = "019fa400-0000-7000-8000-000000000003";
const FOURTH_RUN_ID: &str = "019fa400-0000-7000-8000-000000000004";
const POST_TEXT: &str = "Codex app-server now exposes a typed capability check before experimental calls, so operators can detect unsupported protocol surfaces before a workflow starts.";
const PLACEHOLDER_REVIEW_REF: &str = ".agent/automations/radar/cache/github/content-review-pairs/019fa400-0000-7000-8000-000000000001--aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa--bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/review.json";
const PLACEHOLDER_IMPACT_REF: &str = ".agent/automations/radar/cache/github/content-review-pairs/019fa400-0000-7000-8000-000000000001--aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa--bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/impact.json";
const TEST_PUBLICATION_LINEAGE: &str =
	"e9efcaaa0b3eea16244c69fcffc22f97a21c0338f1071ee86d9b59cd9e2c1bd9";
const TEST_IDEMPOTENCY_KEY: &str =
	"radar-publication:e9efcaaa0b3eea16244c69fcffc22f97a21c0338f1071ee86d9b59cd9e2c1bd9";

#[test]
fn validates_fresh_xurl_contracts() {
	assert_social_errors(&valid_social_candidate(), []);
	assert_social_errors(&valid_social_publish_reservation(), []);
	assert_social_errors(&valid_social_post(), []);
	assert_social_errors(&valid_social_outcome(), []);
}

#[test]
fn rejects_browser_and_controller_legacy_fields() {
	let mut post = valid_social_post();
	post["browser_touched"] = json!(true);
	post["browser_session"] = json!({});
	post["controller_account"] = json!("hackink");
	assert_social_errors(
		&post,
		[
			"social_post.browser_session is not allowed",
			"social_post.browser_touched is not allowed",
			"social_post.controller_account is not allowed",
		],
	);

	let mut reservation = valid_social_publish_reservation();
	reservation["controller_account"] = json!("hackink");
	assert_social_errors(
		&reservation,
		["social_publish_reservation.controller_account is not allowed"],
	);
}

#[test]
fn rejects_unbound_social_post_owner() {
	let mut post = valid_social_post();
	post["owner"]["automation_id"] = json!("other");
	post["owner"]["run_id"] = json!("not-a-run");

	assert_social_errors(
		&post,
		[
			"owner.automation_id must be decodex-xurl-publisher",
			"owner.run_id must be a lowercase UUID",
		],
	);
}

#[test]
fn canonical_validation_rejects_minimal_retention_projections() {
	for value in [
		json!({
			"schema": "social_candidate/v1",
			"decision": {"worthiness": "publish"}
		}),
		json!({"schema": "social_strategy/v1"}),
		json!({
			"schema": "social_post/v1",
			"status": "published",
			"owner": {
				"automation_id": "decodex-xurl-publisher",
				"run_id": RUN_ID
			}
		}),
		json!({
			"schema": "social_outcome/v1",
			"owner": {
				"automation_id": "decodex-xurl-publisher",
				"run_id": RUN_ID
			}
		}),
	] {
		assert!(
			crate::validate_generated_social_artifact(&value).is_err(),
			"minimal retention projection must not be authoritative: {value}"
		);
	}
}

#[test]
fn rejects_unsupported_media_publication_fields() {
	let mut candidate = valid_social_candidate();
	candidate["media_refs"] = json!(["asset.png"]);
	assert_social_errors(&candidate, ["social_candidate.media_refs is not allowed"]);

	let mut post = valid_social_post();
	post["media_refs"] = json!(["https://x.com/media/1"]);
	post["publication"]["image_template"] = json!("decodex_signal_card");
	assert_social_errors(
		&post,
		["publication.image_template is not allowed", "social_post.media_refs is not allowed"],
	);
}

#[test]
fn rejects_unsupported_multi_post_mode() {
	for mut artifact in
		[valid_social_candidate(), valid_social_publish_reservation(), valid_social_post()]
	{
		artifact["mode"] = json!("thread");
		assert_social_errors(
			&artifact,
			[
				"mode must be one of ['operator_impact', 'practical_explainer', 'release_pulse', 'release_rollup', 'watch_note']",
			],
		);
	}
}

#[test]
fn rejects_url_bearing_public_text_and_multiple_publish_items() {
	let mut candidate = valid_social_candidate();
	let linked_text = "Codex operators can inspect the protocol change at https://github.com/openai/codex/pull/1 before adopting the new workflow.";
	candidate["candidate_text"] = json!([linked_text]);
	candidate["claims"][0]["text"] = json!(linked_text);
	assert_social_errors(
		&candidate,
		["text[0] must not contain URL, domain, email, or other link-like text"],
	);

	let mut post = valid_social_post();
	post["text"] = json!([POST_TEXT, "A second post is not allowed."]);
	assert_social_errors(&post, ["published text must contain exactly one item"]);
}

#[test]
fn rejects_short_publish_text_and_free_form_claim_evidence() {
	let short = "界".repeat(79);
	let mut candidate = valid_social_candidate();
	candidate["candidate_text"] = json!([short.clone()]);
	candidate["claims"][0]["text"] = json!(short);
	assert_social_errors(
		&candidate,
		["publish candidate_text item must contain at least 80 Unicode characters"],
	);

	let mut post = valid_social_post();
	post["text"] = json!(["界".repeat(79)]);
	assert_social_errors(
		&post,
		["published text item must contain at least 80 Unicode characters"],
	);

	let mut candidate = valid_social_candidate();
	candidate["claims"][0]["evidence"] = json!("A free-form summary is not a source reference.");
	assert_social_errors(
		&candidate,
		[
			"claims[0].evidence must bind one verified Radar review or impact",
			"claims[0].evidence must exactly match one declared source reference",
		],
	);

	let mut post = valid_social_post();
	post["source_refs"].as_object_mut().expect("source refs").remove("urls");
	let error = crate::social_evidence::validate_internal_evidence_files(&post)
		.expect_err("published claims cannot defer an unresolved source to lineage")
		.to_string();
	assert!(error.contains("does not resolve to a declared source reference"), "{error}");
}

#[cfg(unix)]
#[test]
fn internal_evidence_digest_is_rechecked_before_reservation_and_publication() {
	let repo_root = crate::repo_root().expect("repo root");
	let temp = crate::repo_local_test_directory("publisher-evidence-");
	let evidence_path = temp.path().join("evidence/signal.json");
	let original_evidence = json!({
		"schema": "signal_entry/v1",
		"summary": "Typed capability checks are now operator-visible."
	});
	crate::write_new_json(&evidence_path, &original_evidence).expect("private evidence");
	let evidence_ref = crate::path_arg(&repo_root, &evidence_path);
	let evidence_sha256 = crate::load_json_with_sha256(&evidence_path).expect("evidence digest").1;
	let mut candidate = valid_social_candidate();
	candidate["source_refs"]["signals"] = json!([evidence_ref.clone()]);
	candidate["evidence_digests"]
		.as_object_mut()
		.expect("evidence digests")
		.insert(evidence_ref.clone(), Value::String(evidence_sha256));
	let mut mismatched_candidate = candidate.clone();
	mismatched_candidate["evidence_digests"]
		.as_object_mut()
		.expect("evidence digests")
		.insert(evidence_ref, Value::String("0".repeat(64)));
	let mismatched_candidate_path =
		write_candidate_named(temp.path(), "mismatched.json", mismatched_candidate);
	let error = crate::reserve_social_publish(&reserve_request(
		temp.path(),
		&mismatched_candidate_path,
		SECOND_RUN_ID,
	))
	.expect_err("mismatched evidence must stop before reservation")
	.to_string();
	assert!(error.contains("does not match its immutable content digest"), "{error}");

	let candidate_path = write_candidate(temp.path(), candidate);
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("digest-bound evidence should reserve");

	let changed_evidence = json!({
		"schema": "signal_entry/v1",
		"summary": "The source was replaced after reservation."
	});
	crate::replace_existing_json(&evidence_path, &original_evidence, &changed_evidence)
		.expect("replace evidence");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let request = publish_request(temp.path(), Path::new(&reservation.path), RUN_ID);
	let error = crate::social_xurl::publish_with_test_binary(&request, &xurl)
		.expect_err("changed evidence must stop before X")
		.to_string();
	assert!(error.contains("does not match its immutable content digest"), "{error}");
	assert!(!log_path.exists());

	crate::replace_existing_json(&evidence_path, &changed_evidence, &original_evidence)
		.expect("restore evidence");
	let report = crate::social_xurl::publish_with_test_binary(&request, &xurl)
		.expect("restored digest-bound evidence should publish");
	assert_eq!(report.status, "published");
}

#[test]
fn internal_evidence_rejects_missing_and_unsupported_sources() {
	let repo_root = crate::repo_root().expect("repo root");
	let temp = crate::repo_local_test_directory("publisher-evidence-");
	let missing_path = temp.path().join("evidence/missing.json");
	let missing_ref = crate::path_arg(&repo_root, &missing_path);
	let mut missing_candidate = valid_social_candidate();
	missing_candidate["source_refs"]["signals"] = json!([missing_ref.clone()]);
	missing_candidate["evidence_digests"]
		.as_object_mut()
		.expect("evidence digests")
		.insert(missing_ref, json!("0".repeat(64)));
	let missing_candidate_path =
		write_candidate_named(temp.path(), "missing.json", missing_candidate);
	let error = crate::reserve_social_publish(&reserve_request(
		temp.path(),
		&missing_candidate_path,
		RUN_ID,
	))
	.expect_err("missing evidence must fail closed")
	.to_string();
	assert!(error.contains("candidate evidence") && error.contains("invalid"), "{error}");

	let unsupported_path = temp.path().join("evidence/unsupported.json");
	crate::write_new_json(&unsupported_path, &json!({"schema": "unsupported/v1"}))
		.expect("unsupported evidence");
	let unsupported_ref = crate::path_arg(&repo_root, &unsupported_path);
	let unsupported_sha256 =
		crate::load_json_with_sha256(&unsupported_path).expect("unsupported digest").1;
	let mut unsupported_candidate = valid_social_candidate();
	unsupported_candidate["source_refs"]["signals"] = json!([unsupported_ref.clone()]);
	unsupported_candidate["evidence_digests"]
		.as_object_mut()
		.expect("evidence digests")
		.insert(unsupported_ref, json!(unsupported_sha256));
	let unsupported_candidate_path =
		write_candidate_named(temp.path(), "unsupported.json", unsupported_candidate);
	let error = crate::reserve_social_publish(&reserve_request(
		temp.path(),
		&unsupported_candidate_path,
		SECOND_RUN_ID,
	))
	.expect_err("unsupported evidence schema must fail closed")
	.to_string();
	assert!(error.contains("must use schema signal_entry/v1"), "{error}");
}

#[test]
fn rejects_unverified_xurl_publication_evidence() {
	let mut post = valid_social_post();
	post["publication"]["publisher"] = json!("chrome");
	post["publication"]["verified_account"] = json!("hackink");
	post["publication"]["recorded_cost_ceiling_microusd"] = json!(205_000);
	assert_social_errors(
		&post,
		[
			"publication.recorded_cost_ceiling_microusd must be 30000, 35000, or 40000",
			"publication.publisher must be xurl",
			"publication.verified_account must be decodexspace",
		],
	);

	post = valid_social_post();
	post["publication"]["read_response_sha256"] = json!("not-a-digest");
	assert_social_errors(
		&post,
		["publication.read_response_sha256 must be a lowercase SHA-256 digest"],
	);
}

#[test]
fn rejects_browser_outcome_evidence() {
	let mut outcome = valid_social_outcome();
	outcome.as_object_mut().expect("outcome is an object").remove("observation");
	outcome["browser_session"] = json!({
		"initial_account": "hackink",
		"target_account": "decodexspace"
	});
	assert_social_errors(
		&outcome,
		["observation must be an object", "social_outcome.browser_session is not allowed"],
	);
}

#[test]
fn duplicate_active_reservations_and_outcome_windows_fail_cross_file_validation() {
	let mut state = SocialValidationState::new();
	let reservation = valid_social_publish_reservation();
	let mut errors = Vec::new();
	crate::social_validation::validate_social_cross_file_constraints(
		Path::new("reservations/one.json"),
		&reservation,
		&mut state,
		&mut errors,
	);
	crate::social_validation::validate_social_cross_file_constraints(
		Path::new("reservations/two.json"),
		&reservation,
		&mut state,
		&mut errors,
	);
	assert!(errors.iter().any(|error| error.contains("duplicate active")));

	let mut state = SocialValidationState::new();
	let outcome = valid_social_outcome();
	let mut errors = Vec::new();
	crate::social_validation::validate_social_cross_file_constraints(
		Path::new("outcomes/one.json"),
		&outcome,
		&mut state,
		&mut errors,
	);
	crate::social_validation::validate_social_cross_file_constraints(
		Path::new("outcomes/two.json"),
		&outcome,
		&mut state,
		&mut errors,
	);
	assert!(errors.iter().any(|error| error.contains("duplicate social_outcome cycle")));
}

#[test]
fn publication_lineage_rejects_valid_but_mismatched_post_owner() {
	let candidate_path =
		".agent/automations/decodex/cache/social/x/candidates/openai-codex-pr-22414.json";
	let reservation_path =
		".agent/automations/decodex/cache/social/x/reservations/2026-07-27/reservation.json";
	let post_path =
		".agent/automations/decodex/cache/social/x/posts/019fa400-0000-7000-8000-000000000001.json";
	let candidate = valid_social_candidate();
	let mut reservation = valid_social_publish_reservation();
	reservation["status"] = json!("consumed");
	reservation["consumed_by_social_post"] = json!(post_path);
	let mut post = valid_social_post();
	post["owner"]["run_id"] = json!(SECOND_RUN_ID);
	let mut state = SocialValidationState::new();
	let mut errors = Vec::new();

	for (path, value) in
		[(candidate_path, &candidate), (reservation_path, &reservation), (post_path, &post)]
	{
		crate::social_validation::validate_social_cross_file_constraints(
			Path::new(path),
			value,
			&mut state,
			&mut errors,
		);
	}
	state.finish(&mut errors);

	assert!(
		errors.iter().any(|error| error.contains("published social_post lineage does not match")),
		"{errors:?}"
	);
}

#[test]
fn publish_text_requires_exact_ordered_claim_composition() {
	let mut candidate = valid_social_candidate();
	candidate["candidate_text"] = json!([format!("{POST_TEXT} It is available today.")]);
	let errors = crate::social_validation::validate_social_artifact(&candidate).errors;
	assert!(
		errors.iter().any(|error| error.contains("canonical ordered claim composition")),
		"{errors:?}"
	);

	let first = "Codex app-server exposes a typed capability check.";
	let second = "Operators can reject unsupported calls before a workflow starts.";
	let connective = " It also ships today. ";
	let review_ref = candidate["radar_source_refs"]["review"].clone();
	candidate["candidate_text"] = json!([format!("{first}{connective}{second}")]);
	candidate["claims"] = json!([
		{"text": first, "evidence": review_ref, "confidence": "confirmed"},
		{"text": second, "evidence": review_ref, "confidence": "confirmed"}
	]);
	candidate["text_segments"] = json!([
		{"kind": "claim", "claim_index": 0},
		{"kind": "connective", "text": connective},
		{"kind": "claim", "claim_index": 1}
	]);
	let errors = crate::social_validation::validate_social_artifact(&candidate).errors;
	assert!(
		errors.iter().any(|error| error.contains("approved non-factual connective")),
		"{errors:?}"
	);

	let connective = " ";
	candidate["candidate_text"] = json!([format!("{first}{connective}{second}")]);
	candidate["text_segments"][1]["text"] = json!(connective);
	let errors = crate::social_validation::validate_social_artifact(&candidate).errors;
	assert!(
		!errors.iter().any(|error| {
			error.contains("canonical ordered claim composition")
				|| error.contains("approved non-factual connective")
				|| error.contains("cover claims once in order")
		}),
		"{errors:?}"
	);

	candidate["text_segments"][0]["claim_index"] = json!(1);
	let errors = crate::social_validation::validate_social_artifact(&candidate).errors;
	assert!(errors.iter().any(|error| error.contains("cover claims once in order")), "{errors:?}");
}

#[test]
fn radar_lineage_rejects_source_tampering_and_wrong_receipt_values() {
	let path_only_error = crate::validate_generated_social_artifact(&valid_social_candidate())
		.expect_err("agent-authored path-only eligibility must fail")
		.to_string();
	assert!(
		path_only_error.contains("private JSON") || path_only_error.contains("No such file"),
		"{path_only_error}"
	);

	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_fixture = write_candidate(temp.path(), valid_social_candidate());
	let candidate = crate::load_json(&candidate_fixture).expect("candidate");
	let review_ref = candidate["radar_source_refs"]["review"].as_str().expect("review ref");
	let review_path = crate::repo_root().expect("repo root").join(review_ref);
	let original = crate::load_json(&review_path).expect("review source");
	let mut tampered = original.clone();
	tampered["observed_change"] = json!("A substituted observation.");
	crate::replace_existing_json(&review_path, &original, &tampered).expect("tamper review");
	let error = crate::social_record::validate_candidate_eligibility(&candidate)
		.expect_err("raw source tampering must fail")
		.to_string();
	assert!(error.contains("pair digest") || error.contains("review digest"), "{error}");
	crate::replace_existing_json(&review_path, &tampered, &original).expect("restore review");

	for (field, wrong) in [
		("upstream_head", "cccccccccccccccccccccccccccccccccccccccc".to_owned()),
		(
			"review_sha256",
			"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_owned(),
		),
	] {
		let mut wrong_candidate = candidate.clone();
		wrong_candidate["radar_eligibility"][field] = json!(wrong);
		let error = crate::social_record::validate_candidate_eligibility(&wrong_candidate)
			.expect_err("wrong eligibility field must fail")
			.to_string();
		assert!(error.contains("does not match"), "{field}: {error}");
	}

	let mut wrong_commits = candidate;
	wrong_commits["radar_eligibility"]["commit_shas"] =
		json!(["eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"]);
	let error = crate::social_record::validate_candidate_eligibility(&wrong_commits)
		.expect_err("wrong commit set must fail")
		.to_string();
	assert!(error.contains("commit_shas"), "{error}");
}

#[test]
fn radar_lineage_rejects_wrong_collection_and_mixed_cache_roots() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let first = write_candidate_named(temp.path(), "first.json", valid_social_candidate());
	let second = write_candidate_named(temp.path(), "second.json", valid_social_candidate());
	let first_payload = crate::load_json(&first).expect("first candidate");
	let second_payload = crate::load_json(&second).expect("second candidate");

	let mut wrong_collection = first_payload.clone();
	wrong_collection["radar_source_refs"]["review"] =
		wrong_collection["radar_source_refs"]["impact"].clone();
	let error = crate::social_record::validate_candidate_eligibility(&wrong_collection)
		.expect_err("wrong collection must fail")
		.to_string();
	assert!(error.contains("review source"), "{error}");

	let mut legacy = first_payload.clone();
	legacy["radar_source_refs"]["review"] =
		json!(".agent/automations/radar/cache/github/reviews/legacy.json");
	let error = crate::social_record::validate_candidate_eligibility(&legacy)
		.expect_err("the removed review collection must fail")
		.to_string();
	assert!(error.contains("Radar pair path"), "{error}");

	let mut replayed_queue = first_payload.clone();
	let queue_ref = replayed_queue["radar_source_refs"]["queue"].as_str().expect("queue ref");
	replayed_queue["radar_source_refs"]["queue"] =
		json!(Path::new(queue_ref).with_file_name("replayed.json").to_string_lossy());
	let error = crate::social_record::validate_candidate_eligibility(&replayed_queue)
		.expect_err("an alternate queue path must not carry lineage authority")
		.to_string();
	assert!(error.contains("exact canonical private Radar queue path"), "{error}");
	let schema_errors = crate::social_validation::validate_social_artifact(&replayed_queue).errors;
	assert!(
		schema_errors
			.iter()
			.any(|error| error.contains("exact canonical private Radar queue path")),
		"{schema_errors:?}"
	);

	let old_pair = format!("{RUN_ID}--{}", "a".repeat(64));
	let mut old_shape = first_payload.clone();
	old_shape["radar_source_refs"]["review"] = json!(format!(
		".agent/automations/radar/cache/github/content-review-pairs/{old_pair}/review.json"
	));
	old_shape["radar_source_refs"]["impact"] = json!(format!(
		".agent/automations/radar/cache/github/content-review-pairs/{old_pair}/impact.json"
	));
	let error = crate::social_record::validate_candidate_eligibility(&old_shape)
		.expect_err("the retired two-part pair path must fail")
		.to_string();
	assert!(error.contains("pair directory is malformed"), "{error}");

	let review_ref = first_payload["radar_source_refs"]["review"].as_str().expect("review ref");
	let review_path = crate::repo_root().expect("repo root").join(review_ref);
	let pair_dir = review_path.parent().expect("pair directory");
	let pair_digest = pair_dir
		.file_name()
		.and_then(|name| name.to_str())
		.and_then(|name| name.split("--").nth(2))
		.expect("pair digest");
	let alternate_pair = pair_dir
		.parent()
		.expect("pair collection")
		.join(format!("{SECOND_RUN_ID}--{}--{pair_digest}", "c".repeat(64)));
	crate::ensure_private_directory(&alternate_pair).expect("alternate pair");
	for file in ["review.json", "impact.json"] {
		let payload = crate::load_json(&pair_dir.join(file)).expect("paired source");
		crate::write_new_json(&alternate_pair.join(file), &payload).expect("alternate pair source");
	}
	let mut cross_pair = first_payload.clone();
	cross_pair["radar_source_refs"]["impact"] = json!(crate::path_arg(
		&crate::repo_root().expect("repo root"),
		&alternate_pair.join("impact.json")
	));
	let error = crate::social_record::validate_candidate_eligibility(&cross_pair)
		.expect_err("review and impact from different canonical pair directories must fail")
		.to_string();
	assert!(error.contains("share one canonical content-review pair directory"), "{error}");

	let mut mixed = first_payload;
	mixed["radar_source_refs"]["queue"] = second_payload["radar_source_refs"]["queue"].clone();
	let error = crate::social_record::validate_candidate_eligibility(&mixed)
		.expect_err("mixed cache roots must fail")
		.to_string();
	assert!(error.contains("one private cache root"), "{error}");
}

#[test]
fn manager_record_command_writes_once_and_cleans_staging() {
	use std::os::unix::fs::PermissionsExt as _;

	let temp = tempfile::tempdir().expect("temporary directory");
	let staging =
		write_staged_candidate(temp.path(), "candidate-stage.json", valid_social_candidate());
	let request = manager_record_request(temp.path(), &staging, RUN_ID);
	let report = crate::record_social_manager(&request).expect("manager record");
	let destination = temp.path().join("candidates").join(format!("{RUN_ID}.json"));

	assert_eq!(report.status, "recorded");
	assert_eq!(report.kind, "candidate");
	assert!(report.staging_cleaned);
	assert!(!staging.exists());
	assert!(destination.is_file());
	assert_eq!(
		fs::metadata(destination).expect("destination metadata").permissions().mode() & 0o777,
		0o600
	);
}

#[test]
fn manager_record_recovers_crash_and_refuses_overwrite_or_second_effect() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let staging = write_staged_candidate(temp.path(), "crash-stage.json", valid_social_candidate());
	let request = manager_record_request(temp.path(), &staging, RUN_ID);
	let error = crate::social_record::record_social_manager_with_hook(&request, |point| {
		if point == crate::social_record::SocialRecordHookPoint::AuthoritativeWritten {
			return Err(crate::prelude::eyre::eyre!("simulated crash"));
		}

		Ok(())
	})
	.expect_err("simulated crash")
	.to_string();
	assert!(error.contains("simulated crash"), "{error}");
	assert!(staging.exists());
	assert!(temp.path().join("candidates").join(format!("{RUN_ID}.json")).exists());

	let recovered = crate::record_social_manager(&request).expect("crash recovery");
	assert_eq!(recovered.status, "already_recorded");
	assert!(!staging.exists());

	let mut changed = valid_social_candidate();
	let changed_text = "Codex app-server adds a different checked capability path, so operators can reject unsupported calls before they start a workflow.";
	changed["candidate_text"] = json!([changed_text]);
	changed["claims"][0]["text"] = json!(changed_text);
	let changed_staging = write_staged_candidate(temp.path(), "changed.json", changed);
	let changed_request = manager_record_request(temp.path(), &changed_staging, RUN_ID);
	let error = crate::record_social_manager(&changed_request)
		.expect_err("overwrite must fail")
		.to_string();
	assert!(error.contains("refusing to overwrite"), "{error}");
	assert!(changed_staging.exists());

	let strategy_path = write_staging_value(
		temp.path(),
		"strategy.json",
		&valid_social_strategy("daily-2026-07-27"),
	);
	let strategy_request = manager_record_request(temp.path(), &strategy_path, RUN_ID);
	let error = crate::record_social_manager(&strategy_request)
		.expect_err("second effect must fail")
		.to_string();
	assert!(error.contains("different Content Manager effect"), "{error}");
	assert!(strategy_path.exists());
}

#[test]
fn manager_record_enforces_candidate_backpressure() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let first = write_staged_candidate(temp.path(), "first.json", valid_social_candidate());
	let first_request = manager_record_request(temp.path(), &first, RUN_ID);
	crate::record_social_manager(&first_request).expect("first candidate");

	let mut second_candidate = valid_social_candidate();
	second_candidate["slug"] = json!("openai-codex-pr-22414-second-angle");
	second_candidate["decision"]["idempotency_key"] =
		json!("x:decodexspace:operator_impact:openai-codex-pr-22414-second-angle");
	let second = write_staged_candidate(temp.path(), "second.json", second_candidate);
	let second_request = manager_record_request(temp.path(), &second, SECOND_RUN_ID);
	let error = crate::record_social_manager(&second_request)
		.expect_err("unresolved candidate must apply backpressure")
		.to_string();
	assert!(error.contains("candidate backpressure is active"), "{error}");
	assert!(second.exists());
}

#[test]
fn manager_record_lock_serializes_two_writer_race() {
	use std::{
		sync::{Arc, Barrier, mpsc},
		thread,
		time::Duration as StdDuration,
	};

	let temp = tempfile::tempdir().expect("temporary directory");
	let first = write_staged_candidate(temp.path(), "first-race.json", valid_social_candidate());
	let mut second_payload = valid_social_candidate();
	second_payload["slug"] = json!("openai-codex-pr-22414-race-two");
	second_payload["decision"]["idempotency_key"] =
		json!("x:decodexspace:operator_impact:openai-codex-pr-22414-race-two");
	let second = write_staged_candidate(temp.path(), "second-race.json", second_payload);
	let first_request = manager_record_request(temp.path(), &first, RUN_ID);
	let second_request = manager_record_request(temp.path(), &second, SECOND_RUN_ID);
	let release = Arc::new(Barrier::new(2));
	let first_release = Arc::clone(&release);
	let (locked_tx, locked_rx) = mpsc::channel();

	let first_writer = thread::spawn(move || {
		let _keep_sources = first;
		let mut locked_tx = Some(locked_tx);
		crate::social_record::record_social_manager_with_hook(&first_request, |point| {
			if point == crate::social_record::SocialRecordHookPoint::Locked {
				locked_tx.take().expect("single lock notification").send(()).expect("notify lock");
				first_release.wait();
			}

			Ok(())
		})
	});
	locked_rx.recv_timeout(StdDuration::from_secs(2)).expect("first writer lock");
	let second_writer = thread::spawn(move || {
		let _keep_sources = second;
		crate::record_social_manager(&second_request)
	});
	thread::sleep(StdDuration::from_millis(100));
	assert!(!second_writer.is_finished(), "second writer bypassed the mutation lock");
	release.wait();

	let first_report = first_writer.join().expect("first writer thread").expect("first write");
	assert_eq!(first_report.status, "recorded");
	let second_error =
		second_writer.join().expect("second writer thread").expect_err("backpressure after lock");
	assert!(second_error.to_string().contains("candidate backpressure is active"));
}

#[test]
fn reserve_publish_is_atomic_and_enforces_one_post_per_day() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let request = reserve_request(temp.path(), &candidate_path, RUN_ID);
	let first = crate::reserve_social_publish(&request).expect("reservation should succeed");
	assert_eq!(first.status, "reserved");
	assert_eq!(first.daily_limit, 1);

	let duplicate = crate::reserve_social_publish(&request)
		.expect_err("same idempotency key must not reserve twice")
		.to_string();
	assert!(duplicate.contains("idempotency_key already has"));

	let mut other = valid_social_candidate();
	other["slug"] = json!("another-change");
	other["decision"]["idempotency_key"] = json!("x:decodexspace:operator_impact:another-change");
	let other_path = write_candidate_named(temp.path(), "other.json", other);
	let mut other_request =
		reserve_request(temp.path(), &other_path, "019fa400-0000-7000-8000-000000000002");
	other_request.reserved_at = "2026-07-27T12:01:00Z".into();
	let cap = crate::reserve_social_publish(&other_request)
		.expect_err("active reservation must consume the one-post daily cap")
		.to_string();
	assert!(cap.contains("daily publish cap exhausted"));
}

#[test]
fn quality_skip_terminalization_is_idempotent_and_does_not_call_x() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let mut candidate = valid_social_candidate();
	candidate["decision"]["worthiness"] = json!("skip");
	candidate["decision"]["reason"] = json!("No material operator consequence.");
	candidate.as_object_mut().expect("candidate").remove("radar_eligibility");
	candidate.as_object_mut().expect("candidate").remove("radar_source_refs");
	candidate["source_refs"] = json!({"urls": ["https://github.com/openai/codex/pull/22414"]});
	candidate.as_object_mut().expect("candidate").remove("evidence_digests");
	candidate["claims"][0]["evidence"] = json!("https://github.com/openai/codex/pull/22414");
	let candidate_path = write_candidate(temp.path(), candidate);
	let request = skip_request(temp.path(), &candidate_path);
	let first = crate::terminalize_social_skip(&request).expect("skip should succeed");
	let second = crate::terminalize_social_skip(&request).expect("exact skip retry should succeed");
	assert_eq!(first.status, "skipped");
	assert_eq!(second.status, "already_skipped");
	let post = crate::load_json(Path::new(&first.path)).expect("skip record");
	assert_eq!(post["decision"]["daily_limit"], 1);
	assert!(post.get("browser_touched").is_none());
	assert!(post.get("publication").is_none());
}

#[test]
fn quality_skip_rejects_and_cannot_propagate_retired_radar_pair_paths() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let old_pair = format!("{RUN_ID}--{}", "a".repeat(64));
	let old_review = format!(
		".agent/automations/radar/cache/github/content-review-pairs/{old_pair}/review.json"
	);
	let old_impact = format!(
		".agent/automations/radar/cache/github/content-review-pairs/{old_pair}/impact.json"
	);
	let mut candidate = valid_social_candidate();

	candidate["decision"]["worthiness"] = json!("skip");
	candidate["decision"]["reason"] = json!("No material operator consequence.");
	candidate.as_object_mut().expect("candidate").remove("radar_eligibility");
	candidate.as_object_mut().expect("candidate").remove("radar_source_refs");
	candidate["source_refs"] = json!({
		"upstream_reviews": [old_review.clone()],
		"upstream_impacts": [old_impact.clone()]
	});
	candidate["evidence_digests"] = json!({
		(old_review.clone()): "1".repeat(64),
		(old_impact): "2".repeat(64)
	});
	candidate["claims"][0]["evidence"] = json!(old_review);

	let errors = crate::social_validation::validate_social_artifact(&candidate).errors;
	assert!(errors.iter().any(|error| error.contains("strict fresh Radar pair")), "{errors:?}");
	let runtime_error = crate::social_record::validate_candidate_eligibility(&candidate)
		.expect_err("skip eligibility must reject old pair paths")
		.to_string();
	assert!(runtime_error.contains("Radar pair") || runtime_error.contains("pair directory"));

	let candidate_path = write_candidate(temp.path(), candidate);
	let error = crate::terminalize_social_skip(&skip_request(temp.path(), &candidate_path))
		.expect_err("skip terminalization must not propagate an old pair path")
		.to_string();
	assert!(error.contains("strict fresh Radar pair"), "{error}");
	assert!(!temp.path().join("posts").exists());
}

#[cfg(unix)]
#[test]
fn xurl_publish_creates_reads_back_and_atomically_terminalizes() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation should succeed");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let request = publish_request(temp.path(), Path::new(&reservation.path), RUN_ID);

	let first = crate::social_xurl::publish_with_test_binary(&request, &xurl)
		.expect("xurl publication should succeed");
	assert_eq!(first.status, "published");
	assert_eq!(first.verified_account, "decodexspace");
	assert_eq!(first.publication_recorded_cost_ceiling_microusd, 30_000);
	assert_eq!(first.monthly_reserved_cost_ceiling_microusd, 30_000);
	assert_eq!(first.monthly_budget_microusd, 1_250_000);
	assert_eq!(first.published_url, "https://x.com/decodexspace/status/2000000000000000001");

	let post = crate::load_json(Path::new(&first.post_path)).expect("published record");
	assert_eq!(post["publication"]["publisher"], "xurl");
	assert_eq!(post["publication"]["post_id"], "2000000000000000001");
	assert_eq!(post["publication"]["verified_account"], "decodexspace");
	assert_eq!(post["publication"]["recorded_cost_ceiling_microusd"], 30_000);
	assert!(post.get("browser_session").is_none());
	let attempt = crate::load_json(Path::new(&first.attempt_path)).expect("xurl attempt");
	assert_eq!(attempt["schema"], "decodex/xurl-publish-attempt/4");
	assert_eq!(attempt["idempotency_key"], TEST_IDEMPOTENCY_KEY);
	assert_eq!(
		attempt["authorization_contract_sha256"],
		crate::load_json_with_sha256(&request.authorization_contract_path)
			.expect("authorization contract digest")
			.1
	);

	let consumed =
		crate::load_json(Path::new(&first.reservation_path)).expect("consumed reservation");
	assert_eq!(consumed["status"], "consumed");
	assert_eq!(consumed["consumed_by_social_post"], first.post_path);
	assert_eq!(
		fs::read_to_string(&log_path).expect("xurl call log").lines().collect::<Vec<_>>(),
		["auth", "/2/users/me", "post", "read"]
	);

	let retry = crate::social_xurl::publish_with_test_binary(&request, &xurl)
		.expect("exact publication retry should read local evidence");
	assert_eq!(retry.status, "already_published");
	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert_eq!(log.lines().filter(|line| *line == "post").count(), 1);
	assert_eq!(log.lines().filter(|line| *line == "read").count(), 1);
}

#[cfg(unix)]
#[test]
fn publication_artifact_crash_recovers_with_a_later_cli_time_without_paid_calls() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation should succeed");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let publication = crate::social_xurl::publish_with_test_binary(
		&publish_request(temp.path(), Path::new(&reservation.path), RUN_ID),
		&xurl,
	)
	.expect("publication should succeed");
	let paid_call_log = fs::read_to_string(&log_path).expect("xurl call log");

	let reservation_path = Path::new(&publication.reservation_path);
	let consumed = crate::load_json(reservation_path).expect("consumed reservation");
	let mut active = consumed.clone();
	let active_object = active.as_object_mut().expect("reservation object");
	active_object.insert("status".into(), json!("active"));
	active_object.remove("consumed_by_social_post");
	crate::replace_existing_json(reservation_path, &consumed, &active)
		.expect("restore post-write reservation state");

	let attempt_path = Path::new(&publication.attempt_path);
	let published_attempt = crate::load_json(attempt_path).expect("published attempt");
	let mut verified_attempt = published_attempt.clone();
	verified_attempt["status"] = json!("verified");
	crate::replace_existing_json(attempt_path, &published_attempt, &verified_attempt)
		.expect("restore post-write attempt state");
	fs::remove_file(&publication.post_path).expect("restore verified-read-before-post-write state");

	let recovered = crate::reconcile_social_xurl(&reconcile_request(
		temp.path(),
		reservation_path,
		SECOND_RUN_ID,
		"2026-07-28T18:00:00Z",
	))
	.expect("a different task must recover the durable publication without X");
	assert_eq!(recovered.status, "reconciled");
	assert_eq!(recovered.kind, "publication");
	assert_eq!(recovered.operation_id, SECOND_RUN_ID);
	assert_eq!(recovered.original_run_id, RUN_ID);
	assert_eq!(recovered.paid_call_count, 0);
	assert_eq!(fs::read_to_string(&log_path).expect("recovery xurl call log"), paid_call_log);
	let recovered_reservation = crate::load_json(reservation_path).expect("recovered reservation");
	assert_eq!(recovered_reservation["status"], "consumed");
	let recovered_attempt = crate::load_json(attempt_path).expect("recovered attempt");
	assert_eq!(recovered_attempt["status"], "published");
	assert_eq!(recovered_attempt["run_id"], RUN_ID);
	assert_eq!(recovered_attempt["reconciliation"]["operation_id"], SECOND_RUN_ID);
	let post = crate::load_json(Path::new(&publication.post_path)).expect("published post");
	assert_eq!(post["publication"]["posted_at"], "2026-07-27T12:02:00Z");
}

#[cfg(unix)]
#[test]
fn xurl_outcome_reads_due_metrics_once_and_shares_the_budget() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation should succeed");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let publication = crate::social_xurl::publish_with_test_binary(
		&publish_request(temp.path(), Path::new(&reservation.path), RUN_ID),
		&xurl,
	)
	.expect("publication should succeed");
	let request = observe_request(temp.path(), Path::new(&publication.post_path), "24h");

	let first = crate::social_xurl::observe_with_test_binary(&request, &xurl)
		.expect("24-hour outcome should succeed");
	assert_eq!(first.status, "observed");
	assert_eq!(first.observation_recorded_cost_ceiling_microusd, 5_000);
	assert_eq!(first.monthly_reserved_cost_ceiling_microusd, 35_000);
	let outcome = crate::load_json(Path::new(&first.outcome_path)).expect("outcome record");
	assert_eq!(outcome["metrics"]["views"], 10);
	assert_eq!(outcome["metrics"]["likes"], 1);
	assert_eq!(outcome["observation"]["reader"], "xurl");

	let mut seven_day_request =
		observe_request(temp.path(), Path::new(&publication.post_path), "7d");
	seven_day_request.run_id = SECOND_RUN_ID.into();
	seven_day_request.observed_at = "2026-08-03T12:02:00Z".into();
	let seven_day = crate::social_xurl::observe_with_test_binary(&seven_day_request, &xurl)
		.expect("seven-day outcome should succeed");
	assert_eq!(seven_day.status, "observed");
	assert_eq!(seven_day.observation_recorded_cost_ceiling_microusd, 5_000);
	assert_eq!(seven_day.monthly_reserved_cost_ceiling_microusd, 5_000);

	let retry = crate::social_xurl::observe_with_test_binary(&request, &xurl)
		.expect("exact outcome retry should use local evidence");
	assert_eq!(retry.status, "already_observed");
	let seven_day_retry = crate::social_xurl::observe_with_test_binary(&seven_day_request, &xurl)
		.expect("exact seven-day retry should use local evidence");
	assert_eq!(seven_day_retry.status, "already_observed");

	let july = crate::social_xurl::cost_report_for_test(&temp.path().join("attempts"), "2026-07")
		.expect("July cost report");
	assert_eq!(july.used_cost_ceiling_microusd, 35_000);
	assert_eq!(july.reserved_cost_ceiling_microusd, 35_000);
	assert_eq!(july.publication_attempt_count, 1);
	assert_eq!(july.observation_attempt_count, 1);
	assert_eq!(july.total_call_count, 4);

	let august = crate::social_xurl::cost_report_for_test(&temp.path().join("attempts"), "2026-08")
		.expect("August cost report");
	assert_eq!(august.used_cost_ceiling_microusd, 5_000);
	assert_eq!(august.reserved_cost_ceiling_microusd, 5_000);
	assert_eq!(august.publication_attempt_count, 0);
	assert_eq!(august.observation_attempt_count, 1);
	assert_eq!(august.total_call_count, 1);
	assert_eq!(july.used_cost_ceiling_microusd + august.used_cost_ceiling_microusd, 40_000);

	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert_eq!(log.lines().filter(|line| *line == "read").count(), 3);
}

#[cfg(unix)]
#[test]
fn outcome_artifact_crash_recovers_with_a_later_cli_time_without_paid_calls() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation should succeed");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let publication = crate::social_xurl::publish_with_test_binary(
		&publish_request(temp.path(), Path::new(&reservation.path), RUN_ID),
		&xurl,
	)
	.expect("publication should succeed");
	let observation_request =
		observe_request(temp.path(), Path::new(&publication.post_path), "24h");
	let observation = crate::social_xurl::observe_with_test_binary(&observation_request, &xurl)
		.expect("observation should succeed");
	let paid_call_log = fs::read_to_string(&log_path).expect("xurl call log");

	let observation_attempt_path = crate::collect_json_files(&[temp
		.path()
		.join("attempts/2026-07")])
	.expect("attempt paths")
	.into_iter()
	.find(|path| {
		crate::load_json(path)
			.ok()
			.and_then(|attempt| attempt.get("schema").and_then(Value::as_str).map(str::to_owned))
			.as_deref()
			== Some("decodex/xurl-observation-attempt/4")
	})
	.expect("observation attempt");
	let observed_attempt = crate::load_json(&observation_attempt_path).expect("observed attempt");
	assert_eq!(
		observed_attempt["authorization_contract_sha256"],
		crate::load_json_with_sha256(&observation_request.authorization_contract_path)
			.expect("authorization contract digest")
			.1
	);
	let mut inflight_attempt = observed_attempt.clone();
	inflight_attempt["status"] = json!("read_inflight");
	inflight_attempt["call"]["status"] = json!("inflight");
	inflight_attempt["call"]["response_sha256"] = Value::Null;
	inflight_attempt["calls"][0]["status"] = json!("inflight");
	inflight_attempt["calls"][0]["response_sha256"] = Value::Null;
	crate::replace_existing_json(&observation_attempt_path, &observed_attempt, &inflight_attempt)
		.expect("restore outcome-write crash state");

	let recovered = crate::reconcile_social_xurl(&reconcile_request(
		temp.path(),
		Path::new(&observation.outcome_path),
		SECOND_RUN_ID,
		"2026-08-02T12:02:00Z",
	))
	.expect("a different task must recover the durable outcome without X");
	assert_eq!(recovered.status, "reconciled");
	assert_eq!(recovered.kind, "outcome");
	assert_eq!(recovered.operation_id, SECOND_RUN_ID);
	assert_eq!(recovered.original_run_id, RUN_ID);
	assert_eq!(recovered.paid_call_count, 0);
	assert_eq!(fs::read_to_string(&log_path).expect("recovery xurl call log"), paid_call_log);
	let recovered_attempt =
		crate::load_json(&observation_attempt_path).expect("recovered observation attempt");
	assert_eq!(recovered_attempt["status"], "observed");
	assert_eq!(recovered_attempt["calls"][0]["status"], "succeeded");
	assert_eq!(recovered_attempt["run_id"], RUN_ID);
	assert_eq!(recovered_attempt["reconciliation"]["operation_id"], SECOND_RUN_ID);
	let outcome = crate::load_json(Path::new(&observation.outcome_path)).expect("outcome record");
	assert_eq!(
		recovered_attempt["calls"][0]["response_sha256"],
		outcome["observation"]["response_sha256"]
	);
	assert_eq!(outcome["observed_at"], "2026-07-28T12:02:00Z");
}

#[cfg(unix)]
#[test]
fn reconciliation_rejects_unknown_post_identity_without_x() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let publication = crate::social_xurl::publish_with_test_binary(
		&publish_request(temp.path(), Path::new(&reservation.path), RUN_ID),
		&xurl,
	)
	.expect("publication");
	let paid_log = fs::read_to_string(&log_path).expect("paid log");
	let reservation_path = Path::new(&publication.reservation_path);
	let consumed = crate::load_json(reservation_path).expect("consumed reservation");
	let mut active = consumed.clone();
	active["status"] = json!("active");
	active.as_object_mut().expect("reservation object").remove("consumed_by_social_post");
	crate::replace_existing_json(reservation_path, &consumed, &active).expect("active reservation");
	let attempt_path = Path::new(&publication.attempt_path);
	let published_attempt = crate::load_json(attempt_path).expect("published attempt");
	let mut invalid_attempt = published_attempt.clone();
	invalid_attempt["status"] = json!("verified");
	invalid_attempt["post_id"] = Value::Null;
	crate::replace_existing_json(attempt_path, &published_attempt, &invalid_attempt)
		.expect("unknown post identity");

	let error = crate::reconcile_social_xurl(&reconcile_request(
		temp.path(),
		reservation_path,
		SECOND_RUN_ID,
		"2026-07-28T12:00:00Z",
	))
	.expect_err("unknown post identity must not reconcile")
	.to_string();
	assert!(error.contains("lacks its public post identity"), "{error}");
	assert_eq!(fs::read_to_string(&log_path).expect("unchanged paid log"), paid_log);
}

#[cfg(unix)]
#[test]
fn reconciliation_rejects_failed_paid_read_and_owner_mismatch_without_x() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let publication = crate::social_xurl::publish_with_test_binary(
		&publish_request(temp.path(), Path::new(&reservation.path), RUN_ID),
		&xurl,
	)
	.expect("publication");
	let publication_attempt_path = Path::new(&publication.attempt_path);
	let publication_attempt =
		crate::load_json(publication_attempt_path).expect("publication attempt");
	let mut wrong_owner_attempt = publication_attempt.clone();
	wrong_owner_attempt["run_id"] = json!(THIRD_RUN_ID);
	crate::replace_existing_json(
		publication_attempt_path,
		&publication_attempt,
		&wrong_owner_attempt,
	)
	.expect("owner mismatch");
	let paid_log = fs::read_to_string(&log_path).expect("publication log");
	let error = crate::reconcile_social_xurl(&reconcile_request(
		temp.path(),
		Path::new(&publication.reservation_path),
		SECOND_RUN_ID,
		"2026-07-28T12:00:00Z",
	))
	.expect_err("attempt owner mismatch must fail closed")
	.to_string();
	assert!(error.contains("does not match this publication"), "{error}");
	assert_eq!(fs::read_to_string(&log_path).expect("unchanged publication log"), paid_log);

	crate::replace_existing_json(
		publication_attempt_path,
		&wrong_owner_attempt,
		&publication_attempt,
	)
	.expect("restore publication attempt");
	let observation = crate::social_xurl::observe_with_test_binary(
		&observe_request(temp.path(), Path::new(&publication.post_path), "24h"),
		&xurl,
	)
	.expect("observation");
	let observation_attempt_path = crate::collect_json_files(&[temp
		.path()
		.join("attempts/2026-07")])
	.expect("attempt paths")
	.into_iter()
	.find(|path| {
		crate::load_json(path)
			.ok()
			.and_then(|attempt| attempt.get("schema").and_then(Value::as_str).map(str::to_owned))
			.as_deref()
			== Some("decodex/xurl-observation-attempt/4")
	})
	.expect("observation attempt");
	let observed_attempt =
		crate::load_json(&observation_attempt_path).expect("observation attempt");
	let mut failed_attempt = observed_attempt.clone();
	failed_attempt["status"] = json!("halted");
	failed_attempt["call"]["status"] = json!("failed");
	failed_attempt["call"]["response_sha256"] = Value::Null;
	failed_attempt["calls"][0]["status"] = json!("failed");
	failed_attempt["calls"][0]["response_sha256"] = Value::Null;
	crate::replace_existing_json(&observation_attempt_path, &observed_attempt, &failed_attempt)
		.expect("failed paid read");
	let paid_log = fs::read_to_string(&log_path).expect("all paid calls");
	let error = crate::reconcile_social_xurl(&reconcile_request(
		temp.path(),
		Path::new(&observation.outcome_path),
		SECOND_RUN_ID,
		"2026-08-02T12:02:00Z",
	))
	.expect_err("failed paid read must not be upgraded by reconciliation")
	.to_string();
	assert!(error.contains("no locally recoverable successful paid-read attempt"), "{error}");
	assert_eq!(fs::read_to_string(&log_path).expect("unchanged paid calls"), paid_log);
}

#[cfg(unix)]
#[test]
fn reconciliation_rejects_a_substituted_evidence_path() {
	use std::os::unix::fs::symlink;

	let temp = tempfile::tempdir().expect("temporary directory");
	let reservations = temp.path().join("reservations");
	fs::create_dir_all(&reservations).expect("reservations directory");
	let outside = temp.path().join("outside.json");
	crate::write_new_json(&outside, &valid_social_publish_reservation())
		.expect("outside reservation");
	let substituted = reservations.join("substituted.json");
	symlink(&outside, &substituted).expect("substituted evidence");

	let error = crate::reconcile_social_xurl(&reconcile_request(
		temp.path(),
		&substituted,
		SECOND_RUN_ID,
		"2026-07-28T12:00:00Z",
	))
	.expect_err("symlinked evidence must fail closed")
	.to_string();
	assert!(error.contains("invalid") || error.contains("symlink"), "{error}");
}

#[cfg(unix)]
#[test]
fn reconcile_xurl_requires_exactly_one_evidence_or_attempt_path() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let mut request = reconcile_request(
		temp.path(),
		&temp.path().join("evidence.json"),
		SECOND_RUN_ID,
		"2026-07-28T12:00:00Z",
	);
	request.attempt_path = Some(temp.path().join("attempt.json"));
	let error = crate::social_xurl::reconcile_with_test_binary(&request, &xurl)
		.expect_err("two reconciliation sources must fail")
		.to_string();
	assert!(error.contains("exactly one"), "{error}");

	request.evidence_path = PathBuf::new();
	request.attempt_path = None;
	let error = crate::social_xurl::reconcile_with_test_binary(&request, &xurl)
		.expect_err("missing reconciliation source must fail")
		.to_string();
	assert!(error.contains("exactly one"), "{error}");
	assert!(!log_path.exists());
}

#[cfg(unix)]
#[test]
fn reconcile_identity_inflight_ends_without_a_create_effect() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	let reservation_path = Path::new(&reservation.path);
	write_publish_attempt(
		temp.path(),
		&candidate,
		reservation_path,
		RUN_ID,
		SeedPublishAttempt {
			status: "identity_inflight",
			reserved_cost_ceiling_microusd: 30_000,
			calls: json!([xurl_call("identity_read", "inflight", 10_000)]),
			post_id: None,
			published_url: None,
		},
	);
	let attempt_path = publish_attempt_path(temp.path(), RUN_ID);
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let request = reconcile_attempt_request(
		temp.path(),
		&attempt_path,
		SECOND_RUN_ID,
		"2026-07-27T12:05:00Z",
	);

	let report =
		crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(&request, &xurl)
			.expect("identity read recovery");
	assert_eq!(report.status, "identity_recovered_no_create");
	assert_eq!(report.kind, "identity_read");
	assert_eq!(report.paid_call_count, 1);
	assert!(!temp.path().join(format!("posts/{RUN_ID}.json")).exists());
	let reservation = crate::load_json(reservation_path).expect("released reservation");
	assert_eq!(reservation["status"], "expired");
	let attempt = crate::load_json(&attempt_path).expect("reconciled attempt");
	assert_eq!(attempt["status"], "identity_reconciled");
	assert_eq!(attempt["calls"][0]["status"], "uncertain");
	assert_eq!(attempt["calls"][1]["operation_id"], SECOND_RUN_ID);
	let log = fs::read_to_string(&log_path).expect("xurl log");
	assert_eq!(log.lines().filter(|line| *line == "/2/users/me").count(), 1);
	assert!(!log.lines().any(|line| line == "post"));

	let repeated = crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(
		&request,
		&temp.path().join("missing-xurl"),
	)
	.expect("terminal identity recovery is idempotent without xurl");
	assert_eq!(repeated.status, "already_identity_recovered_no_create");
	assert_eq!(repeated.paid_call_count, 0);
}

#[cfg(unix)]
#[test]
fn second_identity_recovery_uses_the_immutable_original_time() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	let reservation_path = Path::new(&reservation.path);
	write_publish_attempt(
		temp.path(),
		&candidate,
		reservation_path,
		RUN_ID,
		SeedPublishAttempt {
			status: "identity_reconcile_halted",
			reserved_cost_ceiling_microusd: 40_000,
			calls: json!([
				xurl_call("identity_read", "failed", 10_000),
				xurl_recovery_call(
					"identity_read_reconcile",
					"failed",
					10_000,
					SECOND_RUN_ID,
					"2026-07"
				)
			]),
			post_id: None,
			published_url: None,
		},
	);
	let attempt_path = publish_attempt_path(temp.path(), RUN_ID);
	let mut attempt = crate::load_json(&attempt_path).expect("attempt");
	attempt["updated_at"] = json!("2026-07-27T13:30:00Z");
	let original = crate::load_json(&attempt_path).expect("original attempt");
	crate::replace_existing_json(&attempt_path, &original, &attempt)
		.expect("simulate failed recovery after reservation expiry");
	let xurl = temp.path().join("missing-xurl");
	let request =
		reconcile_attempt_request(temp.path(), &attempt_path, THIRD_RUN_ID, "2026-07-27T14:00:00Z");

	let error =
		crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(&request, &xurl)
			.expect_err("lineage budget must stop a second identity recovery")
			.to_string();
	assert!(error.contains("publication lineage budget exhausted"), "{error}");
	let attempt = crate::load_json(&attempt_path).expect("recovered attempt");
	assert_eq!(attempt["reserved_cost_ceiling_microusd"], 40_000);
	assert_eq!(attempt["calls"].as_array().expect("calls").len(), 2);
}

#[cfg(unix)]
#[test]
fn reconcile_never_retries_an_unknown_create_effect() {
	for (attempt_status, create_status) in
		[("create_inflight", "inflight"), ("create_uncertain", "uncertain")]
	{
		let temp = tempfile::tempdir().expect("temporary directory");
		let candidate = write_candidate(temp.path(), valid_social_candidate());
		let reservation =
			crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
				.expect("reservation");
		let reservation_path = Path::new(&reservation.path);
		write_publish_attempt(
			temp.path(),
			&candidate,
			reservation_path,
			RUN_ID,
			SeedPublishAttempt {
				status: attempt_status,
				reserved_cost_ceiling_microusd: 30_000,
				calls: json!([
					xurl_call("identity_read", "succeeded", 10_000),
					xurl_call("content_create", create_status, 15_000)
				]),
				post_id: None,
				published_url: None,
			},
		);
		let attempt_path = publish_attempt_path(temp.path(), RUN_ID);
		let xurl = temp.path().join("missing-xurl");
		let request = reconcile_attempt_request(
			temp.path(),
			&attempt_path,
			SECOND_RUN_ID,
			"2026-07-27T12:05:00Z",
		);

		let error =
			crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(&request, &xurl)
				.expect_err("unknown create effect must not be retried")
				.to_string();
		assert!(error.contains("automated create retry is forbidden"), "{error}");
	}
}

#[cfg(unix)]
#[test]
fn recovery_rejects_unapproved_persisted_xurl_version_before_any_xurl_call() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	let reservation_path = Path::new(&reservation.path);
	write_publish_attempt(
		temp.path(),
		&candidate,
		reservation_path,
		RUN_ID,
		SeedPublishAttempt {
			status: "created",
			reserved_cost_ceiling_microusd: 30_000,
			calls: json!([
				xurl_call("identity_read", "succeeded", 10_000),
				xurl_call("content_create", "succeeded", 15_000)
			]),
			post_id: Some("2000000000000000001"),
			published_url: None,
		},
	);
	let attempt_path = publish_attempt_path(temp.path(), RUN_ID);
	let original = crate::load_json(&attempt_path).expect("attempt");
	let mut unsupported = original.clone();
	unsupported["xurl_version"] = json!("1.3.2");
	crate::replace_existing_json(&attempt_path, &original, &unsupported)
		.expect("unsupported persisted version");
	let xurl = temp.path().join("missing-xurl");
	let request = reconcile_attempt_request(
		temp.path(),
		&attempt_path,
		SECOND_RUN_ID,
		"2026-07-27T12:05:00Z",
	);

	let error =
		crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(&request, &xurl)
			.expect_err("unsupported persisted xurl version must fail closed")
			.to_string();

	assert!(error.contains("usage authority is invalid"), "{error}");
}

#[cfg(unix)]
#[test]
fn recovery_rejects_mismatched_candidate_lineage_before_any_xurl_call() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	let reservation_path = Path::new(&reservation.path);
	write_publish_attempt(
		temp.path(),
		&candidate,
		reservation_path,
		RUN_ID,
		SeedPublishAttempt {
			status: "created",
			reserved_cost_ceiling_microusd: 30_000,
			calls: json!([
				xurl_call("identity_read", "succeeded", 10_000),
				xurl_call("content_create", "succeeded", 15_000)
			]),
			post_id: Some("2000000000000000001"),
			published_url: None,
		},
	);
	let attempt_path = publish_attempt_path(temp.path(), RUN_ID);
	let original = crate::load_json(&attempt_path).expect("attempt");
	let mut mismatched = original.clone();
	mismatched["candidate_sha256"] = json!("f".repeat(64));
	crate::replace_existing_json(&attempt_path, &original, &mismatched)
		.expect("mismatched candidate lineage");
	let reservation_before = fs::read(reservation_path).expect("reservation bytes");
	let xurl = temp.path().join("missing-xurl");
	let request = reconcile_attempt_request(
		temp.path(),
		&attempt_path,
		SECOND_RUN_ID,
		"2026-07-27T12:05:00Z",
	);

	let error =
		crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(&request, &xurl)
			.expect_err("mismatched candidate lineage must fail before xurl")
			.to_string();

	assert!(error.contains("does not match this publication"), "{error}");
	assert_eq!(
		fs::read(reservation_path).expect("reservation bytes after failure"),
		reservation_before
	);
}

#[cfg(unix)]
#[test]
fn recovery_rejects_invalid_or_terminal_reservation_before_loading_xurl() {
	for case in ["unexpected_field", "consumed_without_post"] {
		let temp = tempfile::tempdir().expect("temporary directory");
		let candidate = write_candidate(temp.path(), valid_social_candidate());
		let reservation =
			crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
				.expect("reservation");
		let reservation_path = Path::new(&reservation.path);
		write_publish_attempt(
			temp.path(),
			&candidate,
			reservation_path,
			RUN_ID,
			SeedPublishAttempt {
				status: "read_inflight",
				reserved_cost_ceiling_microusd: 30_000,
				calls: json!([
					xurl_call("identity_read", "succeeded", 10_000),
					xurl_call("content_create", "succeeded", 15_000),
					xurl_call("post_read_initial", "inflight", 5_000)
				]),
				post_id: Some("2000000000000000001"),
				published_url: None,
			},
		);
		let original = crate::load_json(reservation_path).expect("original reservation");
		let mut invalid = original.clone();
		let expected_error = match case {
			"unexpected_field" => {
				invalid
					.as_object_mut()
					.expect("reservation object")
					.insert("unexpected".into(), json!(true));
				"reservation failed validation"
			},
			"consumed_without_post" => {
				invalid["status"] = json!("consumed");
				invalid["consumed_by_social_post"] =
					json!(".agent/automations/decodex/cache/social/x/posts/missing.json");
				"active or expired publication reservation"
			},
			_ => unreachable!("bounded case"),
		};
		crate::replace_existing_json(reservation_path, &original, &invalid)
			.expect("invalid recovery reservation fixture");
		let reservation_before = fs::read(reservation_path).expect("reservation bytes");
		let attempt_path = publish_attempt_path(temp.path(), RUN_ID);
		let attempt_before = fs::read(&attempt_path).expect("attempt bytes");
		let request = reconcile_attempt_request(
			temp.path(),
			&attempt_path,
			SECOND_RUN_ID,
			"2026-07-27T12:05:00Z",
		);

		let error = crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(
			&request,
			&temp.path().join("missing-xurl"),
		)
		.expect_err("invalid reservation must fail before xurl")
		.to_string();

		assert!(error.contains(expected_error), "{case}: {error}");
		assert_eq!(
			fs::read(reservation_path).expect("reservation bytes after failure"),
			reservation_before
		);
		assert_eq!(fs::read(&attempt_path).expect("attempt bytes after failure"), attempt_before);
		assert!(!temp.path().join(format!("posts/{RUN_ID}.json")).exists());
	}
}

#[cfg(unix)]
#[test]
fn publish_retry_rejects_unapproved_persisted_xurl_version_before_any_xurl_call() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	let reservation_path = Path::new(&reservation.path);
	write_publish_attempt(
		temp.path(),
		&candidate,
		reservation_path,
		RUN_ID,
		SeedPublishAttempt {
			status: "created",
			reserved_cost_ceiling_microusd: 30_000,
			calls: json!([
				xurl_call("identity_read", "succeeded", 10_000),
				xurl_call("content_create", "succeeded", 15_000)
			]),
			post_id: Some("2000000000000000001"),
			published_url: None,
		},
	);
	let attempt_path = publish_attempt_path(temp.path(), RUN_ID);
	let original = crate::load_json(&attempt_path).expect("attempt");
	let mut unsupported = original.clone();
	unsupported["xurl_version"] = json!("1.3.2");
	crate::replace_existing_json(&attempt_path, &original, &unsupported)
		.expect("unsupported persisted version");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let request = publish_request(temp.path(), reservation_path, RUN_ID);
	let reservation_before = fs::read(reservation_path).expect("reservation bytes");

	let error = crate::social_xurl::publish_with_test_binary(&request, &xurl)
		.expect_err("publish retry with an unsupported persisted xurl version must fail closed")
		.to_string();

	assert!(error.contains("usage authority is invalid"), "{error}");
	assert!(!log_path.exists(), "no xurl command may run before attempt validation");
	assert_eq!(
		fs::read(reservation_path).expect("reservation bytes after failure"),
		reservation_before
	);
}

#[cfg(unix)]
#[test]
fn reconcile_known_post_id_reads_once_and_terminalizes_publication() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	let reservation_path = Path::new(&reservation.path);
	write_publish_attempt(
		temp.path(),
		&candidate,
		reservation_path,
		RUN_ID,
		SeedPublishAttempt {
			status: "read_inflight",
			reserved_cost_ceiling_microusd: 30_000,
			calls: json!([
				xurl_call("identity_read", "succeeded", 10_000),
				xurl_call("content_create", "succeeded", 15_000),
				xurl_call("post_read_initial", "inflight", 5_000)
			]),
			post_id: Some("2000000000000000001"),
			published_url: None,
		},
	);
	let attempt_path = publish_attempt_path(temp.path(), RUN_ID);
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let request = reconcile_attempt_request(
		temp.path(),
		&attempt_path,
		SECOND_RUN_ID,
		"2026-07-27T12:05:00Z",
	);

	let report =
		crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(&request, &xurl)
			.expect("known post readback");
	assert_eq!(report.status, "reconciled");
	assert_eq!(report.kind, "publication_read");
	assert_eq!(report.paid_call_count, 1);
	let post =
		crate::load_json(&temp.path().join(format!("posts/{RUN_ID}.json"))).expect("durable post");
	assert_eq!(post["publication"]["post_id"], "2000000000000000001");
	let attempt = crate::load_json(&attempt_path).expect("published attempt");
	assert_eq!(attempt["status"], "published");
	assert_eq!(attempt["reserved_cost_ceiling_microusd"], 35_000);
	let log = fs::read_to_string(&log_path).expect("xurl log");
	assert_eq!(log.lines().filter(|line| *line == "read").count(), 1);
	assert!(!log.lines().any(|line| line == "post"));

	let repeated = crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(
		&request,
		&temp.path().join("missing-xurl"),
	)
	.expect("terminal publication reconciliation is idempotent without xurl");
	assert_eq!(repeated.status, "already_terminal");
	assert_eq!(repeated.paid_call_count, 0);
}

#[cfg(unix)]
#[test]
fn created_recovery_reuses_the_original_read_reservation_without_duplicate_charge() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	let reservation_path = Path::new(&reservation.path);
	write_publish_attempt(
		temp.path(),
		&candidate,
		reservation_path,
		RUN_ID,
		SeedPublishAttempt {
			status: "created",
			reserved_cost_ceiling_microusd: 30_000,
			calls: json!([
				xurl_call("identity_read", "succeeded", 10_000),
				xurl_call("content_create", "succeeded", 15_000)
			]),
			post_id: Some("2000000000000000001"),
			published_url: None,
		},
	);
	let attempt_path = publish_attempt_path(temp.path(), RUN_ID);
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let request = reconcile_attempt_request(
		temp.path(),
		&attempt_path,
		SECOND_RUN_ID,
		"2026-07-27T12:05:00Z",
	);

	let report =
		crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(&request, &xurl)
			.expect("created publication readback");

	assert_eq!(report.status, "reconciled");
	assert_eq!(report.kind, "publication_read");
	assert_eq!(report.paid_call_count, 1);
	let attempt = crate::load_json(&attempt_path).expect("published attempt");
	assert_eq!(attempt["status"], "published");
	assert_eq!(attempt["reserved_cost_ceiling_microusd"], 30_000);
	assert_eq!(attempt["calls"].as_array().expect("calls").len(), 3);
	assert_eq!(attempt["calls"][2]["operation"], "post_read_initial_reconcile");
	assert_eq!(attempt["calls"][2]["operation_id"], SECOND_RUN_ID);
	assert_eq!(attempt["calls"][2]["billing_month"], Value::Null);
	assert_eq!(attempt["calls"][2]["status"], "succeeded");
	let costs = crate::social_xurl::cost_report_for_test(&temp.path().join("attempts"), "2026-07")
		.expect("July cost report");
	assert_eq!(costs.used_cost_ceiling_microusd, 30_000);
	assert_eq!(costs.reserved_cost_ceiling_microusd, 30_000);
	assert_eq!(costs.post_read_call_count, 1);
	assert_eq!(costs.total_call_count, 3);
	let log = fs::read_to_string(&log_path).expect("xurl log");
	assert_eq!(log.lines().filter(|line| *line == "read").count(), 1);
	assert!(!log.lines().any(|line| line == "post"));
	assert_eq!(
		crate::load_json(reservation_path).expect("consumed reservation")["status"],
		"consumed"
	);
	assert!(temp.path().join(format!("posts/{RUN_ID}.json")).exists());
}

#[cfg(unix)]
#[test]
fn reconcile_interrupted_outcome_read_is_bounded_and_owner_scoped() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let post_path = temp.path().join(format!("posts/{RUN_ID}.json"));
	crate::write_new_json(&post_path, &valid_social_post()).expect("published post");
	let attempt_path = write_observation_attempt(
		temp.path(),
		&post_path,
		"read_inflight",
		vec![xurl_call("outcome_read", "inflight", 5_000)],
		5_000,
	);
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let request = reconcile_attempt_request(
		temp.path(),
		&attempt_path,
		SECOND_RUN_ID,
		"2026-07-28T12:02:00Z",
	);

	let report =
		crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(&request, &xurl)
			.expect("interrupted outcome read");
	assert_eq!(report.kind, "outcome_read");
	assert_eq!(report.paid_call_count, 1);
	let attempt = crate::load_json(&attempt_path).expect("observation attempt");
	assert_eq!(attempt["status"], "observed");
	assert_eq!(attempt["calls"][0]["status"], "uncertain");
	assert_eq!(attempt["calls"][1]["operation_id"], SECOND_RUN_ID);
	assert_eq!(fs::read_to_string(&log_path).expect("xurl log").matches("read\n").count(), 1);

	let repeated = crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(
		&request,
		&temp.path().join("missing-xurl"),
	)
	.expect("terminal outcome reconciliation is idempotent without xurl");
	assert_eq!(repeated.status, "already_terminal");
	assert_eq!(repeated.paid_call_count, 0);

	let owner_error = seeded_outcome_recovery_error(&[SECOND_RUN_ID], SECOND_RUN_ID);
	assert!(owner_error.contains("reuses an owner"), "{owner_error}");
	let exhausted = seeded_outcome_recovery_error(&[SECOND_RUN_ID, THIRD_RUN_ID], FOURTH_RUN_ID);
	assert!(exhausted.contains("exhausted"), "{exhausted}");
}

#[cfg(unix)]
#[test]
fn outcome_recovery_rejects_time_regression_before_loading_xurl() {
	for (case, created_at, updated_at) in [
		("updated_after_reconciliation", "2026-07-28T12:02:00Z", "2026-07-28T12:04:00Z"),
		("created_after_update", "2026-07-28T12:05:00Z", "2026-07-28T12:02:00Z"),
	] {
		let temp = tempfile::tempdir().expect("temporary directory");
		let post_path = temp.path().join(format!("posts/{RUN_ID}.json"));
		crate::write_new_json(&post_path, &valid_social_post()).expect("published post");
		let attempt_path = write_observation_attempt(
			temp.path(),
			&post_path,
			"read_inflight",
			vec![xurl_call("outcome_read", "inflight", 5_000)],
			5_000,
		);
		let original = crate::load_json(&attempt_path).expect("observation attempt");
		let mut regressed = original.clone();
		regressed["created_at"] = json!(created_at);
		regressed["updated_at"] = json!(updated_at);
		crate::replace_existing_json(&attempt_path, &original, &regressed)
			.expect("regressed observation timestamp fixture");
		let attempt_before = fs::read(&attempt_path).expect("attempt bytes");
		let request = reconcile_attempt_request(
			temp.path(),
			&attempt_path,
			SECOND_RUN_ID,
			"2026-07-28T12:03:00Z",
		);

		let error = crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(
			&request,
			&temp.path().join("missing-xurl"),
		)
		.expect_err("time regression must fail before xurl")
		.to_string();

		assert!(
			error.contains("xurl observation recovery timestamps are not monotonic"),
			"{case}: {error}"
		);
		assert_eq!(
			fs::read(&attempt_path).expect("attempt bytes after failure"),
			attempt_before,
			"{case}"
		);
		assert!(!temp.path().join(format!("outcomes/{RUN_ID}.json")).exists());
	}
}

#[cfg(unix)]
#[test]
fn publication_recovery_rejects_internal_time_regression_before_loading_xurl() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	let reservation_path = Path::new(&reservation.path);
	write_publish_attempt(
		temp.path(),
		&candidate,
		reservation_path,
		RUN_ID,
		SeedPublishAttempt {
			status: "created",
			reserved_cost_ceiling_microusd: 30_000,
			calls: json!([
				xurl_call("identity_read", "succeeded", 10_000),
				xurl_call("content_create", "succeeded", 15_000)
			]),
			post_id: Some("2000000000000000001"),
			published_url: None,
		},
	);
	let attempt_path = publish_attempt_path(temp.path(), RUN_ID);
	let original = crate::load_json(&attempt_path).expect("publication attempt");
	let mut regressed = original.clone();
	regressed["created_at"] = json!("2026-07-27T12:05:00Z");
	regressed["updated_at"] = json!("2026-07-27T12:02:00Z");
	crate::replace_existing_json(&attempt_path, &original, &regressed)
		.expect("regressed publication timestamp fixture");
	let attempt_before = fs::read(&attempt_path).expect("attempt bytes");
	let reservation_before = fs::read(reservation_path).expect("reservation bytes");
	let request = reconcile_attempt_request(
		temp.path(),
		&attempt_path,
		SECOND_RUN_ID,
		"2026-07-27T12:03:00Z",
	);

	let error = crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(
		&request,
		&temp.path().join("missing-xurl"),
	)
	.expect_err("time regression must fail before xurl")
	.to_string();

	assert!(error.contains("xurl publication recovery timestamps are not monotonic"), "{error}");
	assert_eq!(fs::read(&attempt_path).expect("attempt bytes after failure"), attempt_before);
	assert_eq!(
		fs::read(reservation_path).expect("reservation bytes after failure"),
		reservation_before
	);
	assert!(!temp.path().join(format!("posts/{RUN_ID}.json")).exists());
}

#[cfg(unix)]
#[test]
fn outcome_recovery_budget_exhaustion_stops_before_loading_xurl() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let post_path = temp.path().join(format!("posts/{RUN_ID}.json"));
	crate::write_new_json(&post_path, &valid_social_post()).expect("published post");
	let attempt_path = write_observation_attempt(
		temp.path(),
		&post_path,
		"read_inflight",
		vec![xurl_call("outcome_read", "inflight", 5_000)],
		5_000,
	);
	for index in 0..40 {
		write_budget_publication_attempt(temp.path(), index);
	}
	for index in 0..9 {
		write_budget_observation_attempt(temp.path(), index);
	}
	let attempt_before = fs::read(&attempt_path).expect("attempt bytes");
	let request = reconcile_attempt_request(
		temp.path(),
		&attempt_path,
		SECOND_RUN_ID,
		"2026-07-28T12:03:00Z",
	);

	let error = crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(
		&request,
		&temp.path().join("missing-xurl"),
	)
	.expect_err("full ledger must fail before xurl")
	.to_string();

	assert!(error.contains("monthly X budget exhausted"), "{error}");
	assert_eq!(fs::read(&attempt_path).expect("attempt bytes after failure"), attempt_before);
	assert!(!temp.path().join(format!("outcomes/{RUN_ID}.json")).exists());
}

#[cfg(unix)]
#[test]
fn publication_recovery_budget_exhaustion_stops_before_loading_xurl() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	let reservation_path = Path::new(&reservation.path);
	write_publish_attempt(
		temp.path(),
		&candidate,
		reservation_path,
		RUN_ID,
		SeedPublishAttempt {
			status: "read_inflight",
			reserved_cost_ceiling_microusd: 30_000,
			calls: json!([
				xurl_call("identity_read", "succeeded", 10_000),
				xurl_call("content_create", "succeeded", 15_000),
				xurl_call("post_read_initial", "inflight", 5_000)
			]),
			post_id: Some("2000000000000000001"),
			published_url: None,
		},
	);
	for index in 0..40 {
		write_budget_publication_attempt(temp.path(), index);
	}
	for index in 0..4 {
		write_budget_observation_attempt(temp.path(), index);
	}
	let attempt_path = publish_attempt_path(temp.path(), RUN_ID);
	let before = fs::read(&attempt_path).expect("attempt bytes");
	let xurl = temp.path().join("missing-xurl");
	let request = reconcile_attempt_request(
		temp.path(),
		&attempt_path,
		SECOND_RUN_ID,
		"2026-07-27T12:05:00Z",
	);

	let error =
		crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(&request, &xurl)
			.expect_err("a full ledger must block the recovery read")
			.to_string();
	assert!(error.contains("monthly X budget exhausted"), "{error}");
	assert_eq!(fs::read(&attempt_path).expect("attempt bytes after failure"), before);
}

#[cfg(unix)]
#[test]
fn created_recovery_validates_reused_budget_before_loading_xurl() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	let reservation_path = Path::new(&reservation.path);
	write_publish_attempt(
		temp.path(),
		&candidate,
		reservation_path,
		RUN_ID,
		SeedPublishAttempt {
			status: "created",
			reserved_cost_ceiling_microusd: 30_000,
			calls: json!([
				xurl_call("identity_read", "succeeded", 10_000),
				xurl_call("content_create", "succeeded", 15_000)
			]),
			post_id: Some("2000000000000000001"),
			published_url: None,
		},
	);
	for index in 0..40 {
		write_budget_publication_attempt(temp.path(), index);
	}
	for index in 0..5 {
		write_budget_observation_attempt(temp.path(), index);
	}
	let attempt_path = publish_attempt_path(temp.path(), RUN_ID);
	let attempt_before = fs::read(&attempt_path).expect("attempt bytes");
	let reservation_before = fs::read(reservation_path).expect("reservation bytes");
	let request = reconcile_attempt_request(
		temp.path(),
		&attempt_path,
		SECOND_RUN_ID,
		"2026-07-27T12:05:00Z",
	);

	let error = crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(
		&request,
		&temp.path().join("missing-xurl"),
	)
	.expect_err("over-limit reused budget must fail before xurl")
	.to_string();

	assert!(error.contains("monthly X budget ledger exceeds its hard cap"), "{error}");
	assert_eq!(fs::read(&attempt_path).expect("attempt bytes after failure"), attempt_before);
	assert_eq!(
		fs::read(reservation_path).expect("reservation bytes after failure"),
		reservation_before
	);
	assert!(!temp.path().join(format!("posts/{RUN_ID}.json")).exists());
}

#[cfg(unix)]
#[test]
fn probe_xurl_is_nonbillable_bounded_and_clears_hostile_environment() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let log_path = temp.path().join("probe.log");
	let xurl = fake_probe_xurl(temp.path(), &log_path, None);
	let auth_contract = write_auth_contract(temp.path());
	let previous = std::env::var_os("XURL_HOSTILE_TEST");
	unsafe {
		std::env::set_var("XURL_HOSTILE_TEST", "https://attacker.invalid");
	}
	let result =
		crate::social_xurl::probe_with_test_binary("2026-07-28T12:00:00Z", &xurl, &auth_contract);
	unsafe {
		if let Some(value) = previous {
			std::env::set_var("XURL_HOSTILE_TEST", value);
		} else {
			std::env::remove_var("XURL_HOSTILE_TEST");
		}
	}
	let report = result.expect("nonbillable probe");
	assert!(report.ready);
	assert_eq!(report.status, "ready");
	assert_eq!(report.xurl_version, "1.3.1");
	assert_eq!(report.xurl_app, "default");
	assert_eq!(report.account_label, "decodexspace");
	assert_eq!(report.authorization_contract.status, "current");
	assert_eq!(report.authorization_contract.target_account, "decodexspace");
	assert_eq!(
		report.authorization_contract.required_operator_authorized_scopes,
		["tweet.read", "users.read", "tweet.write", "offline.access"]
	);
	assert_eq!(report.authorization_contract.xurl_version, "1.3.1");
	assert_eq!(
		report.authorization_contract.xurl_binary_sha256,
		"7b85a210009db7a3f2d6183684674441fbf81276f1101f73d36d0266ec9aa01e"
	);
	assert_eq!(
		report.pricing_policy.official_source,
		"https://docs.x.com/x-api/getting-started/pricing.md"
	);
	assert_eq!(report.pricing_policy.user_read_cost_microusd, 10_000);
	assert_eq!(report.pricing_policy.url_free_content_create_cost_microusd, 15_000);
	assert_eq!(report.pricing_policy.post_read_cost_ceiling_microusd, 5_000);
	assert_eq!(report.pricing_policy.monthly_reservation_cap_microusd, 1_250_000);
	assert_eq!(fs::read_to_string(&log_path).expect("probe log"), "version\nauth status\n");
	let serialized = serde_json::to_string(&report).expect("bounded report");
	assert!(!serialized.contains("client_id"));
	assert!(!serialized.contains("authorization_request_sha256"));
	assert!(
		!serialized.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
	);
	assert!(!serialized.contains("attacker.invalid"));
	assert!(!serialized.contains(temp.path().to_string_lossy().as_ref()));

	let expired =
		crate::social_xurl::probe_with_test_binary("2026-08-10T00:00:00Z", &xurl, &auth_contract)
			.expect("expired policy probe remains nonbillable");
	assert!(!expired.ready);
	assert_eq!(expired.status, "blocked");
	assert_eq!(expired.pricing_policy.status, "stale");
	assert_eq!(
		fs::read_to_string(&log_path).expect("two probes"),
		"version\nauth status\nversion\nauth status\n"
	);
}

#[cfg(unix)]
#[test]
fn probe_xurl_executes_the_bound_binary_after_path_replacement() {
	use std::os::unix::fs::symlink;

	let temp = tempfile::tempdir().expect("temporary directory");
	let runtime = temp.path().join("runtime");
	let retained = temp.path().join("retained");
	let attacker = temp.path().join("attacker");
	fs::create_dir(&runtime).expect("runtime");
	fs::create_dir(&attacker).expect("attacker");
	let log_path = temp.path().join("probe.log");
	let malicious_marker = temp.path().join("malicious");
	let trusted = fake_probe_xurl(&runtime, &log_path, None);
	fake_probe_xurl(&attacker, &log_path, Some(&malicious_marker));
	let auth_contract = write_auth_contract(temp.path());

	let report = crate::social_xurl::probe_with_test_binary_after_bind(
		"2026-07-28T12:00:00Z",
		&trusted,
		&auth_contract,
		|| {
			fs::rename(&runtime, &retained).expect("move trusted runtime");
			symlink(&attacker, &runtime).expect("replace runtime path");
		},
	)
	.expect("bound probe");
	assert!(report.ready);
	assert!(!malicious_marker.exists());
	assert_eq!(fs::read_to_string(log_path).expect("trusted probe log"), "version\nauth status\n");
}

#[cfg(unix)]
#[test]
fn authorization_contract_rejects_missing_mismatched_future_and_sensitive_receipts() {
	assert_probe_auth_rejected(
		None,
		"2026-07-28T12:00:00Z",
		"authorization contract is unavailable",
		"",
	);

	let mut extra_scope = valid_auth_contract();
	extra_scope["required_operator_authorized_scopes"]
		.as_array_mut()
		.expect("required scopes")
		.push(json!("dm.read"));
	assert_probe_auth_rejected(
		Some(extra_scope),
		"2026-07-28T12:00:00Z",
		"does not match the approved fixed authority",
		"",
	);

	for (field, value) in [("xurl_app", "other"), ("target_account", "other")] {
		let mut mismatched = valid_auth_contract();
		mismatched[field] = json!(value);
		assert_probe_auth_rejected(
			Some(mismatched),
			"2026-07-28T12:00:00Z",
			"does not match the approved fixed authority",
			"",
		);
	}

	let mut future = valid_auth_contract();
	future["sealed_at"] = json!("2026-07-29T12:00:00Z");
	assert_probe_auth_rejected(
		Some(future),
		"2026-07-28T12:00:00Z",
		"does not match the approved fixed authority",
		"",
	);

	let mut token_bearing = valid_auth_contract();
	token_bearing["access_token"] = json!("secret-token-must-not-appear");
	let error = assert_probe_auth_rejected(
		Some(token_bearing),
		"2026-07-28T12:00:00Z",
		"authorization contract is invalid",
		"",
	);
	assert!(!error.contains("secret-token-must-not-appear"));

	let mut oversized = valid_auth_contract();
	oversized["sealed_at"] = json!("x".repeat(17 * 1024));
	assert_probe_auth_rejected(
		Some(oversized),
		"2026-07-28T12:00:00Z",
		"exceeds its bounded read limit",
		"",
	);

	let mut wrong_version = valid_auth_contract();
	wrong_version["xurl_version"] = json!("1.3.2");
	assert_probe_auth_rejected(
		Some(wrong_version),
		"2026-07-28T12:00:00Z",
		"does not match the approved fixed authority",
		"",
	);
}

#[cfg(unix)]
#[test]
fn seal_xurl_auth_persists_only_the_fixed_nonsecret_contract() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let xurl = fake_probe_xurl(temp.path(), &temp.path().join("unused.log"), None);
	let receipt_path = temp.path().join("xurl-authorization-contract.json");
	let report = crate::social_xurl::seal_auth_with_test_binary(
		&SocialSealXurlAuthRequest {
			receipt_path: receipt_path.clone(),
			sealed_at: "2026-07-27T12:00:00Z".into(),
		},
		&xurl,
	)
	.expect("fixed authorization contract should seal");

	assert_eq!(report.status, "sealed");
	assert_eq!(report.xurl_version, "1.3.1");
	assert_eq!(
		report.xurl_binary_sha256,
		"7b85a210009db7a3f2d6183684674441fbf81276f1101f73d36d0266ec9aa01e"
	);
	assert_eq!(
		report.required_operator_authorized_scopes,
		["tweet.read", "users.read", "tweet.write", "offline.access"]
	);
	let receipt = fs::read_to_string(receipt_path).expect("sealed receipt");
	let serialized_report = serde_json::to_string(&report).expect("seal report");
	for secret in ["client_id", "client_secret", "access_token", "refresh_token", "auth.yml"] {
		assert!(!receipt.contains(secret));
		assert!(!serialized_report.contains(secret));
	}
}

#[cfg(unix)]
#[test]
fn publish_and_outcome_read_require_current_authorization_contract_before_xurl() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation");
	let publish = publish_request(temp.path(), Path::new(&reservation.path), RUN_ID);
	fs::remove_file(&publish.authorization_contract_path).expect("remove contract");
	let publish_log = temp.path().join("publish.log");
	let xurl = fake_xurl(temp.path(), &publish_log, "decodexspace");
	let error = crate::social_xurl::publish_with_test_binary(&publish, &xurl)
		.expect_err("publish must require authorization contract")
		.to_string();
	assert!(error.contains("authorization contract is unavailable"), "{error}");
	assert!(!publish_log.exists());

	let outcome_temp = tempfile::tempdir().expect("temporary directory");
	let post_path = outcome_temp.path().join("posts/post.json");
	crate::write_new_json(&post_path, &valid_social_post()).expect("post");
	let observe = observe_request(outcome_temp.path(), &post_path, "24h");
	let receipt = crate::load_json(&observe.authorization_contract_path).expect("contract");
	let mut mismatched = receipt.clone();
	mismatched["target_account"] = json!("different-user");
	crate::replace_existing_json(&observe.authorization_contract_path, &receipt, &mismatched)
		.expect("mismatched contract");
	let observe_log = outcome_temp.path().join("observe.log");
	let xurl = fake_xurl(outcome_temp.path(), &observe_log, "decodexspace");
	let error = crate::social_xurl::observe_with_test_binary(&observe, &xurl)
		.expect_err("outcome read must require matching authorization contract")
		.to_string();
	assert!(error.contains("does not match the approved fixed authority"), "{error}");
	assert!(!observe_log.exists());
}

#[cfg(unix)]
#[test]
fn stale_pricing_policy_stops_before_any_xurl_command() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let mut reserve = reserve_request(temp.path(), &candidate_path, RUN_ID);
	reserve.reserved_at = "2026-08-10T12:00:00Z".into();
	reserve.expires_at = "2026-08-10T13:00:00Z".into();
	reserve.day = "2026-08-10".into();
	let reservation = crate::reserve_social_publish(&reserve).expect("reservation");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let mut publish = publish_request(temp.path(), Path::new(&reservation.path), RUN_ID);
	publish.posted_at = "2026-08-10T12:02:00Z".into();

	let error = crate::social_xurl::publish_with_test_binary_and_stale_pricing(&publish, &xurl)
		.expect_err("stale pricing policy must block")
		.to_string();
	assert!(error.contains("pricing policy is not current: stale"), "{error}");
	assert!(!log_path.exists());
}

#[cfg(unix)]
#[test]
fn xurl_outcome_rejects_an_early_window_before_api_read() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let post_path = temp.path().join("posts/post.json");
	crate::write_new_json(&post_path, &valid_social_post()).expect("post should be written");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let mut request = observe_request(temp.path(), &post_path, "24h");
	request.observed_at = "2026-07-27T13:02:00Z".into();
	let error = crate::social_xurl::observe_with_test_binary(&request, &xurl)
		.expect_err("early observation must fail")
		.to_string();
	assert!(error.contains("outside its allowed window"));
	assert!(!log_path.exists());
}

#[cfg(unix)]
#[test]
fn publication_readback_has_at_most_one_budgeted_retry() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation should succeed");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl_config(
		temp.path(),
		&log_path,
		"decodexspace",
		"decodexspace",
		true,
		FakeReadMode::FailFirst,
	);
	let request = publish_request(temp.path(), Path::new(&reservation.path), RUN_ID);

	let report = crate::social_xurl::publish_with_test_binary(&request, &xurl)
		.expect("one known-id retry should recover");
	assert_eq!(report.publication_recorded_cost_ceiling_microusd, 35_000);
	assert_eq!(report.monthly_reserved_cost_ceiling_microusd, 35_000);
	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert_eq!(log.lines().filter(|line| *line == "read").count(), 2);
}

#[cfg(unix)]
#[test]
fn publication_recovery_consumes_lineage_budget_and_prevents_the_later_paid_read() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl_config(
		temp.path(),
		&log_path,
		"decodexspace",
		"decodexspace",
		true,
		FakeReadMode::FailFirst,
	);
	let publication = crate::social_xurl::publish_with_test_binary(
		&publish_request(temp.path(), Path::new(&reservation.path), RUN_ID),
		&xurl,
	)
	.expect("publication recovery");
	assert_eq!(publication.publication_recorded_cost_ceiling_microusd, 35_000);

	let first = observe_request(temp.path(), Path::new(&publication.post_path), "24h");
	crate::social_xurl::observe_with_test_binary(&first, &xurl).expect("remaining 5,000 budget");
	let mut later = observe_request(temp.path(), Path::new(&publication.post_path), "7d");
	later.run_id = SECOND_RUN_ID.into();
	later.observed_at = "2026-08-03T12:02:00Z".into();
	let error = crate::social_xurl::observe_with_test_binary(&later, &xurl)
		.expect_err("the recovery must consume the 7d read budget")
		.to_string();
	assert!(error.contains("publication lineage budget exhausted"), "{error}");
	let log = fs::read_to_string(log_path).expect("xurl log");
	assert_eq!(log.lines().filter(|line| *line == "read").count(), 3);
}

#[cfg(unix)]
#[test]
fn initial_read_crash_consumes_only_the_reserved_retry() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation should succeed");
	let reservation_path = Path::new(&reservation.path);
	write_publish_attempt(
		temp.path(),
		&candidate_path,
		reservation_path,
		RUN_ID,
		SeedPublishAttempt {
			status: "read_inflight",
			reserved_cost_ceiling_microusd: 30_000,
			calls: json!([
				xurl_call("identity_read", "succeeded", 10_000),
				xurl_call("content_create", "succeeded", 15_000),
				xurl_call("post_read_initial", "inflight", 5_000)
			]),
			post_id: Some("2000000000000000001"),
			published_url: None,
		},
	);
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");

	let report = crate::social_xurl::publish_with_test_binary(
		&publish_request(temp.path(), reservation_path, RUN_ID),
		&xurl,
	)
	.expect("known post id permits the one reserved read retry");
	assert_eq!(report.publication_recorded_cost_ceiling_microusd, 35_000);
	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert_eq!(log.lines().filter(|line| *line == "read").count(), 1);
	assert_eq!(log.lines().filter(|line| *line == "post").count(), 0);
}

#[cfg(unix)]
#[test]
fn read_retry_inflight_crash_state_forbids_another_paid_read() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation should succeed");
	let reservation_path = Path::new(&reservation.path);
	write_publish_attempt(
		temp.path(),
		&candidate_path,
		reservation_path,
		RUN_ID,
		SeedPublishAttempt {
			status: "read_retry_inflight",
			reserved_cost_ceiling_microusd: 35_000,
			calls: json!([
				xurl_call("identity_read", "succeeded", 10_000),
				xurl_call("content_create", "succeeded", 15_000),
				xurl_call("post_read_initial", "failed", 5_000),
				xurl_call("post_read_retry", "inflight", 5_000)
			]),
			post_id: Some("2000000000000000001"),
			published_url: None,
		},
	);
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");

	let error = crate::social_xurl::publish_with_test_binary(
		&publish_request(temp.path(), reservation_path, RUN_ID),
		&xurl,
	)
	.expect_err("an inflight retry has an unknown paid-read outcome")
	.to_string();
	assert!(error.contains("another paid retry is forbidden"));
	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert_eq!(log.lines().filter(|line| *line == "read").count(), 0);
	assert_eq!(log.lines().filter(|line| *line == "post").count(), 0);
}

#[cfg(unix)]
#[test]
fn outcome_read_failure_is_not_retried_on_a_later_run() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let post_path = temp.path().join("posts/post.json");
	crate::write_new_json(&post_path, &valid_social_post()).expect("post should be written");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl_config(
		temp.path(),
		&log_path,
		"decodexspace",
		"decodexspace",
		true,
		FakeReadMode::FailAlways,
	);
	let request = observe_request(temp.path(), &post_path, "24h");

	let _ = crate::social_xurl::observe_with_test_binary(&request, &xurl)
		.expect_err("first outcome read should fail");
	let mut later_task = observe_request(temp.path(), &post_path, "24h");
	later_task.run_id = SECOND_RUN_ID.into();
	let retry = crate::social_xurl::observe_with_test_binary(&later_task, &xurl)
		.expect_err("outcome read must not be retried")
		.to_string();
	assert!(retry.contains("another paid retry is forbidden"), "{retry}");
	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert_eq!(log.lines().filter(|line| *line == "read").count(), 1);
}

#[cfg(unix)]
#[test]
fn xurl_publish_rejects_wrong_account_before_public_write() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation should succeed");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "hackink");
	let request = publish_request(temp.path(), Path::new(&reservation.path), RUN_ID);

	let error = crate::social_xurl::publish_with_test_binary(&request, &xurl)
		.expect_err("wrong OAuth2 account must fail before create")
		.to_string();
	assert!(error.contains("does not have exactly one OAuth2 token labeled decodexspace"));
	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert!(!log.lines().any(|line| line == "post"));
}

#[cfg(unix)]
#[test]
fn xurl_publish_rejects_wrong_paid_identity_before_public_write() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation should succeed");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl_with_identity(temp.path(), &log_path, "decodexspace", "hackink", true);
	let request = publish_request(temp.path(), Path::new(&reservation.path), RUN_ID);

	let error = crate::social_xurl::publish_with_test_binary(&request, &xurl)
		.expect_err("paid identity mismatch must stop before create")
		.to_string();
	assert!(error.contains("identity read did not verify @decodexspace"));
	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert_eq!(log.lines().filter(|line| *line == "/2/users/me").count(), 1);
	assert!(!log.lines().any(|line| line == "post"));
}

#[cfg(unix)]
#[test]
fn invalid_successful_create_output_is_never_retried() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation should succeed");
	let log_path = temp.path().join("xurl.log");
	let xurl =
		fake_xurl_with_identity(temp.path(), &log_path, "decodexspace", "decodexspace", false);
	let request = publish_request(temp.path(), Path::new(&reservation.path), RUN_ID);

	let _ = crate::social_xurl::publish_with_test_binary(&request, &xurl)
		.expect_err("invalid successful create output must fail closed");
	let retry = crate::social_xurl::publish_with_test_binary(&request, &xurl)
		.expect_err("uncertain create must not be retried")
		.to_string();
	assert!(retry.contains("create outcome is unknown"), "{retry}");
	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert_eq!(log.lines().filter(|line| *line == "post").count(), 1);
}

#[cfg(unix)]
#[test]
fn uncertain_create_blocks_a_new_task_after_reservation_expiry() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation should succeed");
	let log_path = temp.path().join("xurl.log");
	let xurl =
		fake_xurl_with_identity(temp.path(), &log_path, "decodexspace", "decodexspace", false);
	let _ = crate::social_xurl::publish_with_test_binary(
		&publish_request(temp.path(), Path::new(&reservation.path), RUN_ID),
		&xurl,
	)
	.expect_err("invalid create evidence must become uncertain");

	let mut later = reserve_request(temp.path(), &candidate_path, SECOND_RUN_ID);
	later.reserved_at = "2026-07-27T14:00:00Z".into();
	later.expires_at = "2026-07-27T15:00:00Z".into();
	let error = crate::reserve_social_publish(&later)
		.expect_err("an expired reservation must not erase an uncertain public effect")
		.to_string();
	assert!(error.contains("prior uncertain or verified public-write attempt"));
	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert_eq!(log.lines().filter(|line| *line == "post").count(), 1);
}

#[cfg(unix)]
#[test]
fn uncertain_create_consumes_the_account_daily_slot_across_lineages() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation should succeed");
	let log_path = temp.path().join("xurl.log");
	let xurl =
		fake_xurl_with_identity(temp.path(), &log_path, "decodexspace", "decodexspace", false);
	let _ = crate::social_xurl::publish_with_test_binary(
		&publish_request(temp.path(), Path::new(&reservation.path), RUN_ID),
		&xurl,
	)
	.expect_err("invalid create evidence must become uncertain");

	let mut other = valid_social_candidate();
	other["slug"] = json!("openai-codex-pr-22415");
	let other = write_candidate_named(temp.path(), "other-lineage.json", other);
	let mut later = reserve_request(temp.path(), &other, SECOND_RUN_ID);
	later.reserved_at = "2026-07-27T14:00:00Z".into();
	later.expires_at = "2026-07-27T15:00:00Z".into();
	let error = crate::reserve_social_publish(&later)
		.expect_err("an uncertain create must consume the account-wide daily slot")
		.to_string();

	assert!(error.contains("daily public-write cap is already consumed"), "{error}");
	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert_eq!(log.lines().filter(|line| *line == "post").count(), 1);
}

#[cfg(unix)]
#[test]
fn uncertain_create_rejects_the_same_radar_subject_with_another_supplied_key() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate, RUN_ID))
			.expect("reservation should succeed");
	let log_path = temp.path().join("xurl.log");
	let xurl =
		fake_xurl_with_identity(temp.path(), &log_path, "decodexspace", "decodexspace", false);
	let _ = crate::social_xurl::publish_with_test_binary(
		&publish_request(temp.path(), Path::new(&reservation.path), RUN_ID),
		&xurl,
	)
	.expect_err("invalid create evidence must become uncertain");

	let mut same_subject = crate::load_json(&candidate).expect("candidate");
	same_subject["decision"]["idempotency_key"] =
		json!("radar-publication:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
	let other_path = temp.path().join("candidates/same-subject-other-key.json");
	crate::write_new_json(&other_path, &same_subject).expect("same-subject candidate");
	let mut later = reserve_request(temp.path(), &other_path, SECOND_RUN_ID);
	later.reserved_at = "2026-07-27T14:00:00Z".into();
	later.expires_at = "2026-07-27T15:00:00Z".into();

	let error = crate::reserve_social_publish(&later)
		.expect_err("a supplied key cannot change immutable Radar publication identity")
		.to_string();
	assert!(
		error.contains("idempotency_key must be derived")
			|| error.contains("prior uncertain or verified public-write attempt"),
		"{error}"
	);
	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert_eq!(log.lines().filter(|line| *line == "post").count(), 1);
}

#[cfg(unix)]
#[test]
fn stable_idempotency_key_blocks_a_different_candidate_path_on_the_next_day() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation should succeed");
	let log_path = temp.path().join("xurl.log");
	let xurl =
		fake_xurl_with_identity(temp.path(), &log_path, "decodexspace", "decodexspace", false);
	let _ = crate::social_xurl::publish_with_test_binary(
		&publish_request(temp.path(), Path::new(&reservation.path), RUN_ID),
		&xurl,
	)
	.expect_err("invalid create evidence must become uncertain");

	let next_day_candidate =
		write_candidate_named(temp.path(), "next-day-candidate.json", valid_social_candidate());
	let mut next_day = reserve_request(temp.path(), &next_day_candidate, SECOND_RUN_ID);
	next_day.reserved_at = "2026-07-28T12:00:00Z".into();
	next_day.expires_at = "2026-07-28T13:00:00Z".into();
	next_day.day = "2026-07-28".into();
	let error = crate::reserve_social_publish(&next_day)
		.expect_err("stable idempotency must survive candidate paths and UTC days")
		.to_string();
	assert!(error.contains("prior uncertain or verified public-write attempt"));
	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert_eq!(log.lines().filter(|line| *line == "post").count(), 1);
}

#[cfg(unix)]
#[test]
fn halted_readback_blocks_a_new_task_from_recreating_the_post() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation should succeed");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl_config(
		temp.path(),
		&log_path,
		"decodexspace",
		"decodexspace",
		true,
		FakeReadMode::FailAlways,
	);
	let _ = crate::social_xurl::publish_with_test_binary(
		&publish_request(temp.path(), Path::new(&reservation.path), RUN_ID),
		&xurl,
	)
	.expect_err("both readback attempts should fail");

	let mut later = reserve_request(temp.path(), &candidate_path, SECOND_RUN_ID);
	later.reserved_at = "2026-07-27T14:00:00Z".into();
	later.expires_at = "2026-07-27T15:00:00Z".into();
	let error = crate::reserve_social_publish(&later)
		.expect_err("a known created post must prevent recreation")
		.to_string();
	assert!(error.contains("prior uncertain or verified public-write attempt"));
	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert_eq!(log.lines().filter(|line| *line == "post").count(), 1);
	assert_eq!(log.lines().filter(|line| *line == "read").count(), 2);
}

#[cfg(unix)]
#[test]
fn durable_create_inflight_crash_state_forbids_create_retry() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation should succeed");
	let reservation_path = Path::new(&reservation.path);
	let root = crate::repo_root().expect("repo root");
	let attempt_path = temp.path().join("attempts/2026-07").join(format!("{RUN_ID}.json"));
	let authorization_contract_sha256 =
		crate::load_json_with_sha256(&write_auth_contract(temp.path()))
			.expect("authorization contract digest")
			.1;
	let attempt = json!({
		"schema": "decodex/xurl-publish-attempt/4",
		"run_id": RUN_ID,
		"reservation_ref": crate::path_arg(&root, reservation_path),
		"candidate_ref": crate::path_arg(&root, &candidate_path),
		"candidate_sha256": crate::load_json_with_sha256(&candidate_path)
			.expect("candidate digest").1,
			"idempotency_key": TEST_IDEMPOTENCY_KEY,
			"publication_lineage_sha256": TEST_PUBLICATION_LINEAGE,
		"billing_month": "2026-07",
		"target_account": "decodexspace",
		"status": "create_inflight",
		"created_at": "2026-07-27T12:02:00Z",
		"updated_at": "2026-07-27T12:02:00Z",
		"reserved_cost_ceiling_microusd": 30000,
		"xurl_version": "1.3.1",
		"pricing_policy_id": "x-api-pay-per-usage/2026-07-27",
		"authorization_contract_sha256": authorization_contract_sha256,
		"calls": [
			{
				"operation": "identity_read",
				"status": "succeeded",
				"recorded_cost_ceiling_microusd": 10000,
				"response_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
			},
			{
				"operation": "content_create",
				"status": "inflight",
				"recorded_cost_ceiling_microusd": 15000,
				"response_sha256": null
			}
		],
		"verified_user_id": "42",
		"post_id": null,
		"published_url": null
	});
	crate::write_new_json(&attempt_path, &attempt).expect("crash-state attempt");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let request = publish_request(temp.path(), reservation_path, RUN_ID);

	let error = crate::social_xurl::publish_with_test_binary(&request, &xurl)
		.expect_err("create-inflight state must stop")
		.to_string();
	assert!(error.contains("create outcome is unknown"));
	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert!(!log.lines().any(|line| line == "post"));
}

#[cfg(unix)]
#[test]
fn xurl_publish_rejects_url_before_public_write() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let mut candidate = valid_social_candidate();
	candidate["candidate_text"] = json!(["Source https://example.com"]);
	let candidate_path = temp.path().join("candidates/candidate.json");
	crate::write_new_json(&candidate_path, &candidate).expect("candidate should be written");
	let reservation = valid_social_publish_reservation_for_path(&candidate_path);
	let reservation_path = temp.path().join(format!(
		"reservations/2026-07-27/{}.json",
		crate::social_publish::idempotency_digest(TEST_IDEMPOTENCY_KEY)
	));
	crate::write_new_json(&reservation_path, &reservation).expect("reservation should be written");
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let request = publish_request(temp.path(), &reservation_path, RUN_ID);

	let error = crate::social_xurl::publish_with_test_binary(&request, &xurl)
		.expect_err("URL-bearing post must fail before create")
		.to_string();
	assert!(
		error.contains("candidate failed validation") || error.contains("must not contain a URL")
	);
	assert!(!log_path.exists());
}

#[test]
fn expired_reservations_are_terminalized_before_the_daily_cap_scan() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let first_request = reserve_request(temp.path(), &candidate_path, RUN_ID);
	let first = crate::reserve_social_publish(&first_request).expect("first reservation");

	let mut other = valid_social_candidate();
	other["slug"] = json!("another-change");
	other["decision"]["idempotency_key"] = json!("x:decodexspace:operator_impact:another-change");
	let other_path = write_candidate_named(temp.path(), "other.json", other);
	let mut next =
		reserve_request(temp.path(), &other_path, "019fa400-0000-7000-8000-000000000002");
	next.reserved_at = "2026-07-27T14:00:00Z".into();
	next.expires_at = "2026-07-27T15:00:00Z".into();
	let second = crate::reserve_social_publish(&next).expect("expired slot must be reclaimed");
	assert_eq!(second.status, "reserved");
	let expired = crate::load_json(Path::new(&first.path)).expect("expired reservation");
	assert_eq!(expired["status"], "expired");
	assert_eq!(expired["release_reason"], "Reservation expired before publication.");
}

#[test]
fn production_cli_accepts_fixed_xurl_surfaces() {
	use clap::Parser as _;

	assert!(crate::cli::Cli::try_parse_from(["decodex-publisher", "social", "gc"]).is_ok());
	assert!(crate::cli::Cli::try_parse_from(["decodex-publisher", "social", "probe-xurl"]).is_ok());
	assert!(
		crate::cli::Cli::try_parse_from(["decodex-publisher", "social", "cost-report"]).is_ok()
	);
	assert!(
		crate::cli::Cli::try_parse_from([
			"decodex-publisher",
			"social",
			"cost-report",
			"--month",
			"2026-07",
		])
		.is_ok()
	);
	assert!(
		crate::cli::Cli::try_parse_from(["decodex-publisher", "social", "seal-xurl-auth"]).is_ok()
	);
	assert!(
		crate::cli::Cli::try_parse_from([
			"decodex-publisher",
			"social",
			"seal-xurl-auth",
			"--authorization-request-file",
			"authorization-request.txt",
		])
		.is_err()
	);
	assert!(
		crate::cli::Cli::try_parse_from([
			"decodex-publisher",
			"social",
			"reconcile-xurl",
			"--evidence",
			"reservation.json",
			"--operation-id",
			SECOND_RUN_ID,
		])
		.is_ok()
	);
	assert!(
		crate::cli::Cli::try_parse_from([
			"decodex-publisher",
			"social",
			"reconcile-xurl",
			"--attempt",
			"attempt.json",
			"--operation-id",
			SECOND_RUN_ID,
		])
		.is_ok()
	);
	assert!(
		crate::cli::Cli::try_parse_from([
			"decodex-publisher",
			"social",
			"reconcile-xurl",
			"--evidence",
			"reservation.json",
			"--attempt",
			"attempt.json",
			"--operation-id",
			SECOND_RUN_ID,
		])
		.is_err()
	);
	assert!(
		crate::cli::Cli::try_parse_from([
			"decodex-publisher",
			"social",
			"reconcile-xurl",
			"--operation-id",
			SECOND_RUN_ID,
		])
		.is_err()
	);
}

#[test]
fn production_cli_rejects_time_budget_and_directory_overrides() {
	use clap::Parser as _;

	for arguments in [
		vec!["decodex-publisher", "social", "gc", "--now", "2026-07-27T12:00:00Z"],
		vec!["decodex-publisher", "social", "probe-xurl", "--now", "2026-07-27T12:00:00Z"],
		vec![
			"decodex-publisher",
			"social",
			"reconcile-xurl",
			"--evidence",
			"reservation.json",
			"--operation-id",
			SECOND_RUN_ID,
			"--attempts-dir",
			"/tmp/elsewhere",
		],
		vec![
			"decodex-publisher",
			"social",
			"reserve-publish",
			"--candidate",
			"candidate.json",
			"--run-id",
			RUN_ID,
			"--day",
			"2026-07-27",
		],
		vec![
			"decodex-publisher",
			"social",
			"publish-xurl",
			"--reservation",
			"reservation.json",
			"--run-id",
			RUN_ID,
			"--monthly-budget-microusd",
			"9999999",
		],
		vec![
			"decodex-publisher",
			"social",
			"observe-xurl",
			"--post",
			"post.json",
			"--window",
			"24h",
			"--attempts-dir",
			"/tmp/elsewhere",
		],
	] {
		assert!(crate::cli::Cli::try_parse_from(arguments).is_err());
	}
}

#[cfg(unix)]
#[test]
fn xurl_publish_fails_closed_when_monthly_budget_is_exhausted() {
	let temp = tempfile::tempdir().expect("temporary directory");
	let candidate_path = write_candidate(temp.path(), valid_social_candidate());
	let reservation =
		crate::reserve_social_publish(&reserve_request(temp.path(), &candidate_path, RUN_ID))
			.expect("reservation should succeed");
	for index in 0..41 {
		write_budget_publication_attempt(temp.path(), index);
	}
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let request = publish_request(temp.path(), Path::new(&reservation.path), RUN_ID);

	let error = crate::social_xurl::publish_with_test_binary(&request, &xurl)
		.expect_err("monthly cap must stop the public write")
		.to_string();
	assert!(error.contains("monthly X budget exhausted"));
	let log = fs::read_to_string(log_path).expect("xurl call log");
	assert!(!log.lines().any(|line| line == "post"));
}

#[test]
fn schema_extensions_cannot_hide_credentials_or_raw_api_payloads() {
	let mut post = valid_social_post();
	post["publication"]["access_token"] = json!("secret");
	assert_social_errors(&post, ["publication.access_token is not allowed"]);

	let mut outcome = valid_social_outcome();
	outcome["observation"]["raw_response"] = json!({"data": "private"});
	assert_social_errors(&outcome, ["observation.raw_response is not allowed"]);
}

#[cfg(unix)]
#[test]
fn private_state_rejects_symlinked_output_parents_and_scan_entries() {
	use std::os::unix::fs::symlink;

	let temp = tempfile::tempdir().expect("temporary directory");
	let target = temp.path().join("target");
	fs::create_dir(&target).expect("target directory");
	let linked_parent = temp.path().join("linked-parent");
	symlink(&target, &linked_parent).expect("parent symlink");
	let error = crate::write_new_json(&linked_parent.join("escaped.json"), &json!({"ok": true}))
		.expect_err("private output must not traverse a symlink")
		.to_string();
	assert!(error.contains("symlink"));

	let scan = temp.path().join("scan");
	let outside = temp.path().join("outside.json");
	crate::write_new_json(&outside, &json!({"ok": true})).expect("outside JSON");
	fs::create_dir(&scan).expect("scan directory");
	symlink(&outside, scan.join("linked.json")).expect("file symlink");
	let error = crate::collect_json_files(&[scan])
		.expect_err("private scan must reject a symlink")
		.to_string();
	assert!(error.contains("symlink"));
}

fn assert_social_errors<const N: usize>(payload: &Value, expected: [&str; N]) {
	let mut actual = crate::social_validation::validate_social_artifact(payload).errors;
	actual.sort();
	let mut expected = expected.into_iter().map(str::to_owned).collect::<Vec<_>>();
	expected.sort();
	assert_eq!(actual, expected);
}

struct CandidateFixture {
	path: PathBuf,
	_radar_directories: Vec<tempfile::TempDir>,
}
impl Deref for CandidateFixture {
	type Target = Path;

	fn deref(&self) -> &Self::Target {
		&self.path
	}
}
impl AsRef<Path> for CandidateFixture {
	fn as_ref(&self) -> &Path {
		&self.path
	}
}

fn write_candidate(root: &Path, candidate: Value) -> CandidateFixture {
	write_candidate_named(root, "candidate.json", candidate)
}

fn write_candidate_named(root: &Path, name: &str, mut candidate: Value) -> CandidateFixture {
	let radar_directories = attach_test_radar_lineage(&mut candidate);
	let path = root.join("candidates").join(name);
	crate::write_new_json(&path, &candidate).expect("candidate should be written");
	CandidateFixture { path, _radar_directories: radar_directories }
}

fn write_staged_candidate(root: &Path, name: &str, mut candidate: Value) -> CandidateFixture {
	let radar_directories = attach_test_radar_lineage(&mut candidate);
	let path = write_staging_value(root, name, &candidate);
	CandidateFixture { path, _radar_directories: radar_directories }
}

fn write_staging_value(root: &Path, name: &str, value: &Value) -> PathBuf {
	let path = root.join("staging").join(name);
	crate::write_new_json(&path, value).expect("staging artifact");
	path
}

fn manager_record_request(
	root: &Path,
	staging_path: &Path,
	run_id: &str,
) -> crate::SocialRecordManagerRequest {
	crate::SocialRecordManagerRequest {
		staging_path: staging_path.to_path_buf(),
		staging_dir: root.join("staging"),
		candidates_dir: root.join("candidates"),
		strategies_dir: root.join("strategies"),
		reservations_dir: root.join("reservations"),
		posts_dir: root.join("posts"),
		outcomes_dir: root.join("outcomes"),
		locks_dir: root.join("locks"),
		run_id: run_id.into(),
	}
}

fn attach_test_radar_lineage(candidate: &mut Value) -> Vec<tempfile::TempDir> {
	if candidate
		.get("decision")
		.and_then(Value::as_object)
		.and_then(|decision| decision.get("worthiness"))
		.and_then(Value::as_str)
		!= Some("publish")
	{
		return Vec::new();
	}

	let repo_root = crate::repo_root().expect("repo root");
	let directory = crate::repo_local_test_directory("publisher-radar-");
	let queue_dir = directory.path().join("github/review-queue");
	let pairs_dir = directory.path().join("github/content-review-pairs");
	crate::ensure_private_directory(&queue_dir).expect("private Radar queue collection");
	crate::ensure_private_directory(&pairs_dir).expect("private Radar pair collection");
	let queue_path = queue_dir.join("openai-codex-latest.json");
	let candidate_repo =
		candidate.get("repo").and_then(Value::as_str).expect("candidate repo").to_owned();
	let candidate_slug =
		candidate.get("slug").and_then(Value::as_str).expect("candidate slug").to_owned();
	let candidate_mode =
		candidate.get("mode").and_then(Value::as_str).expect("candidate mode").to_owned();
	let subject_id = if candidate_slug == "openai-codex-pr-22414" { "22414" } else { "22415" };
	let observed_at = time::OffsetDateTime::now_utc()
		.format(&time::format_description::well_known::Rfc3339)
		.expect("current RFC3339 timestamp");
	let mut queue = valid_radar_queue();
	queue["repo"] = json!(candidate_repo.clone());
	queue["generated_at"] = json!(observed_at.clone());
	queue["subjects"][0]["subject_id"] = json!(subject_id);
	let mut review = valid_radar_review();
	review["repo"] = json!(candidate_repo.clone());
	review["slug"] = json!(candidate_slug.clone());
	review["reviewed_at"] = json!(observed_at.clone());
	review["subject"]["subject_id"] = json!(subject_id);
	let review_raw = pretty_json_bytes(&review);
	let review_sha256 = digest_hex(&review_raw);
	let mut impact = valid_radar_impact();
	impact["repo"] = json!(candidate_repo.clone());
	impact["slug"] = json!(candidate_slug.clone());
	impact["review_lineage"]["slug"] = json!(candidate_slug.clone());
	impact["review_lineage"]["subject_id"] = json!(subject_id);
	impact["publisher_angle"] = json!(candidate_mode);
	impact["reviewed_at"] = json!(observed_at);
	impact["review_lineage"]["artifact_sha256"] = json!(review_sha256.clone());
	let impact_raw = pretty_json_bytes(&impact);
	let pair_digest = crate::social_record::radar_content_pair_sha256(&review_raw, &impact_raw);
	let pair_dir = pairs_dir.join(format!("{RUN_ID}--{}--{pair_digest}", "a".repeat(64)));
	crate::ensure_private_directory(&pair_dir).expect("private Radar pair directory");
	let review_path = pair_dir.join("review.json");
	let impact_path = pair_dir.join("impact.json");
	crate::write_new_json(&queue_path, &queue).expect("Radar queue fixture");
	crate::write_new_json(&review_path, &review).expect("Radar review fixture");
	crate::write_new_json(&impact_path, &impact).expect("Radar impact fixture");
	let queue_sha256 = crate::load_json_with_sha256(&queue_path).expect("queue digest").1;
	let review_sha256 = crate::load_json_with_sha256(&review_path).expect("review digest").1;
	let impact_sha256 = crate::load_json_with_sha256(&impact_path).expect("impact digest").1;
	let queue_ref = crate::path_arg(&repo_root, &queue_path);
	let review_ref = crate::path_arg(&repo_root, &review_path);
	let impact_ref = crate::path_arg(&repo_root, &impact_path);
	let lineage_sha256 = crate::social_record::eligibility_lineage_sha256(
		&candidate_repo,
		"pr",
		subject_id,
		&candidate_slug,
		"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
		&["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()],
		&queue_sha256,
		&review_sha256,
		&impact_sha256,
	);

	candidate["radar_source_refs"] = json!({
		"queue": queue_ref.clone(),
		"review": review_ref.clone(),
		"impact": impact_ref.clone()
	});
	candidate["radar_eligibility"] = json!({
		"schema": "radar_content_eligibility/v1",
		"repo": candidate_repo,
		"subject_kind": "pr",
		"subject_id": subject_id,
		"slug": candidate_slug,
		"upstream_head": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
		"commit_shas": ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
		"queue_sha256": queue_sha256,
		"review_sha256": review_sha256,
		"impact_sha256": impact_sha256,
		"lineage_sha256": lineage_sha256
	});
	candidate["decision"]["idempotency_key"] = json!(
		crate::social_record::publication_idempotency_key(candidate)
			.expect("publication idempotency key")
	);
	let source_refs =
		candidate["source_refs"].as_object_mut().expect("candidate source_refs object");
	source_refs.insert("upstream_reviews".into(), json!([review_ref.clone()]));
	source_refs.insert("upstream_impacts".into(), json!([impact_ref.clone()]));
	let evidence_digests = candidate
		.as_object_mut()
		.expect("candidate object")
		.entry("evidence_digests")
		.or_insert_with(|| json!({}))
		.as_object_mut()
		.expect("candidate evidence_digests object");
	evidence_digests.insert(review_ref.clone(), json!(review_sha256));
	evidence_digests.insert(impact_ref, json!(impact_sha256));
	evidence_digests.remove(PLACEHOLDER_REVIEW_REF);
	evidence_digests.remove(PLACEHOLDER_IMPACT_REF);
	for claim in candidate["claims"].as_array_mut().expect("candidate claims") {
		if claim.get("evidence").and_then(Value::as_str) == Some(PLACEHOLDER_REVIEW_REF) {
			claim["evidence"] = json!(review_ref);
		}
	}

	vec![directory]
}

fn pretty_json_bytes(value: &Value) -> Vec<u8> {
	let mut bytes = serde_json::to_vec_pretty(value).expect("fixture JSON");
	bytes.push(b'\n');
	bytes
}

fn digest_hex(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn write_auth_contract(root: &Path) -> PathBuf {
	let path = root.join("xurl-authorization-contract.json");
	if !path.exists() {
		crate::write_new_json(&path, &valid_auth_contract())
			.expect("authorization contract should be written");
	}
	path
}

fn valid_auth_contract() -> Value {
	json!({
		"schema": "decodex/xurl-authorization-contract/1",
		"policy_id": "xurl-oauth-least-privilege/3",
		"target_account": "decodexspace",
		"xurl_app": "default",
			"required_operator_authorized_scopes": [
				"tweet.read",
				"users.read",
				"tweet.write",
				"offline.access"
			],
		"xurl_version": "1.3.1",
		"xurl_binary_sha256":
			"7b85a210009db7a3f2d6183684674441fbf81276f1101f73d36d0266ec9aa01e",
		"sealed_at": "2026-07-27T00:00:00Z"
	})
}

#[cfg(unix)]
fn assert_probe_auth_rejected(
	receipt: Option<Value>,
	now: &str,
	expected_error: &str,
	expected_log: &str,
) -> String {
	let temp = tempfile::tempdir().expect("temporary directory");
	let receipt_path = temp.path().join("xurl-authorization-contract.json");
	if let Some(receipt) = receipt {
		crate::write_new_json(&receipt_path, &receipt).expect("authorization contract");
	}
	let log_path = temp.path().join("probe.log");
	let xurl = fake_probe_xurl(temp.path(), &log_path, None);
	let error = crate::social_xurl::probe_with_test_binary(now, &xurl, &receipt_path)
		.expect_err("invalid authorization contract must fail closed")
		.to_string();
	assert!(error.contains(expected_error), "{error}");
	let log = fs::read_to_string(log_path).unwrap_or_default();
	assert_eq!(log, expected_log);

	error
}

fn reserve_request(
	root: &Path,
	candidate_path: &Path,
	run_id: &str,
) -> SocialReservePublishRequest {
	SocialReservePublishRequest {
		candidate_path: candidate_path.to_path_buf(),
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

fn publish_request(root: &Path, reservation_path: &Path, run_id: &str) -> SocialPublishXurlRequest {
	SocialPublishXurlRequest {
		reservation_path: reservation_path.to_path_buf(),
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

fn observe_request(root: &Path, post_path: &Path, window: &str) -> SocialObserveXurlRequest {
	SocialObserveXurlRequest {
		run_id: RUN_ID.into(),
		post_path: post_path.to_path_buf(),
		authorization_contract_path: write_auth_contract(root),
		posts_dir: root.join("posts"),
		outcomes_dir: root.join("outcomes"),
		attempts_dir: root.join("attempts"),
		locks_dir: root.join("locks"),
		observed_at: "2026-07-28T12:02:00Z".into(),
		window: window.into(),
		monthly_budget_microusd: 1_250_000,
	}
}

fn reconcile_request(
	root: &Path,
	evidence_path: &Path,
	operation_id: &str,
	reconciled_at: &str,
) -> SocialReconcileXurlRequest {
	SocialReconcileXurlRequest {
		evidence_path: evidence_path.to_path_buf(),
		attempt_path: None,
		authorization_contract_path: write_auth_contract(root),
		reservations_dir: root.join("reservations"),
		candidates_dir: root.join("candidates"),
		posts_dir: root.join("posts"),
		outcomes_dir: root.join("outcomes"),
		attempts_dir: root.join("attempts"),
		locks_dir: root.join("locks"),
		operation_id: operation_id.into(),
		reconciled_at: reconciled_at.into(),
	}
}

fn reconcile_attempt_request(
	root: &Path,
	attempt_path: &Path,
	operation_id: &str,
	reconciled_at: &str,
) -> SocialReconcileXurlRequest {
	SocialReconcileXurlRequest {
		evidence_path: PathBuf::new(),
		attempt_path: Some(attempt_path.to_path_buf()),
		authorization_contract_path: write_auth_contract(root),
		reservations_dir: root.join("reservations"),
		candidates_dir: root.join("candidates"),
		posts_dir: root.join("posts"),
		outcomes_dir: root.join("outcomes"),
		attempts_dir: root.join("attempts"),
		locks_dir: root.join("locks"),
		operation_id: operation_id.into(),
		reconciled_at: reconciled_at.into(),
	}
}

fn xurl_call(operation: &str, status: &str, cost: u64) -> Value {
	let mut call = json!({
		"operation": operation,
		"status": status,
		"recorded_cost_ceiling_microusd": cost,
		"response_sha256": if status == "inflight" {
			Value::Null
		} else {
			Value::String("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into())
		}
	});
	if operation == "post_read_retry" {
		call["billing_month"] = json!("2026-07");
	}
	call
}

fn xurl_recovery_call(
	operation: &str,
	status: &str,
	cost: u64,
	operation_id: &str,
	billing_month: &str,
) -> Value {
	json!({
		"operation": operation,
		"operation_id": operation_id,
		"billing_month": billing_month,
		"status": status,
		"recorded_cost_ceiling_microusd": cost,
		"response_sha256": Value::Null
	})
}

struct SeedPublishAttempt<'a> {
	status: &'a str,
	reserved_cost_ceiling_microusd: u64,
	calls: Value,
	post_id: Option<&'a str>,
	published_url: Option<&'a str>,
}

fn write_publish_attempt(
	root: &Path,
	candidate_path: &Path,
	reservation_path: &Path,
	run_id: &str,
	seed: SeedPublishAttempt<'_>,
) {
	let repo_root = crate::repo_root().expect("repo root");
	let verified_user_id = (!seed.status.starts_with("identity_")).then_some("42");
	let candidate_sha256 =
		crate::load_json_with_sha256(candidate_path).expect("candidate digest").1;
	let authorization_contract_sha256 = crate::load_json_with_sha256(&write_auth_contract(root))
		.expect("authorization contract digest")
		.1;
	let attempt = json!({
		"schema": "decodex/xurl-publish-attempt/4",
		"run_id": run_id,
		"reservation_ref": crate::path_arg(&repo_root, reservation_path),
		"candidate_ref": crate::path_arg(&repo_root, candidate_path),
		"candidate_sha256": candidate_sha256,
		"idempotency_key": TEST_IDEMPOTENCY_KEY,
		"publication_lineage_sha256": TEST_PUBLICATION_LINEAGE,
		"billing_month": "2026-07",
		"target_account": "decodexspace",
		"status": seed.status,
		"created_at": "2026-07-27T12:02:00Z",
		"updated_at": "2026-07-27T12:02:00Z",
		"reserved_cost_ceiling_microusd": seed.reserved_cost_ceiling_microusd,
		"xurl_version": "1.3.1",
		"pricing_policy_id": "x-api-pay-per-usage/2026-07-27",
		"authorization_contract_sha256": authorization_contract_sha256,
		"calls": seed.calls,
		"verified_user_id": verified_user_id,
		"post_id": seed.post_id,
		"published_url": seed.published_url
	});
	crate::write_new_json(&root.join("attempts/2026-07").join(format!("{run_id}.json")), &attempt)
		.expect("publish attempt");
}

fn publish_attempt_path(root: &Path, run_id: &str) -> PathBuf {
	root.join("attempts/2026-07").join(format!("{run_id}.json"))
}

fn write_observation_attempt(
	root: &Path,
	post_path: &Path,
	status: &str,
	calls: Vec<Value>,
	reserved_cost_ceiling_microusd: u64,
) -> PathBuf {
	let repo_root = crate::repo_root().expect("repo root");
	let post_ref = crate::path_arg(&repo_root, post_path);
	let attempt_key = Sha256::digest(format!("{post_ref}\0{}", "24h").as_bytes())
		.iter()
		.map(|byte| format!("{byte:02x}"))
		.collect::<String>();
	let attempt_path = root.join("attempts/2026-07").join(format!("observe-{attempt_key}.json"));
	let authorization_contract_sha256 = crate::load_json_with_sha256(&write_auth_contract(root))
		.expect("authorization contract digest")
		.1;
	let call = calls.last().expect("observation call").clone();
	let attempt = json!({
		"schema": "decodex/xurl-observation-attempt/4",
		"run_id": RUN_ID,
		"billing_month": "2026-07",
		"reserved_cost_ceiling_microusd": reserved_cost_ceiling_microusd,
		"status": status,
			"post_ref": post_ref,
			"post_id": "2000000000000000001",
			"publication_lineage_sha256": TEST_PUBLICATION_LINEAGE,
			"window": "24h",
		"created_at": "2026-07-28T12:02:00Z",
		"updated_at": "2026-07-28T12:02:00Z",
		"pricing_policy_id": "x-api-pay-per-usage/2026-07-27",
		"authorization_contract_sha256": authorization_contract_sha256,
		"call": call,
		"calls": calls
	});
	crate::write_new_json(&attempt_path, &attempt).expect("observation attempt");
	attempt_path
}

#[cfg(unix)]
fn seeded_outcome_recovery_error(recovery_owners: &[&str], requested_owner: &str) -> String {
	let temp = tempfile::tempdir().expect("temporary directory");
	let post_path = temp.path().join(format!("posts/{RUN_ID}.json"));
	crate::write_new_json(&post_path, &valid_social_post()).expect("published post");
	let mut calls = vec![xurl_call("outcome_read", "failed", 5_000)];
	calls.extend(recovery_owners.iter().map(|owner| {
		xurl_recovery_call("outcome_read_reconcile", "failed", 5_000, owner, "2026-07")
	}));
	let attempt_path = write_observation_attempt(
		temp.path(),
		&post_path,
		"read_reconcile_halted",
		calls,
		5_000 * (1 + recovery_owners.len() as u64),
	);
	let log_path = temp.path().join("xurl.log");
	let xurl = fake_xurl(temp.path(), &log_path, "decodexspace");
	let request = reconcile_attempt_request(
		temp.path(),
		&attempt_path,
		requested_owner,
		"2026-07-28T12:03:00Z",
	);
	let error =
		crate::social_xurl::reconcile_attempt_with_test_binary_without_pricing(&request, &xurl)
			.expect_err("a recovery owner cannot reserve twice")
			.to_string();
	let log = fs::read_to_string(log_path).unwrap_or_default();
	assert!(!log.lines().any(|line| line == "read"));
	error
}

fn write_budget_publication_attempt(root: &Path, index: u64) {
	let run_id = format!("019fa400-1000-7000-8000-{index:012}");
	let publication_lineage_sha256 = format!("{index:064x}");
	let attempt = json!({
		"schema": "decodex/xurl-publish-attempt/4",
		"run_id": run_id,
		"reservation_ref": format!("budget-reservation-{index}.json"),
		"candidate_ref": format!("budget-candidate-{index}.json"),
		"idempotency_key": format!("radar-publication:{publication_lineage_sha256}"),
		"publication_lineage_sha256": publication_lineage_sha256,
		"billing_month": "2026-07",
		"target_account": "decodexspace",
		"status": "reserved",
		"created_at": "2026-07-01T00:00:00Z",
		"updated_at": "2026-07-01T00:00:00Z",
		"reserved_cost_ceiling_microusd": 30_000,
		"xurl_version": "1.3.1",
		"pricing_policy_id": "x-api-pay-per-usage/2026-07-27",
		"authorization_contract_sha256":
			"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		"calls": [],
		"verified_user_id": null,
		"post_id": null,
		"published_url": null
	});
	crate::write_new_json(
		&root.join("attempts/2026-07").join(format!("budget-publication-{index}.json")),
		&attempt,
	)
	.expect("publication budget attempt");
}

fn write_budget_observation_attempt(root: &Path, index: u64) {
	let call = xurl_call("outcome_read", "succeeded", 5_000);
	let publication_lineage_sha256 = format!("{:064x}", index + 10_000);
	let attempt = json!({
		"schema": "decodex/xurl-observation-attempt/4",
		"run_id": format!("019fa400-2000-7000-8000-{index:012}"),
		"billing_month": "2026-07",
		"reserved_cost_ceiling_microusd": 5_000,
		"status": "observed",
		"post_ref": format!("budget-post-{index}.json"),
		"post_id": format!("{}", index + 1),
		"publication_lineage_sha256": publication_lineage_sha256,
		"window": "24h",
		"created_at": "2026-07-01T00:00:00Z",
		"updated_at": "2026-07-01T00:00:00Z",
		"pricing_policy_id": "x-api-pay-per-usage/2026-07-27",
		"authorization_contract_sha256":
			"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		"call": call,
		"calls": [call]
	});
	crate::write_new_json(
		&root.join("attempts/2026-07").join(format!("budget-observation-{index}.json")),
		&attempt,
	)
	.expect("observation budget attempt");
}

fn skip_request(root: &Path, candidate_path: &Path) -> SocialTerminalizeSkipRequest {
	SocialTerminalizeSkipRequest {
		candidate_path: candidate_path.to_path_buf(),
		candidates_dir: root.join("candidates"),
		reservations_dir: root.join("reservations"),
		posts_dir: root.join("posts"),
		locks_dir: root.join("locks"),
		run_id: RUN_ID.into(),
		day: "2026-07-27".into(),
		timezone: "UTC".into(),
		daily_limit: 1,
		dry_run: false,
	}
}

#[cfg(unix)]
fn fake_xurl(root: &Path, log_path: &Path, account: &str) -> PathBuf {
	fake_xurl_with_identity(root, log_path, account, account, true)
}

#[cfg(unix)]
fn fake_probe_xurl(root: &Path, log_path: &Path, malicious_marker: Option<&Path>) -> PathBuf {
	use std::os::unix::fs::PermissionsExt as _;

	let path = root.join("xurl");
	let script = malicious_marker.map_or_else(
		|| {
			format!(
				r#"#!/bin/sh
set -eu
if [ -n "${{XURL_HOSTILE_TEST+x}}" ]; then
  exit 91
fi
if [ "$#" -eq 1 ] && [ "$1" = "--version" ]; then
  echo "version" >> '{log}'
  echo "xurl version 1.3.1"
  exit 0
fi
if [ "$#" -eq 4 ] && [ "$1" = "--app" ] && [ "$2" = "default" ] && [ "$3" = "auth" ] && [ "$4" = "status" ]; then
  echo "auth status" >> '{log}'
  echo "▸ default  [client_id: private-client-id]"
  echo "      oauth2: decodexspace"
  exit 0
fi
echo "unexpected" >> '{log}'
exit 92
"#,
				log = log_path.display(),
			)
		},
		|marker| {
			format!(
				"#!/bin/sh\nset -eu\nprintf '%s\\n' malicious > '{}'\nexit 99\n",
				marker.display()
			)
		},
	);
	fs::write(&path, script).expect("fake probe xurl should be written");
	let mut permissions = fs::metadata(&path).expect("fake probe xurl metadata").permissions();
	permissions.set_mode(0o700);
	fs::set_permissions(&path, permissions).expect("fake probe xurl should be executable");
	path
}

#[cfg(unix)]
fn fake_xurl_with_identity(
	root: &Path,
	log_path: &Path,
	account_label: &str,
	identity: &str,
	valid_create: bool,
) -> PathBuf {
	fake_xurl_config(root, log_path, account_label, identity, valid_create, FakeReadMode::Succeed)
}

#[cfg(unix)]
#[derive(Clone, Copy)]
enum FakeReadMode {
	Succeed,
	FailFirst,
	FailAlways,
}

#[cfg(unix)]
fn fake_xurl_config(
	root: &Path,
	log_path: &Path,
	account_label: &str,
	identity: &str,
	valid_create: bool,
	read_mode: FakeReadMode,
) -> PathBuf {
	use std::os::unix::fs::PermissionsExt as _;

	let path = root.join("xurl");
	let create = if valid_create {
		format!(r#"printf '%s\n' '{{"data":{{"id":"2000000000000000001","text":"{POST_TEXT}"}}}}'"#)
	} else {
		"printf '%s\n' '{\"data\":{\"text\":\"missing id\"}}'".into()
	};
	let read_guard = match read_mode {
		FakeReadMode::Succeed => String::new(),
		FakeReadMode::FailFirst => format!(
			"if [ ! -f '{marker}' ]; then touch '{marker}'; exit 1; fi",
			marker = root.join("read.failed-once").display()
		),
		FakeReadMode::FailAlways => "exit 1".into(),
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
  {read_guard}
  printf '%s\n' '{{"data":{{"id":"2000000000000000001","text":"{text}","author_id":"42","public_metrics":{{"impression_count":10,"like_count":1,"reply_count":0,"retweet_count":0,"bookmark_count":0}}}},"includes":{{"users":[{{"id":"42","username":"decodexspace"}}]}}}}'
else
  exit 2
fi
"#,
		log = log_path.display(),
		text = POST_TEXT,
	);
	fs::write(&path, script).expect("fake xurl should be written");
	let mut permissions = fs::metadata(&path).expect("fake xurl metadata").permissions();
	permissions.set_mode(0o700);
	fs::set_permissions(&path, permissions).expect("fake xurl should be executable");
	path
}

pub(crate) fn valid_social_candidate() -> Value {
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
			"queue_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
			"review_sha256": "1111111111111111111111111111111111111111111111111111111111111111",
			"impact_sha256": "2222222222222222222222222222222222222222222222222222222222222222",
			"lineage_sha256": "3333333333333333333333333333333333333333333333333333333333333333"
		},
		"radar_source_refs": {
			"queue": ".agent/automations/radar/cache/github/review-queue/openai-codex-latest.json",
			"review": PLACEHOLDER_REVIEW_REF,
			"impact": PLACEHOLDER_IMPACT_REF
		},
		"source_refs": {
			"upstream_reviews": [PLACEHOLDER_REVIEW_REF],
			"upstream_impacts": [PLACEHOLDER_IMPACT_REF]
		},
		"evidence_digests": {
			(PLACEHOLDER_REVIEW_REF): "1111111111111111111111111111111111111111111111111111111111111111",
			(PLACEHOLDER_IMPACT_REF):
				"2222222222222222222222222222222222222222222222222222222222222222"
		},
		"evidence_notes": ["PR #22414 changes an app-server capability boundary."],
		"claims": [{
			"text": POST_TEXT,
			"evidence": PLACEHOLDER_REVIEW_REF,
			"confidence": "confirmed"
		}],
		"decision": {
			"worthiness": "publish",
			"idempotency_key": TEST_IDEMPOTENCY_KEY,
			"reason": "The change alters an operator-visible protocol workflow."
		}
	})
}

pub(crate) fn valid_social_strategy(cycle_key: &str) -> Value {
	json!({
		"schema": "social_strategy/v1",
		"cycle_key": cycle_key,
		"cadence": "daily",
		"reviewed_at": "2026-07-27T12:00:00Z",
		"evidence_refs": ["manager:no-change"],
		"decisions": [{
			"dimension": "no_change",
			"key": "daily-quality-review",
			"previous_value": "unchanged",
			"next_value": "unchanged",
			"reason": "No evidence supports a strategy change."
		}],
		"guardrails": {
			"evidence_gate": "unchanged",
			"privacy_gate": "unchanged",
			"idempotency_gate": "unchanged",
			"account_gate": "unchanged",
			"publication_gate": "unchanged"
		},
		"next_review_at": "2026-07-28T12:00:00Z"
	})
}

fn valid_radar_queue() -> Value {
	json!({
		"schema": "upstream_review_queue/v1",
		"repo": "openai/codex",
		"generated_at": "2026-07-27T00:00:00Z",
		"source": {
			"default_branch": "main",
			"upstream_head": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
			"search_limit": 40
		},
		"subjects": [{
			"subject_kind": "pr",
			"subject_id": "22414",
			"title": "Add typed capability checks",
			"url": "https://github.com/openai/codex/pull/22414",
			"source_state": "merged",
			"commit_shas": ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"],
			"changed_file_count": 1,
			"sample_paths": ["codex-rs/app-server/src/lib.rs"],
			"surface_hints": ["app_server_protocol"],
			"attention_flags": [],
			"review_priority": "high",
			"review_reason": "Protocol capability behavior changed.",
			"next_step": "ai_review_required"
		}],
		"counts": {
			"subjects_queued": 1,
			"recent_commits_scanned": 1,
			"published_subjects_seen": 0,
			"critical": 0,
			"high": 1,
			"normal": 0,
			"low": 0
		}
	})
}

fn valid_radar_review() -> Value {
	json!({
		"schema": "upstream_review/v1",
		"slug": "openai-codex-pr-22414",
		"repo": "openai/codex",
		"upstream_head": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
		"subject": {
			"subject_kind": "pr",
			"subject_id": "22414",
			"commit_shas": ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]
		},
		"source_refs": {
			"items": [{
				"kind": "pull_request",
				"title": "Add typed capability checks",
				"url": "https://github.com/openai/codex/pull/22414"
			}]
		},
		"reviewed_at": "2026-07-27T00:00:00Z",
		"observed_change": "The app-server adds a typed capability check.",
		"changed_surfaces": ["app server"],
		"confidence": "confirmed",
		"evidence": ["PR #22414 changes the capability boundary."],
		"next_actions": [{
			"type": "upstream_impact",
			"reason": "The protocol change affects operator workflows."
		}]
	})
}

fn valid_radar_impact() -> Value {
	json!({
		"schema": "upstream_impact/v1",
		"slug": "openai-codex-pr-22414",
		"repo": "openai/codex",
		"reviewed_at": "2026-07-27T00:00:00Z",
		"review_lineage": {
			"artifact_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
			"slug": "openai-codex-pr-22414",
			"subject_kind": "pr",
			"subject_id": "22414",
			"upstream_head": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
			"commit_shas": ["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]
		},
		"source_refs": {
			"items": [{
				"kind": "pull_request",
				"title": "Add typed capability checks",
				"url": "https://github.com/openai/codex/pull/22414"
			}]
		},
		"observed_change": "The app-server adds a typed capability check.",
		"public_signal_decision": "publish",
		"control_plane_impact": "candidate",
		"publisher_angle": "operator_impact",
		"confidence": "confirmed",
		"evidence": ["PR #22414 changes the capability boundary."]
	})
}

pub(crate) fn valid_social_publish_reservation() -> Value {
	json!({
		"schema": "social_publish_reservation/v1",
		"slug": "openai-codex-pr-22414",
		"channel": "x",
		"target_account": "decodexspace",
		"mode": "operator_impact",
		"status": "active",
		"idempotency_key": TEST_IDEMPOTENCY_KEY,
		"publication_lineage_sha256": TEST_PUBLICATION_LINEAGE,
		"reserved_at": "2026-07-27T12:00:00Z",
		"expires_at": "2026-07-27T13:00:00Z",
		"day": "2026-07-27",
		"timezone": "UTC",
		"candidate_refs": {
			"social_candidates": [
				".agent/automations/decodex/cache/social/x/candidates/openai-codex-pr-22414.json"
			]
		},
		"duplicate_keys": [
			"openai-codex-pr-22414",
			TEST_IDEMPOTENCY_KEY
		],
		"owner": {
			"automation_id": "decodex-xurl-publisher",
			"run_id": RUN_ID
		}
	})
}

fn valid_social_publish_reservation_for_path(candidate_path: &Path) -> Value {
	let mut reservation = valid_social_publish_reservation();
	reservation["candidate_refs"]["social_candidates"] = json!([candidate_path.to_string_lossy()]);
	reservation
}

pub(crate) fn valid_social_post() -> Value {
	json!({
		"schema": "social_post/v1",
		"slug": "openai-codex-pr-22414",
		"channel": "x",
		"target_account": "decodexspace",
		"owner": {
			"automation_id": "decodex-xurl-publisher",
			"run_id": RUN_ID
		},
		"mode": "operator_impact",
		"status": "published",
		"audience": "Codex operators",
		"text": [POST_TEXT],
		"source_refs": {
			"reservations": [".agent/automations/decodex/cache/social/x/reservations/2026-07-27/reservation.json"],
			"social_candidates": [".agent/automations/decodex/cache/social/x/candidates/openai-codex-pr-22414.json"],
			"urls": ["https://github.com/openai/codex/pull/22414"]
		},
		"evidence_notes": ["PR #22414 changes an app-server capability boundary."],
		"claims": [{
			"text": "The app-server exposes a typed capability check.",
			"evidence": "https://github.com/openai/codex/pull/22414",
			"confidence": "confirmed"
		}],
		"decision": {
			"worthiness": "publish",
			"priority": "high",
			"idempotency_key": TEST_IDEMPOTENCY_KEY,
			"reason": "The change alters an operator-visible protocol workflow.",
			"daily_limit": 1,
			"daily_count_before": 0,
			"daily_count_after": 1,
			"day": "2026-07-27",
			"timezone": "UTC"
		},
		"publication": {
			"posted_at": "2026-07-27T12:02:00Z",
			"published_urls": ["https://x.com/decodexspace/status/2000000000000000001"],
			"post_id": "2000000000000000001",
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
				"publication_lineage_sha256": TEST_PUBLICATION_LINEAGE,
				"recorded_cost_ceiling_microusd": 30000
		}
	})
}

pub(crate) fn valid_social_outcome() -> Value {
	json!({
		"schema": "social_outcome/v1",
		"slug": "openai-codex-pr-22414-24h",
		"target_account": "decodexspace",
		"owner": {
			"automation_id": "decodex-xurl-publisher",
			"run_id": RUN_ID
		},
		"social_post_ref": ".agent/automations/decodex/cache/social/x/posts/019fa400-0000-7000-8000-000000000001.json",
		"published_url": "https://x.com/decodexspace/status/2000000000000000001",
		"observed_at": "2026-07-28T12:02:00Z",
		"window": "24h",
		"metrics": {
			"views": 125,
			"likes": 4,
			"replies": 1,
			"reposts": 2
		},
		"observation": {
			"reader": "xurl",
			"xurl_version": "1.3.1",
			"xurl_app": "default",
				"verified_account": "decodexspace",
				"publication_lineage_sha256": TEST_PUBLICATION_LINEAGE,
				"response_sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
			"recorded_cost_ceiling_microusd": 5000
		},
		"notes": ["Metrics were read through the bounded xurl post lookup."]
	})
}
