use crate::commit_message::{self};

#[test]
fn build_commit_message_omits_empty_related_and_false_breaking() {
	let message =
		commit_message::build_commit_message("tighten workflow defaults", "XY-225", &[], false)
			.expect("commit message should build");

	assert_eq!(
		message,
		r#"{"schema":"decodex/commit/2","change":"tighten workflow defaults","authority":"XY-225","impact":"compatible"}"#
	);
}

#[test]
fn build_commit_message_rejects_related_issues() {
	let message = commit_message::build_commit_message(
		"tighten workflow defaults",
		"XY-225",
		&[String::from("XY-12"), String::from("XY-99")],
		true,
	)
	.expect_err("related issues should be outside commit/2");

	assert!(message.to_string().contains("does not accept related"));
}

#[test]
fn build_landing_commit_message_normalizes_land_prefix() {
	for (summary, related, breaking, expected) in [
		(
			"tighten workflow defaults",
			Vec::new(),
			true,
			r#"{"schema":"decodex/commit/2","change":"Land tighten workflow defaults","authority":"XY-225","impact":"breaking"}"#,
		),
		(
			"Land tighten workflow defaults",
			Vec::new(),
			false,
			r#"{"schema":"decodex/commit/2","change":"Land tighten workflow defaults","authority":"XY-225","impact":"compatible"}"#,
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
		r#"{"schema":"decodex/commit/2","change":"ship hotfix outside tracker","authority":"manual","impact":"compatible"}"#
	);
}

#[test]
fn build_commit_message_accepts_baseline_authority() {
	let message = commit_message::build_commit_message(
		"normalize repo gate baseline",
		"baseline",
		&[],
		false,
	)
	.expect("baseline authority should build");

	assert_eq!(
		message,
		r#"{"schema":"decodex/commit/2","change":"normalize repo gate baseline","authority":"baseline","impact":"compatible"}"#
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
			r#"{"schema":"decodex/commit/2","change":"ship fix","authority":"XY-225","impact":"breaking"}"#,
			r#"{"schema":"decodex/commit/2","change":"Land ship fix","authority":"XY-225","impact":"breaking"}"#,
		),
		(
			r#"{"schema":"decodex/commit/2","change":"Land ship fix","authority":"XY-225","impact":"compatible"}"#,
			r#"{"schema":"decodex/commit/2","change":"Land ship fix","authority":"XY-225","impact":"compatible"}"#,
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
			r#"{"schema":"decodex/commit/2","change":"ship fix","authority":"XY-225","impact":"compatible"}"#,
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
		r#"{"schema":"decodex/commit/2","change":"ship fix","authority":"manual","impact":"compatible"}"#,
	)
	.expect("manual schema subject should validate");
	commit_message::validate_commit_message_subject(
		r#"{"schema":"decodex/commit/2","change":"normalize baseline","authority":"baseline","impact":"compatible"}"#,
	)
	.expect("baseline schema subject should validate");
}

#[test]
fn validate_commit_message_subject_rejects_non_schema_records() {
	for (message, expected) in [
		("ship fix", "expected value"),
		(
			r#"{"schema":"decodex/commit/2","change":"ship fix","authority":"manual","impact":"compatible","extra":true}"#,
			"unknown field",
		),
		(
			r#"{"schema":"decodex/commit/2","change":"ship fix","authority":"manual","impact":"unknown"}"#,
			"impact",
		),
		(
			r#"{"schema":"decodex/commit/2","change":"ship fix","authority":"manual","impact":"compatible","related":["XY-12"]}"#,
			"unknown field",
		),
		(
			r#"{"schema":"decodex/commit/2","change":"ship fix","authority":"not-an-issue","impact":"compatible"}"#,
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
