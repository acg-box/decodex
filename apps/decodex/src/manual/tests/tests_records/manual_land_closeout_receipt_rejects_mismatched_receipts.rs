use tempfile::TempDir;

#[rustfmt::skip]
use crate::manual::{self, tests};

#[test]
fn manual_land_closeout_receipt_rejects_mismatched_receipts() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let checkout = tests::init_git_checkout(&temp_dir, "repo");

	manual::write_manual_land_closeout_receipt(
		&checkout,
		"https://github.com/hack-ink/decodex/pull/67",
		"deadbeef",
		"xy-225",
		r#"{"schema":"decodex/commit/2"}"#,
	)
	.expect("closeout marker should write");

	assert!(
		!manual::manual_land_closeout_receipt_matches(
			&checkout,
			"https://github.com/hack-ink/decodex/pull/67",
			"cafebabe",
			"xy-225",
			r#"{"schema":"decodex/commit/2"}"#,
		)
		.expect("closeout marker should compare"),
	);
}
