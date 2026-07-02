use crate::{
	orchestrator,
	orchestrator::tests::{self, FakeTracker, TEST_SERVICE_ID},
	state::StateStore,
	tracker,
};

#[test]
fn eligibility_uses_state_label_blocker_and_lease_rules() {
	let (_, _, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let eligible_issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![eligible_issue.clone()]);
	let opted_out_issue = tests::sample_issue("Todo", &["decodex:manual-only"]);
	let needs_attention_issue = tests::sample_issue("Todo", &["decodex:needs-attention"]);
	let mut blocked_issue = tests::sample_issue("Todo", &[]);

	blocked_issue.blockers = vec![tests::sample_blocker("issue-2", "PUB-102", "In Progress")];

	let mut unblocked_issue = tests::sample_issue("Todo", &[]);

	unblocked_issue.blockers = vec![tests::sample_blocker("issue-3", "PUB-103", "Done")];

	let wrong_state_issue = tests::sample_issue("In Progress", &[]);

	assert!(
		orchestrator::is_issue_eligible(
			&tracker,
			&eligible_issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("eligibility should succeed")
	);
	assert!(
		!orchestrator::is_issue_eligible(
			&tracker,
			&opted_out_issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("eligibility should succeed")
	);
	assert!(
		!orchestrator::is_issue_eligible(
			&tracker,
			&needs_attention_issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("eligibility should succeed")
	);
	assert!(
		!orchestrator::is_issue_eligible(
			&tracker,
			&blocked_issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("eligibility should succeed")
	);
	assert!(
		orchestrator::is_issue_eligible(
			&tracker,
			&unblocked_issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("eligibility should succeed")
	);
	assert!(
		!orchestrator::is_issue_eligible(
			&tracker,
			&wrong_state_issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("eligibility should succeed")
	);

	state_store
		.upsert_lease("pubfi", "issue-1", "run-1", "In Progress")
		.expect("lease should record");

	assert!(
		!orchestrator::is_issue_eligible(
			&tracker,
			&eligible_issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("eligibility should succeed")
	);
}

#[test]
fn claimed_issue_still_passes_post_claim_dispatch_policy() {
	let (_, _, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	state_store
		.try_acquire_lease("pubfi", &issue.id, "run-1", "In Progress")
		.expect("lease acquisition should succeed");

	assert!(
		orchestrator::issue_passes_dispatch_policy(
			&tracker,
			&issue,
			&workflow,
			&tracker::automation_queue_label(TEST_SERVICE_ID),
			false,
		)
		.expect("dispatch policy should succeed"),
		"post-claim policy should ignore the caller's own lease"
	);
	assert!(
		!orchestrator::is_issue_eligible(
			&tracker,
			&issue,
			TEST_SERVICE_ID,
			&workflow,
			&state_store,
		)
		.expect("pre-claim eligibility should still reject leased issues")
	);
}

#[test]
fn machine_only_fenced_descriptions_fail_normal_dispatch_policy() {
	let (_, _, workflow) = tests::temp_project_layout();
	let cases = [
		(
			"single json fence",
			"```json\n{\n  \"schema\": \"opaque-pointer/1\",\n  \"id\": \"ptr-1\"\n}\n```",
		),
		(
			"multiple json fences",
			"```json\n{\n  \"schema\": \"opaque-pointer/1\"\n}\n```\n\n```json\n{\n  \"schema\": \"opaque-pointer/2\"\n}\n```",
		),
		("four backtick json fence", "````json\n{\n  \"schema\": \"opaque-pointer/1\"\n}\n````"),
		("tilde json fence", "~~~json\n{\n  \"schema\": \"opaque-pointer/1\"\n}\n~~~"),
	];

	for (case_name, description) in cases {
		let mut issue = tests::sample_issue("Todo", &[]);

		issue.description = description.to_owned();

		let tracker = FakeTracker::new(vec![issue.clone()]);

		assert!(
			!orchestrator::issue_passes_dispatch_policy(
				&tracker,
				&issue,
				&workflow,
				&tracker::automation_queue_label(TEST_SERVICE_ID),
				false,
			)
			.expect("dispatch policy should succeed"),
			"normal dispatch should reject {case_name} without a human briefing surface"
		);
	}
}

#[test]
fn prose_plus_fenced_block_description_still_passes_normal_dispatch_policy() {
	let (_, _, workflow) = tests::temp_project_layout();
	let mut issue = tests::sample_issue("Todo", &[]);

	issue.description = String::from(
		"Implement the retained lane repair.\n\n```json\n{\n  \"schema\": \"opaque-pointer/1\"\n}\n```",
	);

	let tracker = FakeTracker::new(vec![issue.clone()]);

	assert!(
		orchestrator::issue_passes_dispatch_policy(
			&tracker,
			&issue,
			&workflow,
			&tracker::automation_queue_label(TEST_SERVICE_ID),
			false,
		)
		.expect("dispatch policy should succeed"),
		"dispatch should remain allowed when a generic briefing exists outside the fenced block"
	);
}

#[test]
fn truncated_label_pages_do_not_block_queue_label_dispatch() {
	let (_, _, workflow) = tests::temp_project_layout();
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let mut issue = tests::sample_issue("Todo", &[]);

	issue.labels_complete = false;

	issue.labels.retain(|label| label.name != queue_label.as_str());

	let tracker = FakeTracker::new(vec![issue.clone()])
		.with_label_lookup_issues(&queue_label, vec![issue.clone()]);

	assert!(
		orchestrator::issue_passes_dispatch_policy(
			&tracker,
			&issue,
			&workflow,
			&queue_label,
			false,
		)
		.expect("dispatch policy should succeed"),
		"server-filtered queue membership should remain authoritative when the local label page is truncated"
	);
}

#[test]
fn truncated_label_pages_block_dispatch_when_queue_label_was_removed() {
	let (_, _, workflow) = tests::temp_project_layout();
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let mut issue = tests::sample_issue("Todo", &[]);

	issue.labels_complete = false;

	issue.labels.retain(|label| label.name != queue_label.as_str());

	let tracker = FakeTracker::new(vec![]);

	assert!(
		!orchestrator::issue_passes_dispatch_policy(
			&tracker,
			&issue,
			&workflow,
			&queue_label,
			false,
		)
		.expect("dispatch policy should succeed"),
		"dispatch should re-check queue membership server-side when the local label page is truncated"
	);
}

#[test]
fn text_fenced_briefing_still_passes_normal_dispatch_policy() {
	let (_, _, workflow) = tests::temp_project_layout();
	let mut issue = tests::sample_issue("Todo", &[]);

	issue.description =
		String::from("```text\nImplement the retained lane repair and keep scope tight.\n```");

	let tracker = FakeTracker::new(vec![issue.clone()]);

	assert!(
		orchestrator::issue_passes_dispatch_policy(
			&tracker,
			&issue,
			&workflow,
			&tracker::automation_queue_label(TEST_SERVICE_ID),
			false,
		)
		.expect("dispatch policy should succeed"),
		"human-readable fenced text should still count as a generic briefing surface"
	);
}
