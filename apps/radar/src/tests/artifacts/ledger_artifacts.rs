use crate::tests::{assertions, fixtures};

#[test]
fn accepts_valid_radar_archive_manifest() {
	let manifest = fixtures::valid_radar_archive_manifest();

	assertions::assert_errors(&manifest, []);
}

#[test]
fn rejects_radar_archive_manifest_without_external_assets() {
	let mut manifest = fixtures::valid_radar_archive_manifest();

	manifest["retention_days"] = serde_json::json!(30);

	manifest.as_object_mut().expect("manifest should be object").remove("archive_asset");

	assertions::assert_errors(
		&manifest,
		["retention_days must be 21", "archive_asset must be an object"],
	);
}

#[test]
fn path_validation_accepts_historical_archive_retention_policy() {
	let mut manifest = fixtures::valid_radar_archive_manifest();

	manifest["created_at"] = serde_json::json!("2026-05-13T07:52:56Z");
	manifest["retention_days"] = serde_json::json!(28);

	assertions::assert_errors(&manifest, ["retention_days must be 21"]);
	assertions::assert_path_errors(
		".agent/automations/radar/cache/archive/index/2026-05-13-pre-2026-04-13.json",
		&manifest,
		[],
	);
}

#[test]
fn accepts_valid_release_delta_and_rejects_missing_default_pair() {
	let mut release_delta = fixtures::valid_release_delta();

	assertions::assert_errors(&release_delta, []);

	release_delta["comparisons"][0]["prerelease_tag_name"] =
		serde_json::json!("rust-v0.2.0-alpha.2");

	assertions::assert_errors(
		&release_delta,
		["comparisons must include the default stable/prerelease pair"],
	);
}

#[test]
fn accepts_valid_review_queue_and_rejects_duplicate_subject() {
	let mut queue = fixtures::valid_review_queue();

	assertions::assert_errors(&queue, []);

	queue["subjects"] =
		serde_json::json!([fixtures::valid_queue_subject(), fixtures::valid_queue_subject()]);
	queue["counts"]["subjects_queued"] = serde_json::json!(2);

	assertions::assert_errors(&queue, ["duplicates pr:22414"]);
}
