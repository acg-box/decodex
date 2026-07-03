use crate::commit_message::{self};

#[test]
fn build_commit_message_omits_empty_related_and_false_breaking() {
	let message =
		commit_message::build_commit_message("tighten workflow defaults", "XY-225", &[], false)
			.expect("commit message should build");

	assert_eq!(
		message,
		r#"{"schema":"decodex/commit/1","summary":"tighten workflow defaults","authority":"XY-225"}"#
	);
}

#[test]
fn build_commit_message_includes_optional_fields() {
	let message = commit_message::build_commit_message(
		"tighten workflow defaults",
		"XY-225",
		&[String::from("XY-12"), String::from("XY-99")],
		true,
	)
	.expect("commit message should build");

	assert_eq!(
		message,
		r#"{"schema":"decodex/commit/1","summary":"tighten workflow defaults","authority":"XY-225","related":["XY-12","XY-99"],"breaking":true}"#
	);
}

#[test]
fn build_landing_commit_message_normalizes_land_prefix() {
	for (summary, related, breaking, expected) in [
		(
			"tighten workflow defaults",
			vec![String::from("XY-12")],
			true,
			r#"{"schema":"decodex/commit/1","summary":"Land tighten workflow defaults","authority":"XY-225","related":["XY-12"],"breaking":true}"#,
		),
		(
			"Land tighten workflow defaults",
			Vec::new(),
			false,
			r#"{"schema":"decodex/commit/1","summary":"Land tighten workflow defaults","authority":"XY-225"}"#,
		),
	] {
		let message =
			commit_message::build_landing_commit_message(summary, "XY-225", &related, breaking)
				.expect("landing commit message should build");

		assert_eq!(message, expected);
	}
}

#[test]
fn build_commit_message_rejects_multiline_summary() {
	let error = commit_message::build_commit_message("one\ntwo", "XY-225", &[], false)
		.expect_err("multiline summary should fail");

	assert!(error.to_string().contains("must stay on one line"));
}

#[test]
fn build_commit_message_accepts_manual_authority() {
	let message =
		commit_message::build_commit_message("ship hotfix outside tracker", "manual", &[], false)
			.expect("manual authority should build");

	assert_eq!(
		message,
		r#"{"schema":"decodex/commit/1","summary":"ship hotfix outside tracker","authority":"manual"}"#
	);
}

#[test]
fn looks_like_issue_identifier_requires_suffix_number() {
	assert!(commit_message::looks_like_issue_identifier("XY-225"));
	assert!(commit_message::looks_like_issue_identifier("A1-9"));
	assert!(!commit_message::looks_like_issue_identifier("XY"));
	assert!(!commit_message::looks_like_issue_identifier("XY-"));
	assert!(!commit_message::looks_like_issue_identifier("-123"));
	assert!(!commit_message::looks_like_issue_identifier("XY-12A"));
}

#[test]
fn build_landed_merge_commit_message_normalizes_land_prefix() {
	for (head_message, expected) in [
		(
			r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"XY-225","related":["XY-12"],"breaking":true}"#,
			r#"{"schema":"decodex/commit/1","summary":"Land ship fix","authority":"XY-225","related":["XY-12"],"breaking":true}"#,
		),
		(
			r#"{"schema":"decodex/commit/1","summary":"Land ship fix","authority":"XY-225"}"#,
			r#"{"schema":"decodex/commit/1","summary":"Land ship fix","authority":"XY-225"}"#,
		),
	] {
		let landed_message =
			commit_message::build_landed_merge_commit_message(head_message, "XY-225")
				.expect("landed merge message should build");

		assert_eq!(landed_message, expected);
	}
}

#[test]
fn build_landed_merge_commit_message_rejects_invalid_head_subjects() {
	for (head_message, authority, expected) in [
		(
			r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"XY-225"}"#,
			"XY-226",
			"does not match expected authority",
		),
		("ship fix", "XY-225", "expected value"),
	] {
		let error = commit_message::build_landed_merge_commit_message(head_message, authority)
			.expect_err("invalid landed head subject should fail");

		assert!(error.to_string().contains(expected));
	}
}

#[test]
fn validate_commit_message_subject_accepts_schema_record_without_expected_authority() {
	commit_message::validate_commit_message_subject(
		r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"manual"}"#,
	)
	.expect("manual schema subject should validate");
}

#[test]
fn validate_commit_message_subject_rejects_non_schema_records() {
	for (message, expected) in [
		("ship fix", "expected value"),
		(
			r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"manual","extra":true}"#,
			"unknown field",
		),
		(
			r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"manual","related":["not-an-issue"]}"#,
			"issue identifier",
		),
		(
			r#"{"schema":"decodex/commit/1","summary":"ship fix","authority":"not-an-issue"}"#,
			"authority",
		),
	] {
		let error = commit_message::validate_commit_message_subject(message)
			.expect_err("invalid schema subject should fail");

		assert!(
			error.to_string().contains(expected),
			"`{message}` failed with unexpected error: {error}"
		);
	}
}
