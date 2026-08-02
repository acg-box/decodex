use std::{
	fs,
	os::unix::fs::{PermissionsExt as _, symlink},
	path::Path,
};

use serde_json::json;
use sha2::{Digest as _, Sha256};

use crate::RadarContentV2ResetRequest;

fn pretty_json_bytes(value: &serde_json::Value) -> Vec<u8> {
	let mut bytes = serde_json::to_vec_pretty(value).expect("fixture serialization");
	bytes.push(b'\n');
	bytes
}

fn sha256_hex(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn pair_sha256(review: &[u8], impact: &[u8]) -> String {
	let mut digest = Sha256::new();
	digest.update(b"radar-content-review-pair-v1");
	for payload in [review, impact] {
		digest.update((payload.len() as u64).to_be_bytes());
		digest.update(payload);
	}
	sha256_hex(&digest.finalize())
}

#[test]
fn reset_removes_retired_content_state_preserves_authority_and_is_idempotent() {
	let temp = crate::test_support::private_tempdir();
	let root = temp.path().join(crate::DEFAULT_CACHE_ROOT);
	let old_pair = root.join("github/content-review-pairs/run--digest/review.json");
	let old_staging = root.join("github/content-review-staging/run.json");
	let old_bundle = root.join("github/bundles/run.json");
	let old_upgrade = root.join("github/control-plane-upgrades/run.json");
	let ledger = root.join("github/radar.sqlite3");
	let queue = root.join("github/review-queue/openai-codex-latest.json");
	let signal = root.join("site-content/signals/keep.json");

	for path in [&old_pair, &old_staging, &old_bundle, &old_upgrade, &ledger, &queue, &signal] {
		crate::write_private_file_atomic(path, b"{}\n").expect("private fixture should be written");
	}
	let report = crate::reset_content_v2(&RadarContentV2ResetRequest { cache_root: root.clone() })
		.expect("fresh-start reset should succeed");

	assert_eq!(report.schema, "radar_content_v2_reset/v1");
	assert_eq!(report.status, "reset");
	assert_eq!(report.collections_cleared, 4);
	assert_eq!(report.files_removed, 4);
	assert!(report.directories_removed >= 5);
	assert_eq!(report.bytes_removed, 12);
	for path in [old_pair, old_staging, old_bundle, old_upgrade] {
		assert!(!path.exists());
	}
	for path in [ledger, queue, signal] {
		assert!(path.exists(), "reset must preserve {}", path.display());
	}

	let second = crate::reset_content_v2(&RadarContentV2ResetRequest { cache_root: root })
		.expect("second reset should be a no-op");
	assert_eq!(second.status, "already_active");
	assert_eq!(second.collections_cleared, 0);
	assert_eq!(second.files_removed, 0);
	assert_eq!(second.directories_removed, 0);
	assert_eq!(second.bytes_removed, 0);
}

#[test]
fn reset_fails_before_deletion_for_symlinks_and_hardlinks() {
	for unsafe_kind in ["symlink", "hardlink"] {
		let temp = crate::test_support::private_tempdir();
		let root = temp.path().join(crate::DEFAULT_CACHE_ROOT);
		let preserved = root.join("github/content-review-pairs/old/review.json");
		let unsafe_path = root.join("github/bundles/unsafe.json");
		let target = root.join("outside.json");

		crate::write_private_file_atomic(&preserved, b"{}\n").expect("pair fixture");
		crate::write_private_file_atomic(&target, b"{}\n").expect("target fixture");
		let placeholder = unsafe_path.parent().expect("bundle parent").join("placeholder.json");
		crate::write_private_file_atomic(&placeholder, b"{}\n").expect("bundle directory fixture");
		fs::remove_file(placeholder).expect("placeholder should be removed");
		match unsafe_kind {
			"symlink" => symlink(&target, &unsafe_path).expect("unsafe symlink fixture"),
			"hardlink" => fs::hard_link(&target, &unsafe_path).expect("unsafe hardlink fixture"),
			_ => unreachable!(),
		}

		let error =
			crate::reset_content_v2(&RadarContentV2ResetRequest { cache_root: root.clone() })
				.expect_err("unsafe entry must fail the reset preflight");
		assert!(error.to_string().contains("symlink") || error.to_string().contains("link count"));
		assert!(preserved.exists(), "preflight failure must not delete another collection");
		assert!(!root.join("content-v2-activation.json").exists());
	}
}

#[test]
fn matched_tree_removal_rejects_root_replacement() {
	let temp = crate::test_support::private_tempdir();
	let root = temp.path().join(crate::DEFAULT_CACHE_ROOT);
	let collection = Path::new("github/bundles");
	let collection_path = root.join(collection);
	let displaced = root.join("github/displaced-bundles");

	crate::write_private_file_atomic(&collection_path.join("old.json"), b"{}\n")
		.expect("collection fixture");
	let cache = crate::private_fs::PrivateCache::open_existing(&root).expect("private cache");
	let lock = cache.lock().expect("cache lock");
	let snapshot = lock
		.inspect_directory_tree(collection, 16, 16, 1024)
		.expect("tree inventory")
		.expect("collection should exist");
	let replacement_path = collection_path.clone();
	let displaced_for_hook = displaced.clone();
	let error = lock
		.remove_directory_atomic_if_matches_after_inventory(collection, &snapshot, move || {
			fs::rename(&replacement_path, &displaced_for_hook)
				.expect("original collection displacement");
			fs::create_dir(&replacement_path).expect("replacement collection");
			fs::set_permissions(&replacement_path, fs::Permissions::from_mode(0o700))
				.expect("replacement collection mode");
			let replacement_file = replacement_path.join("new.json");
			fs::write(&replacement_file, b"{}\n").expect("replacement file");
			fs::set_permissions(&replacement_file, fs::Permissions::from_mode(0o600))
				.expect("replacement file mode");
		})
		.expect_err("root replacement must be rejected");

	assert!(error.to_string().contains("identity changed during isolation"));
	assert!(collection_path.join("new.json").exists());
	assert!(displaced.join("old.json").exists());
}

#[test]
fn marker_readback_preserves_post_activation_v2_state_byte_for_byte() {
	let temp = crate::test_support::private_tempdir();
	let root = temp.path().join(crate::DEFAULT_CACHE_ROOT);
	let request = RadarContentV2ResetRequest { cache_root: root.clone() };
	let first = crate::reset_content_v2(&request).expect("initial reset should install marker");
	assert_eq!(first.status, "reset");

	let run_id = "019fa400-0000-7000-8000-000000000001";
	let mut bundle = crate::tests::fixtures::valid_bundle();
	bundle["files"][0]["patch_excerpt"] = json!("+pub fn current_v2_anchor() {}");
	bundle["docs_refs"] = json!([]);
	bundle["examples_refs"] = json!([]);
	let bundle_raw = pretty_json_bytes(&bundle);
	let (_, receipt) = crate::bundle_evidence_from_bytes(&bundle_raw).expect("bundle receipt");

	let mut review = crate::tests::fixtures::valid_upstream_review();
	review["evidence"] = json!(["codex-rs/app-server/src/lib.rs: current implementation"]);
	let review_raw = pretty_json_bytes(&review);
	let mut staged_impact = crate::tests::fixtures::valid_upstream_impact();
	staged_impact["evidence"] = json!(["codex-rs/app-server/src/lib.rs: current implementation"]);
	let staging = json!({
		"schema": "radar_content_review_pair_staging/v2",
		"run_id": run_id,
		"queue_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		"selection_sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
		"bundle_evidence_receipt": receipt,
		"patch_anchor": {
			"path": "codex-rs/app-server/src/lib.rs",
			"kind": "implementation"
		},
		"review": review,
		"impact": staged_impact
	});
	let staging_raw = pretty_json_bytes(&staging);

	let mut impact = crate::tests::fixtures::valid_upstream_impact();
	impact["evidence"] = json!(["codex-rs/app-server/src/lib.rs: current implementation"]);
	impact["review_lineage"]["artifact_sha256"] = json!(sha256_hex(&review_raw));
	let impact_raw = pretty_json_bytes(&impact);
	let pair_digest = pair_sha256(&review_raw, &impact_raw);
	let pair_dir = format!(
		"github/content-review-pairs/019fa400-0000-7000-8000-000000000002--{}--{pair_digest}",
		"c".repeat(64)
	);
	let mut upgrade = crate::tests::fixtures::valid_control_plane_upgrade_candidate();
	upgrade["source_refs"]["upstream_reviews"] =
		json!([format!(".agent/automations/radar/cache/{pair_dir}/review.json")]);
	upgrade["source_refs"]["upstream_impacts"] =
		json!([format!(".agent/automations/radar/cache/{pair_dir}/impact.json")]);

	let records = [
		(root.join(format!("github/bundles/{run_id}.json")), bundle_raw),
		(root.join(&pair_dir).join("review.json"), review_raw),
		(root.join(&pair_dir).join("impact.json"), impact_raw),
		(root.join(format!("github/content-review-staging/{run_id}.json")), staging_raw),
		(root.join("github/control-plane-upgrades/current.json"), pretty_json_bytes(&upgrade)),
	];
	for (path, bytes) in &records {
		crate::write_private_file_atomic(path, bytes).expect("post-activation v2 state");
	}

	let readback = crate::reset_content_v2(&request).expect("marker readback should succeed");
	assert_eq!(readback.status, "already_active");
	assert_eq!(readback.collections_cleared, 0);
	assert_eq!(readback.files_removed, 0);
	assert_eq!(readback.directories_removed, 0);
	assert_eq!(readback.bytes_removed, 0);
	for (path, bytes) in records {
		assert_eq!(fs::read(&path).expect("preserved v2 record"), bytes, "{}", path.display());
	}
	assert!(root.join("content-v2-activation.json").is_file());
}

#[test]
fn reset_inventory_and_removal_reject_max_plus_one_entries() {
	let temp = crate::test_support::private_tempdir();
	let root = temp.path().join(crate::DEFAULT_CACHE_ROOT);
	let collection = Path::new("github/bundles");
	for index in 0..3 {
		crate::write_private_file_atomic(
			&root.join(collection).join(format!("bundle-{index}.json")),
			b"{}\n",
		)
		.expect("bounded bundle fixture");
	}
	let request = RadarContentV2ResetRequest { cache_root: root.clone() };
	let error = crate::content_activation::reset_content_v2_with_test_limits(&request, 3, 8, 1024)
		.expect_err("root plus three files must exceed an entry bound of three");
	assert!(error.to_string().contains("bounded entry limit"));
	assert_eq!(fs::read_dir(root.join(collection)).expect("bundle collection").count(), 3);

	let cache = crate::private_fs::PrivateCache::open_existing(&root).expect("private cache");
	let lock = cache.lock().expect("cache lock");
	let snapshot = lock
		.inspect_directory_tree(collection, 8, 8, 1024)
		.expect("tree inventory")
		.expect("collection should exist");
	let late = root.join(collection).join("late.json");
	fs::write(&late, b"{}\n").expect("late max-plus-one fixture");
	fs::set_permissions(&late, fs::Permissions::from_mode(0o600))
		.expect("late max-plus-one fixture mode");
	let error = lock
		.remove_directory_atomic_if_matches(collection, &snapshot)
		.expect_err("matched removal must bound and reject an extra entry");
	assert!(error.to_string().contains("bounded entry limit"));
	assert!(late.exists());
}

#[test]
fn fixed_reset_root_validation_rejects_an_outside_cache() {
	let root = crate::repo_root().expect("repository root");
	let expected = root.join(crate::DEFAULT_CACHE_ROOT);
	crate::content_activation::validate_reset_cache_root(&expected, &expected)
		.expect("exact reset root");
	let error = crate::content_activation::validate_reset_cache_root(
		&expected,
		&root.join("target/outside-cache"),
	)
	.expect_err("outside reset root must fail");
	assert!(error.to_string().contains("detected repository cache root"));
}
