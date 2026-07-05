use tempfile::TempDir;

#[rustfmt::skip]
use crate::manual::{self, tests};

#[test]
fn manual_land_closeout_marker_roundtrips() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let checkout = tests::init_git_checkout(&temp_dir, "repo");

	manual::write_manual_land_closeout_marker(
		&checkout,
		"https://github.com/hack-ink/decodex/pull/67",
		"deadbeef",
		"xy-225",
		r#"{"schema":"decodex/commit/1"}"#,
	)
	.expect("closeout marker should write");

	assert!(
		manual::manual_land_closeout_matches(
			&checkout,
			"https://github.com/hack-ink/decodex/pull/67",
			"deadbeef",
			"xy-225",
			r#"{"schema":"decodex/commit/1"}"#,
		)
		.expect("closeout marker should read"),
	);

	let marker = manual::read_manual_land_closeout_marker(&checkout)
		.expect("closeout marker should parse")
		.expect("closeout marker should exist");

	assert_eq!(marker.landed_change.as_deref(), Some(r#"{"schema":"decodex/commit/1"}"#));
	assert!(
		!checkout.join(".decodex/manual-land-closeout").exists(),
		"closeout marker should live under git admin state, not the working tree"
	);
}
