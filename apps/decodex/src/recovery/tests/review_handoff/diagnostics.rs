use crate::{
	prelude::{Result, eyre},
	recovery::{
		HandoffDiagnosticRequest, REVIEW_HANDOFF_BOUND_CLASSIFICATION,
		REVIEW_HANDOFF_OWNERSHIP_DRIFT_CLASSIFICATION,
		REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION,
		tests::{self},
	},
	state::{ReviewHandoffMarker, ReviewOrchestrationMarker, StateStore},
};

#[test]
fn rebind_lifecycle_marker_write_failure_clears_partial_handoff_marker() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
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
	let error = super::write_review_lifecycle_markers_with_rollback(
		&state_store,
		"pubfi",
		"issue-id",
		&handoff,
		&orchestration,
		|| -> Result<()> { Err(eyre::eyre!("orchestration marker write failed")) },
	)
	.expect_err("orchestration write failure should be returned");

	assert!(error.to_string().contains("orchestration marker write failed"));
	assert!(
		state_store
			.review_lifecycle_record("pubfi", "issue-id", branch_name)
			.expect("lifecycle read should succeed")
			.is_none()
	);
	assert!(
		state_store
			.review_handoff_marker("pubfi", "issue-id", branch_name)
			.expect("handoff read should succeed")
			.is_none()
	);
}

#[test]
fn diagnostic_treats_descendant_handoff_head_as_bound() {
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let (temp_dir, original_head, current_head) = tests::temp_git_worktree(branch_name);
	let worktree = tests::sample_worktree_at(branch_name, temp_dir.path());
	let handoff = ReviewHandoffMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		original_head,
	);
	let landing_state = tests::sample_landing_state(pr_url, branch_name, &current_head);
	let diagnostic = super::diagnostic_binding(HandoffDiagnosticRequest {
		service_id: "pubfi",
		issue_identifier: "PUB-718",
		issue_state_name: "In Review",
		success_state: "In Review",
		in_progress_state: "In Progress",
		failure_state: "Todo",
		worktree: &worktree,
		existing_handoff: Some(&handoff),
		existing_orchestration: None,
		local_branch_name: Some(branch_name),
		local_head_oid: Some(&current_head),
		worktree_clean: Some(true),
		pr_inspection: Some(&landing_state),
		active_label_present: Some(true),
	});

	assert_eq!(diagnostic.classification, REVIEW_HANDOFF_BOUND_CLASSIFICATION);
	assert_eq!(diagnostic.reason, "review_handoff_record_present");
	assert_eq!(diagnostic.mismatched_field, None);
}

#[test]
fn diagnostic_requires_rebind_when_current_marker_state_transition_pending() {
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
	let diagnostic = super::diagnostic_binding(HandoffDiagnosticRequest {
		service_id: "pubfi",
		issue_identifier: "PUB-718",
		issue_state_name: "In Progress",
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
	assert_eq!(diagnostic.reason, "review_handoff_state_transition_pending");
	assert_eq!(diagnostic.mismatched_field.as_deref(), Some("issue.state"));
	assert!(diagnostic.next_action.contains("rebind PUB-718"));
	assert!(diagnostic.next_action.contains("pending issue-state transition"));
}

#[test]
fn diagnostic_requires_refresh_when_handoff_head_is_stale() {
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let (temp_dir, original_head, rebased_head) = tests::temp_rebased_git_worktree(branch_name);
	let worktree = tests::sample_worktree_at(branch_name, temp_dir.path());
	let handoff = ReviewHandoffMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		original_head,
	);
	let landing_state = tests::sample_landing_state(pr_url, branch_name, &rebased_head);
	let diagnostic = super::diagnostic_binding(HandoffDiagnosticRequest {
		service_id: "pubfi",
		issue_identifier: "PUB-718",
		issue_state_name: "In Review",
		success_state: "In Review",
		in_progress_state: "In Progress",
		failure_state: "Todo",
		worktree: &worktree,
		existing_handoff: Some(&handoff),
		existing_orchestration: None,
		local_branch_name: Some(branch_name),
		local_head_oid: Some(&rebased_head),
		worktree_clean: Some(true),
		pr_inspection: Some(&landing_state),
		active_label_present: Some(true),
	});

	assert_eq!(diagnostic.classification, REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION);
	assert_eq!(diagnostic.reason, "review_handoff_lineage_mismatch");
	assert_eq!(diagnostic.pr_head_oid.as_deref(), Some(rebased_head.as_str()));
	assert_eq!(diagnostic.mismatched_field.as_deref(), Some("review_handoff.pr_head_oid"));
	assert!(diagnostic.next_action.contains("rebind PUB-718"));
	assert!(diagnostic.next_action.contains("--dry-run"));
}

#[test]
fn diagnostic_requires_refresh_when_orchestration_head_is_stale() {
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
		"0123456789abcdef0123456789abcdef01234567",
		"waiting_for_result",
		None,
		None,
		None,
		0,
		0,
		None,
	);
	let landing_state = tests::sample_landing_state(pr_url, branch_name, head_oid);
	let diagnostic = super::diagnostic_binding(HandoffDiagnosticRequest {
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
		active_label_present: Some(true),
	});

	assert_eq!(diagnostic.classification, REVIEW_HANDOFF_REBIND_REQUIRED_CLASSIFICATION);
	assert_eq!(diagnostic.reason, "review_orchestration_head_mismatch");
	assert_eq!(diagnostic.mismatched_field.as_deref(), Some("review_orchestration.head_sha"));
}

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
	let diagnostic = super::diagnostic_binding(HandoffDiagnosticRequest {
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
	let diagnostic = super::diagnostic_binding(HandoffDiagnosticRequest {
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
	let diagnostic = super::diagnostic_binding(HandoffDiagnosticRequest {
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
