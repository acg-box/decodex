use crate::manual::{self, tests, tests::support::TestTracker};

#[test]
fn prepare_closeout_matches_authority_case_insensitively() {
	assert_eq!("xy-225".to_ascii_uppercase(), "XY-225");
}

#[test]
fn manual_closeout_scope_requires_service_ownership() {
	let issue = tests::sample_issue("issue-1", "XY-225", false, &[]);
	let error = manual::ensure_manual_closeout_issue_scope(&TestTracker::new(), &issue, "pubfi")
		.expect_err("service ownership should be required");

	assert!(error.to_string().contains("decodex:active:pubfi"));

	let issue = tests::sample_issue("issue-1", "XY-225", false, &[]);
	let tracker = TestTracker::new().with_label_issues("decodex:active:pubfi", vec![issue.clone()]);

	manual::ensure_manual_closeout_issue_scope(&tracker, &issue, "pubfi")
		.expect("server-confirmed service ownership should pass");
}

#[test]
fn manual_closeout_clear_removes_present_transient_decodex_labels() {
	for (case_name, labels, expected_label_ids) in [
		(
			"all transient labels present",
			&["decodex:active:pubfi", "decodex:queued:pubfi", "decodex:needs-attention"][..],
			&["team-label-0", "team-label-1", "team-label-2"][..],
		),
		("optional transient labels absent", &["decodex:active:pubfi"][..], &["team-label-0"][..]),
	] {
		let issue = tests::sample_issue("issue-1", "XY-225", true, labels);
		let tracker = TestTracker::new();

		manual::clear_manual_closeout_issue_scope(
			&tracker,
			&issue,
			"pubfi",
			"decodex:needs-attention",
		)
		.expect(case_name);

		let expected_removals = expected_label_ids
			.iter()
			.map(|label_id| vec![(*label_id).to_owned()])
			.collect::<Vec<_>>();

		assert_eq!(tracker.label_removals.borrow().as_slice(), expected_removals.as_slice());
	}
}

#[test]
fn manual_closeout_clear_classifies_label_removal_errors() {
	for (case_name, labels, message, expected_label_ids, expected_error) in [
		(
			"missing label removal is idempotent",
			&["decodex:active:pubfi", "decodex:queued:pubfi", "decodex:needs-attention"][..],
			"Linear GraphQL request failed: Label not on issue",
			&["team-label-0", "team-label-1", "team-label-2"][..],
			None,
		),
		(
			"other label removal errors are preserved",
			&["decodex:active:pubfi"][..],
			"Linear GraphQL request failed: Timeout",
			&["team-label-0"][..],
			Some("Timeout"),
		),
	] {
		let issue = tests::sample_issue("issue-1", "XY-225", true, labels);
		let tracker = TestTracker::new().with_label_removal_error(message);
		let result = manual::clear_manual_closeout_issue_scope(
			&tracker,
			&issue,
			"pubfi",
			"decodex:needs-attention",
		);

		if let Some(expected_error) = expected_error {
			let error = result.expect_err(case_name);

			assert!(error.to_string().contains(expected_error));
		} else {
			result.expect(case_name);
		}

		let expected_removals = expected_label_ids
			.iter()
			.map(|label_id| vec![(*label_id).to_owned()])
			.collect::<Vec<_>>();

		assert_eq!(tracker.label_removals.borrow().as_slice(), expected_removals.as_slice());
	}
}
