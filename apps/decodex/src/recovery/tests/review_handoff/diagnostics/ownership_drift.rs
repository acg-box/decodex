use crate::{
	recovery::{
		HandoffDiagnosticRequest, REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION,
		REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION, tests, tests::review_handoff,
	},
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker},
};

#[test]
fn diagnostic_bound_handoff_reports_missing_active_ownership_recovery() {
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let worktree = tests::sample_worktree(branch_name);
	let handoff = ReviewHandoffMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		head_oid,
	);
	let orchestration = ReviewOrchestrationMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		head_oid,
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);
	let landing_state = tests::sample_landing_state(pr_url, branch_name, head_oid);
	let diagnostic = review_handoff::diagnostic_binding(HandoffDiagnosticRequest {
		service_id: "pubfi",
		issue_identifier: "PUB-718",
		issue_state_name: "In Review",
		success_state: "In Review",
		in_progress_state: "In Progress",
		failure_state: "Todo",
		worktree: &worktree,
		existing_handoff: Some(&handoff),
		existing_orchestration: Some(&orchestration),
		local_branch_name: Some(branch_name),
		local_head_oid: Some(head_oid),
		worktree_clean: Some(true),
		pr_inspection: Some(&landing_state),
		active_label_present: Some(false),
	});

	assert_eq!(diagnostic.classification, REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION);
	assert_eq!(diagnostic.reason, "active_ownership_label_missing");
	assert_eq!(diagnostic.mismatched_field.as_deref(), Some("issue.labels"));
	assert!(diagnostic.next_action.contains("decodex:active:pubfi"));
	assert!(diagnostic.next_action.contains("Restore explicit lane ownership"));
}

#[test]
fn diagnostic_reports_rebind_for_failure_state_ownership_drift() {
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let worktree = tests::sample_worktree(branch_name);
	let handoff = ReviewHandoffMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		head_oid,
	);
	let orchestration = ReviewOrchestrationMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		head_oid,
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);
	let landing_state = tests::sample_landing_state(pr_url, branch_name, head_oid);
	let diagnostic = review_handoff::diagnostic_binding(HandoffDiagnosticRequest {
		service_id: "pubfi",
		issue_identifier: "PUB-718",
		issue_state_name: "Todo",
		success_state: "In Review",
		in_progress_state: "In Progress",
		failure_state: "Todo",
		worktree: &worktree,
		existing_handoff: Some(&handoff),
		existing_orchestration: Some(&orchestration),
		local_branch_name: Some(branch_name),
		local_head_oid: Some(head_oid),
		worktree_clean: Some(true),
		pr_inspection: Some(&landing_state),
		active_label_present: Some(false),
	});

	assert_eq!(diagnostic.classification, REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION);
	assert_eq!(diagnostic.reason, "active_ownership_label_missing");
	assert_eq!(diagnostic.mismatched_field.as_deref(), Some("issue.labels"));
	assert!(diagnostic.next_action.contains("rebind PUB-718"));
	assert!(diagnostic.next_action.contains("--dry-run"));
	assert!(!diagnostic.next_action.contains("Restore explicit lane ownership"));
}

#[test]
fn diagnostic_reports_rebind_for_failure_state_drift_with_active_label() {
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let worktree = tests::sample_worktree(branch_name);
	let handoff = ReviewHandoffMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		head_oid,
	);
	let orchestration = ReviewOrchestrationMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		head_oid,
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	);
	let landing_state = tests::sample_landing_state(pr_url, branch_name, head_oid);
	let diagnostic = review_handoff::diagnostic_binding(HandoffDiagnosticRequest {
		service_id: "pubfi",
		issue_identifier: "PUB-718",
		issue_state_name: "Todo",
		success_state: "In Review",
		in_progress_state: "In Progress",
		failure_state: "Todo",
		worktree: &worktree,
		existing_handoff: Some(&handoff),
		existing_orchestration: Some(&orchestration),
		local_branch_name: Some(branch_name),
		local_head_oid: Some(head_oid),
		worktree_clean: Some(true),
		pr_inspection: Some(&landing_state),
		active_label_present: Some(true),
	});

	assert_eq!(diagnostic.classification, REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION);
	assert_eq!(diagnostic.reason, "review_handoff_failure_state_drift");
	assert_eq!(diagnostic.mismatched_field.as_deref(), Some("issue.state"));
	assert!(diagnostic.next_action.contains("rebind PUB-718"));
	assert!(diagnostic.next_action.contains("--dry-run"));
}
