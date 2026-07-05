use crate::{
	orchestrator::{
		self, RetainedReviewRunIdentity, TERMINAL_GUARDED_RUN_STATUS,
		tests::{self, TEST_SERVICE_ID},
	},
	state::StateStore,
	tracker::{self, TrackerIssue},
};

fn candidate_selection_service_owned_issue(state_name: &str) -> TrackerIssue {
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);

	tests::sample_issue(state_name, &[active_label.as_str()])
}

#[test]
fn retained_closeout_identity_reuse_respects_attempt_history() {
	{
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = candidate_selection_service_owned_issue("In Review");
		let identity = RetainedReviewRunIdentity {
			run_id: String::from("pub-101-attempt-1-111"),
			attempt_number: 1,
		};

		assert!(
			orchestrator::retained_closeout_run_identity_is_reusable(
				&state_store,
				&issue.id,
				&identity,
			)
			.expect("missing attempts should be reusable for recovered closeout")
		);

		state_store
			.record_run_attempt(&identity.run_id, &issue.id, identity.attempt_number, "failed")
			.expect("failed attempt should record");

		assert!(
			!orchestrator::retained_closeout_run_identity_is_reusable(
				&state_store,
				&issue.id,
				&identity,
			)
			.expect("failed attempts should not be reused for closeout")
		);
		assert_eq!(
			state_store.next_attempt_number(&issue.id).expect("next attempt should calculate"),
			2,
			"actual failed closeout attempts should still allocate the next attempt"
		);
	}
	{
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = candidate_selection_service_owned_issue("In Review");
		let identity = RetainedReviewRunIdentity {
			run_id: String::from("pub-101-attempt-1-111"),
			attempt_number: 1,
		};

		state_store
			.record_run_attempt(&identity.run_id, &issue.id, identity.attempt_number, "succeeded")
			.expect("completed handoff attempt should record");
		state_store
			.record_run_attempt("pub-101-attempt-2-222", &issue.id, 2, "succeeded")
			.expect("later non-retry attempt should record");

		assert!(
			orchestrator::retained_closeout_run_identity_is_reusable(
				&state_store,
				&issue.id,
				&identity,
			)
			.expect("later non-retry attempts should not block handoff identity reuse")
		);
		assert_eq!(
			state_store.next_attempt_number(&issue.id).expect("next attempt should calculate"),
			3,
			"non-retry local history may still know about later attempts"
		);
	}

	for status in ["failed", "interrupted", TERMINAL_GUARDED_RUN_STATUS] {
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = candidate_selection_service_owned_issue("In Review");
		let identity = RetainedReviewRunIdentity {
			run_id: String::from("pub-101-attempt-1-111"),
			attempt_number: 1,
		};
		let retry_run_id = format!("pub-101-attempt-2-{status}");

		state_store
			.record_run_attempt(&identity.run_id, &issue.id, identity.attempt_number, "succeeded")
			.expect("completed handoff attempt should record");
		state_store
			.record_run_attempt(&retry_run_id, &issue.id, 2, status)
			.expect("later closeout retry should record");

		assert!(
			!orchestrator::retained_closeout_run_identity_is_reusable(
				&state_store,
				&issue.id,
				&identity,
			)
			.expect("later retry-budget attempts should block handoff identity reuse"),
			"later `{status}` closeout retry should block handoff identity reuse"
		);
		assert_eq!(
			state_store.next_attempt_number(&issue.id).expect("next attempt should calculate"),
			3,
			"real `{status}` closeout retries should keep incrementing"
		);
	}
}
