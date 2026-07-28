use crate::tests::{assertions, fixtures};

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
