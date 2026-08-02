use std::{
	ffi::CString,
	fs::{self, FileTimes},
	io::Write as _,
	os::unix::{ffi::OsStrExt as _, fs::MetadataExt as _},
	sync::mpsc,
	thread,
	time::{Duration, SystemTime},
};

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
fn rejects_a_replayed_queue_at_an_alternate_private_path() {
	let temp_dir = crate::test_support::private_tempdir();
	let (mut request, queue_path, _, _) = write_fresh_private_artifacts(temp_dir.path());
	let alternate = queue_path.with_file_name("replayed.json");
	let bytes = fs::read(&queue_path).expect("canonical queue fixture");
	crate::write_private_file_atomic(&alternate, &bytes).expect("alternate queue fixture");
	request.queue = alternate;

	let error = crate::content_eligibility(&request)
		.expect_err("an alternate queue path must not carry eligibility authority");
	assert!(error.to_string().contains("github/review-queue/openai-codex-latest.json"));
}

#[test]
fn private_eligibility_rejects_retired_pair_paths_and_stale_pair_digests() {
	for case in ["retired-two-part", "stale-pair-digest"] {
		let temp_dir = crate::test_support::private_tempdir();
		let (mut request, _, review_path, impact_path) =
			write_fresh_private_artifacts(temp_dir.path());
		let pair = review_path.parent().expect("pair directory");
		let base = pair.parent().expect("pair collection");
		let run = "019fa400-0000-7000-8000-000000000001";
		let replacement_name = match case {
			"retired-two-part" => format!("{run}--{}", "a".repeat(64)),
			"stale-pair-digest" => format!("{run}--{}--{}", "a".repeat(64), "0".repeat(64)),
			_ => unreachable!(),
		};
		let replacement = base.join(replacement_name);

		fs::rename(pair, &replacement).expect("pair directory should be renamed");
		request.review = replacement.join("review.json");
		request.impact = replacement.join("impact.json");
		let error = crate::content_eligibility(&request)
			.expect_err("private eligibility must reject a non-authoritative pair path");

		assert!(
			error.to_string().contains("directory") || error.to_string().contains("digest"),
			"{case}: {error:?}"
		);
		assert!(!impact_path.exists());
	}
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
		crate::read_regular_file_bounded_with(&path, 4, "content eligibility input", move || {
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

#[test]
fn regular_content_read_rejects_a_symlink_and_an_initially_oversized_file() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let target = temp_dir.path().join("target.json");
	let symlink = temp_dir.path().join("symlink.json");
	let oversized = temp_dir.path().join("oversized.json");

	fs::write(&target, b"1234").expect("target fixture should be written");
	std::os::unix::fs::symlink(&target, &symlink).expect("symlink fixture should be created");
	fs::write(&oversized, b"12345").expect("oversized fixture should be written");

	let symlink_error = crate::read_regular_file_bounded(&symlink, 4, "content eligibility input")
		.expect_err("a symlink must fail no-follow validation");
	let oversized_error =
		crate::read_regular_file_bounded(&oversized, 4, "content eligibility input")
			.expect_err("an initially oversized file must fail before allocation");

	assert!(symlink_error.to_string().contains("regular non-symlink"));
	assert!(oversized_error.to_string().contains("bounded read limit"));
}

#[test]
fn regular_content_read_rejects_a_fifo_without_blocking() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let path = temp_dir.path().join("artifact.fifo");
	let fifo = CString::new(path.as_os_str().as_bytes()).expect("FIFO path should not contain NUL");

	assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0, "FIFO should be created");
	let (sender, receiver) = mpsc::channel();
	let reader = thread::spawn(move || {
		let result = crate::read_regular_file_bounded(&path, 4, "content eligibility input");

		sender.send(result).expect("FIFO read result should be observed");
	});
	let result = receiver
		.recv_timeout(Duration::from_secs(2))
		.expect("external FIFO bounded read must not wait for a writer");

	reader.join().expect("FIFO reader thread should finish");
	let error = result.expect_err("a FIFO must fail regular-file validation");

	assert!(error.to_string().contains("regular non-symlink"));
}

#[test]
fn regular_content_read_rejects_path_replacement_during_read() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let path = temp_dir.path().join("artifact.json");
	let displaced = temp_dir.path().join("displaced.json");

	fs::write(&path, b"1234").expect("fixture should be written");
	let replacement_path = path.clone();
	let error =
		crate::read_regular_file_bounded_with(&path, 4, "content eligibility input", move || {
			fs::rename(&replacement_path, &displaced).expect("fixture should be displaced");
			fs::write(&replacement_path, b"1234").expect("replacement should be written");
		})
		.expect_err("a pathname replacement must fail identity revalidation");

	assert!(error.to_string().contains("identity changed during read"));
}

#[test]
fn regular_content_read_detects_an_mtime_preserving_in_place_rewrite() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let path = temp_dir.path().join("artifact.json");
	let modified = SystemTime::now() - Duration::from_secs(60);

	fs::write(&path, b"1234").expect("fixture should be written");
	let file = fs::OpenOptions::new().write(true).open(&path).expect("fixture should reopen");

	file.set_times(FileTimes::new().set_modified(modified)).expect("fixture mtime should be set");
	let initial = fs::metadata(&path).expect("initial metadata should be readable");
	let initial_ctime = (initial.ctime(), initial.ctime_nsec());
	let rewrite_path = path.clone();
	let error =
		crate::read_regular_file_bounded_with(&path, 4, "content eligibility input", move || {
			thread::sleep(Duration::from_millis(10));
			let mut file = fs::OpenOptions::new()
				.write(true)
				.truncate(true)
				.open(&rewrite_path)
				.expect("fixture should reopen in place");

			file.write_all(b"5678").expect("replacement bytes should be written");
			file.sync_all().expect("replacement bytes should be visible");
			file.set_times(FileTimes::new().set_modified(modified))
				.expect("fixture mtime should be restored");
			let changed = file.metadata().expect("changed metadata should be readable");

			assert_ne!((changed.ctime(), changed.ctime_nsec()), initial_ctime);
		})
		.expect_err("ctime must detect same-inode, same-size, mtime-preserving rewrites");

	assert!(error.to_string().contains("identity changed during read"));
}

fn write_fresh_artifacts(
	root: &std::path::Path,
) -> (RadarContentEligibilityRequest, std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
	let timestamp = crate::utc_now_iso().expect("current timestamp should format");
	let mut queue = fixtures::valid_review_queue();
	let mut review = fixtures::valid_upstream_review();
	let mut impact = fixtures::valid_upstream_impact();
	let queue_path = root.join(crate::paths::REVIEW_QUEUE_RELATIVE_PATH);
	let review_path = root.join("review.json");
	let impact_path = root.join("impact.json");

	queue["generated_at"] = serde_json::json!(timestamp);
	review["reviewed_at"] = serde_json::json!(timestamp);
	impact["reviewed_at"] = serde_json::json!(timestamp);
	fs::create_dir_all(queue_path.parent().expect("queue parent"))
		.expect("queue parent should be created");
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

	queue["generated_at"] = serde_json::json!(timestamp);
	review["reviewed_at"] = serde_json::json!(timestamp);
	impact["reviewed_at"] = serde_json::json!(timestamp);
	crate::write_json(&queue_path, &queue).expect("private queue should be written");
	let review_raw = pretty_bytes(&review);
	impact["review_lineage"]["artifact_sha256"] = serde_json::json!(digest_hex(&review_raw));
	let impact_raw = pretty_bytes(&impact);
	let pair_digest = content_pair_digest(&review_raw, &impact_raw);
	let pair = cache.join(format!(
		"github/content-review-pairs/019fa400-0000-7000-8000-000000000001--{}--{pair_digest}",
		"a".repeat(64)
	));
	let review_path = pair.join("review.json");
	let impact_path = pair.join("impact.json");
	crate::write_private_file_atomic(&review_path, &review_raw)
		.expect("private review should be written");
	crate::write_private_file_atomic(&impact_path, &impact_raw)
		.expect("private impact should be written");

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

fn pretty_bytes(value: &serde_json::Value) -> Vec<u8> {
	let mut bytes = serde_json::to_vec_pretty(value).expect("fixture should serialize");
	bytes.push(b'\n');
	bytes
}

fn content_pair_digest(review: &[u8], impact: &[u8]) -> String {
	let mut digest = Sha256::new();

	digest.update(b"radar-content-review-pair-v1");
	for payload in [review, impact] {
		digest.update(u64::try_from(payload.len()).expect("fixture length").to_be_bytes());
		digest.update(payload);
	}

	digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn digest_hex(payload: &[u8]) -> String {
	Sha256::digest(payload).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn set_review_digest(impact: &mut serde_json::Value, review_path: &std::path::Path) {
	let review = fs::read(review_path).expect("review bytes should be readable");

	impact["review_lineage"]["artifact_sha256"] = serde_json::json!(digest_hex(&review));
}
