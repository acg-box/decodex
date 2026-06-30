use super::*;

#[test]
fn rebind_state_allows_missing_marker_partial_in_progress_handoff() {
	let workflow = sample_workflow();
	let issue = sample_issue("In Progress");
	let transition = super::super::validate_rebind_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&issue,
		super::super::RebindMode::RestoreMissingHandoff,
	)
	.expect("missing-marker rebind should recover partial in-progress handoff")
	.expect("partial in-progress handoff should transition to success state");

	assert_eq!(transition.state_name, "In Review");
	assert_eq!(transition.state_id, "state-review");
}

#[test]
fn rebind_state_allows_current_marker_partial_in_progress_handoff() {
	let workflow = sample_workflow();
	let issue = sample_issue("In Progress");
	let transition = super::super::validate_rebind_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&issue,
		super::super::RebindMode::CompleteExistingHandoffState,
	)
	.expect("current-marker state completion should recover partial in-progress handoff")
	.expect("partial in-progress handoff should transition to success state");

	assert_eq!(transition.state_name, "In Review");
	assert_eq!(transition.state_id, "state-review");
}

#[test]
fn rebind_state_allows_current_marker_failure_state_drift_recovery() {
	let workflow = sample_workflow();
	let issue = sample_issue("Todo");
	let transition = super::super::validate_rebind_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&issue,
		super::super::RebindMode::CompleteExistingHandoffState,
	)
	.expect("current-marker state completion should recover failure-state drift")
	.expect("failure-state drift should transition to success state");

	assert_eq!(transition.state_name, "In Review");
	assert_eq!(transition.state_id, "state-review");
}

#[test]
fn rebind_state_rejects_failure_state_without_current_marker_repair_mode() {
	let workflow = sample_workflow();
	let issue = sample_issue("Todo");

	for mode in [
		super::super::RebindMode::RestoreMissingHandoff,
		super::super::RebindMode::RefreshExistingHandoff,
	] {
		let error = super::super::validate_rebind_issue_state_for_policy(
			workflow.frontmatter().tracker(),
			&issue,
			mode,
		)
		.expect_err("failure-state repair requires current-marker completion mode");

		assert!(
			error.to_string().contains("review handoff rebind requires"),
			"unexpected error for {mode:?}: {error}"
		);
	}
}

#[test]
fn rebind_state_requires_success_state_for_existing_marker_refresh() {
	let workflow = sample_workflow();
	let issue = sample_issue("In Progress");
	let error = super::super::validate_rebind_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&issue,
		super::super::RebindMode::RefreshExistingHandoff,
	)
	.expect_err("existing-marker refresh should still require success state");

	assert!(error.to_string().contains("requires `In Review`"));
	assert!(!error.to_string().contains("partial missing-marker"));
}

#[test]
fn adopt_state_allows_in_progress_or_review_only() {
	let workflow = sample_workflow();
	let in_progress = sample_issue("In Progress");
	let transition = super::super::validate_adopt_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&in_progress,
	)
	.expect("in-progress issue should be adoptable")
	.expect("in-progress issue should transition to review");

	assert_eq!(transition.state_name, "In Review");
	assert_eq!(transition.state_id, "state-review");

	let in_review = sample_issue("In Review");
	let no_transition = super::super::validate_adopt_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&in_review,
	)
	.expect("in-review issue should remain adoptable");

	assert!(no_transition.is_none());

	let todo = sample_issue("Todo");
	let error = super::super::validate_adopt_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&todo,
	)
	.expect_err("manual takeover should not bypass failure/start states");

	assert!(error.to_string().contains("manual takeover adopt requires"));
}

#[test]
fn adopt_landing_state_rejects_pending_checks() {
	let mut landing_state = sample_landing_state(
		"https://github.com/hack-ink/decodex/pull/344",
		"xy/xy-944-manual-takeover-adopt",
		"1123456789abcdef0123456789abcdef01234567",
	);

	landing_state.status_check_rollup_state = Some(String::from("PENDING"));

	let error = super::super::validate_adopt_landing_state(&landing_state)
		.expect_err("manual takeover must not adopt pending checks");

	assert!(error.to_string().contains("still waiting on checks"));
}

#[test]
fn adopt_landing_state_rejects_blocked_merge_state_after_green_gates() {
	let mut landing_state = sample_landing_state(
		"https://github.com/hack-ink/decodex/pull/344",
		"xy/xy-944-manual-takeover-adopt",
		"1123456789abcdef0123456789abcdef01234567",
	);

	landing_state.merge_state_status = String::from("BLOCKED");

	let error = super::super::validate_adopt_landing_state(&landing_state)
		.expect_err("manual takeover should not bypass blocked merge state");

	assert!(error.to_string().contains("not ready to adopt"));
	assert!(error.to_string().contains("mergeStateStatus=`BLOCKED`"));
}

#[test]
fn adopt_landing_state_rejects_closed_or_draft_prs() {
	let mut closed = sample_landing_state(
		"https://github.com/hack-ink/decodex/pull/344",
		"xy/xy-944-manual-takeover-adopt",
		"1123456789abcdef0123456789abcdef01234567",
	);

	closed.state = String::from("CLOSED");

	let error = super::super::validate_adopt_landing_state(&closed)
		.expect_err("manual takeover must reject closed PRs");

	assert!(error.to_string().contains("adopt requires `OPEN`"));

	let mut draft = sample_landing_state(
		"https://github.com/hack-ink/decodex/pull/344",
		"xy/xy-944-manual-takeover-adopt",
		"1123456789abcdef0123456789abcdef01234567",
	);

	draft.is_draft = true;

	let error = super::super::validate_adopt_landing_state(&draft)
		.expect_err("manual takeover must reject draft PRs");

	assert!(error.to_string().contains("is still draft"));
}

#[test]
fn adopt_landing_state_rejects_failed_required_checks() {
	let mut landing_state = sample_landing_state(
		"https://github.com/hack-ink/decodex/pull/344",
		"xy/xy-944-manual-takeover-adopt",
		"1123456789abcdef0123456789abcdef01234567",
	);

	landing_state.status_check_rollup_state = Some(String::from("FAILURE"));
	landing_state.merge_state_status = String::from("BLOCKED");

	let error = super::super::validate_adopt_landing_state(&landing_state)
		.expect_err("manual takeover must reject failed required checks");

	assert!(error.to_string().contains("failed required checks"));
}

#[test]
fn adopt_existing_worktree_mapping_accepts_same_project_and_path() {
	let temp_dir = TempDir::new().expect("temp worktree should exist");
	let branch_name = "x/pubfi-pub-718";
	let issue = sample_issue("In Progress");
	let mapping = sample_worktree_at(branch_name, temp_dir.path());
	let canonical_worktree =
		fs::canonicalize(temp_dir.path()).expect("temp worktree should canonicalize");

	super::super::validate_adopt_existing_worktree_mapping(
		"pubfi",
		&issue,
		&mapping,
		&canonical_worktree,
	)
	.expect("matching mapping should be accepted");
}

#[test]
fn adopt_existing_worktree_mapping_accepts_stale_branch_for_same_path() {
	let retained_dir = TempDir::new().expect("retained worktree should exist");
	let issue = sample_issue("In Progress");
	let mapping = sample_worktree_at("x/pubfi-pub-718-old", retained_dir.path());
	let retained_worktree =
		fs::canonicalize(retained_dir.path()).expect("retained worktree should canonicalize");

	super::super::validate_adopt_existing_worktree_mapping(
		"pubfi",
		&issue,
		&mapping,
		&retained_worktree,
	)
	.expect("stale mapping branch should be adopted when path matches");
}

#[test]
fn adopt_existing_worktree_mapping_rejects_stale_path() {
	let retained_dir = TempDir::new().expect("retained worktree should exist");
	let current_dir = TempDir::new().expect("current worktree should exist");
	let issue = sample_issue("In Progress");
	let mapping = sample_worktree_at("x/pubfi-pub-718", retained_dir.path());
	let current_worktree =
		fs::canonicalize(current_dir.path()).expect("current worktree should canonicalize");
	let error = super::super::validate_adopt_existing_worktree_mapping(
		"pubfi",
		&issue,
		&mapping,
		&current_worktree,
	)
	.expect_err("stale mapping path must be rejected");

	assert!(error.to_string().contains("already has a retained worktree mapping at"));
}

#[test]
fn manual_adopt_run_id_is_stable_for_head() {
	let head_oid = "0123456789abcdef0123456789abcdef01234567";
	let run_id = super::super::manual_adopt_run_id("XY-944", 2, head_oid);

	assert_eq!(run_id, "xy-944-manual-adopt-2-0123456789ab");
	assert_eq!(run_id, super::super::manual_adopt_run_id("XY-944", 2, head_oid));
}

#[test]
fn adopt_private_event_records_manual_takeover_lifecycle_evidence() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let validation = super::super::AdoptValidation {
		issue: sample_issue("In Review"),
		branch_name: branch_name.to_owned(),
		worktree_path: Path::new("/tmp/PUB-718").to_path_buf(),
		run_id: String::from("pub-718-manual-adopt-2-1123456789ab"),
		attempt_number: 2,
		landing_state: sample_landing_state(pr_url, branch_name, head_oid),
		local_head_oid: head_oid.to_owned(),
		worktree_path_for_event: Some(String::from(".worktrees/PUB-718")),
		active_label_present: false,
		success_state_transition: None,
		previous_worktree_mapping: None,
	};

	super::super::append_review_handoff_adopt_private_event(
		&state_store,
		"pubfi",
		&validation,
		"local_markers_written",
		false,
	)
	.expect("adopt private event should append");
	super::super::append_review_handoff_adopt_private_event(
		&state_store,
		"pubfi",
		&validation,
		"active_label_checked",
		true,
	)
	.expect("adopt active-label private event should append");

	let events = state_store
		.list_private_execution_events(
			"pubfi",
			&validation.issue.id,
			&validation.run_id,
			validation.attempt_number,
		)
		.expect("private events should read");
	let event = events.first().expect("adopt event should exist");
	let payload = event.payload();
	let second_event = events.get(1).expect("active-label adopt event should exist");
	let second_payload = second_event.payload();

	assert_eq!(events.len(), 2);
	assert_eq!(event.event_type(), REVIEW_HANDOFF_ADOPT_EVENT);
	assert_eq!(payload["schema"], "decodex.review_handoff_recovery_private_event/1");
	assert_eq!(payload["event"], REVIEW_HANDOFF_ADOPT_EVENT);
	assert_eq!(payload["writeback_stage"], "local_markers_written");
	assert_eq!(payload["manual_takeover_adopt"], true);
	assert_eq!(payload["active_label_restored"], false);
	assert_eq!(payload["pr_url"], pr_url);
	assert_eq!(payload["pr_head_sha"], head_oid);
	assert_eq!(payload["next_action"], "continue retained post-review lifecycle");
	assert_eq!(second_event.event_type(), REVIEW_HANDOFF_ADOPT_EVENT);
	assert_eq!(second_payload["writeback_stage"], "active_label_checked");
	assert_eq!(second_payload["active_label_restored"], true);
}

#[test]
fn rebind_private_event_records_retained_lifecycle_evidence() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let validation = super::super::RebindValidation {
		issue: sample_issue("In Review"),
		worktree: sample_worktree(branch_name),
		run_id: String::from("pub-718-attempt-2-1123456789ab"),
		attempt_number: 2,
		landing_state: sample_landing_state(pr_url, branch_name, head_oid),
		local_head_oid: head_oid.to_owned(),
		worktree_path_for_event: Some(String::from(".worktrees/PUB-718")),
		active_label_present: true,
		restore_active_label: false,
		mode: super::super::RebindMode::RefreshExistingHandoff,
		success_state_transition: None,
		clear_needs_attention_label: false,
	};

	super::super::append_review_handoff_rebind_private_event(
		&state_store,
		"pubfi",
		&validation,
		"local_markers_written",
		false,
	)
	.expect("rebind private event should append");

	let events = state_store
		.list_private_execution_events(
			"pubfi",
			&validation.issue.id,
			&validation.run_id,
			validation.attempt_number,
		)
		.expect("private events should read");
	let event = events.first().expect("rebind event should exist");
	let payload = event.payload();

	assert_eq!(events.len(), 1);
	assert_eq!(event.event_type(), REVIEW_HANDOFF_REBIND_EVENT);
	assert_eq!(payload["schema"], "decodex.review_handoff_recovery_private_event/1");
	assert_eq!(payload["event"], REVIEW_HANDOFF_REBIND_EVENT);
	assert_eq!(payload["writeback_stage"], "local_markers_written");
	assert_eq!(payload["mode"], "refresh_existing_handoff");
	assert_eq!(payload["active_label_present"], true);
	assert_eq!(payload["active_label_restored"], false);
	assert_eq!(payload["pr_url"], pr_url);
	assert_eq!(payload["pr_head_sha"], head_oid);
	assert_eq!(payload["next_action"], "continue retained post-review lifecycle");
}

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

	let error = super::super::write_review_lifecycle_markers_with_rollback(
		&state_store,
		"pubfi",
		"issue-id",
		&handoff,
		&orchestration,
		|| -> crate::prelude::Result<()> {
			Err(crate::prelude::eyre::eyre!("orchestration marker write failed"))
		},
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
	let (temp_dir, original_head, current_head) = temp_git_worktree(branch_name);
	let worktree = sample_worktree_at(branch_name, temp_dir.path());
	let handoff = ReviewHandoffMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		original_head,
	);
	let landing_state = sample_landing_state(pr_url, branch_name, &current_head);
	let diagnostic = super::super::diagnostic_binding(super::super::HandoffDiagnosticRequest {
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
	let worktree = sample_worktree(branch_name);
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
	let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
	let diagnostic = super::super::diagnostic_binding(super::super::HandoffDiagnosticRequest {
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
	let (temp_dir, original_head, rebased_head) = temp_rebased_git_worktree(branch_name);
	let worktree = sample_worktree_at(branch_name, temp_dir.path());
	let handoff = ReviewHandoffMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		original_head,
	);
	let landing_state = sample_landing_state(pr_url, branch_name, &rebased_head);
	let diagnostic = super::super::diagnostic_binding(super::super::HandoffDiagnosticRequest {
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
	let worktree = sample_worktree(branch_name);
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
	let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
	let diagnostic = super::super::diagnostic_binding(super::super::HandoffDiagnosticRequest {
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
	let worktree = sample_worktree(branch_name);
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
	let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
	let diagnostic = super::super::diagnostic_binding(super::super::HandoffDiagnosticRequest {
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
	let worktree = sample_worktree(branch_name);
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
	let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
	let diagnostic = super::super::diagnostic_binding(super::super::HandoffDiagnosticRequest {
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
	let worktree = sample_worktree(branch_name);
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
	let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
	let diagnostic = super::super::diagnostic_binding(super::super::HandoffDiagnosticRequest {
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

#[test]
fn rebind_validation_refreshes_existing_same_branch_pr_marker() {
	let workflow = sample_workflow();
	let issue = sample_issue("In Review");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let worktree = sample_worktree(branch_name);
	let handoff = ReviewHandoffMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		"0123456789abcdef0123456789abcdef01234567",
	);
	let landing_state =
		sample_landing_state(pr_url, branch_name, "1123456789abcdef0123456789abcdef01234567");
	let (run_id, attempt_number, mode) = super::super::validate_existing_handoff_refresh(
		workflow.frontmatter().tracker(),
		&issue,
		&worktree,
		&handoff,
		None,
		&landing_state,
		"1123456789abcdef0123456789abcdef01234567",
	)
	.expect("stale existing marker should be refreshable");

	assert_eq!(run_id, "pub-718-attempt-1");
	assert_eq!(attempt_number, 1);
	assert_eq!(mode, super::super::RebindMode::RefreshExistingHandoff);
}

#[test]
fn rebind_validation_rejects_stale_marker_failure_state_drift_recovery() {
	let workflow = sample_workflow();
	let issue = sample_issue("Todo");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let worktree = sample_worktree(branch_name);
	let handoff = ReviewHandoffMarker::new(
		"pub-718-attempt-1",
		1,
		branch_name,
		pr_url,
		"main",
		branch_name,
		"0123456789abcdef0123456789abcdef01234567",
	);
	let landing_state =
		sample_landing_state(pr_url, branch_name, "1123456789abcdef0123456789abcdef01234567");
	let (_run_id, _attempt_number, mode) = super::super::validate_existing_handoff_refresh(
		workflow.frontmatter().tracker(),
		&issue,
		&worktree,
		&handoff,
		None,
		&landing_state,
		"1123456789abcdef0123456789abcdef01234567",
	)
	.expect("stale existing marker should require marker refresh first");

	assert_eq!(mode, super::super::RebindMode::RefreshExistingHandoff);

	let error = super::super::validate_rebind_issue_state_for_policy(
		workflow.frontmatter().tracker(),
		&issue,
		mode,
	)
	.expect_err("stale marker refresh must not repair failure-state drift");

	assert!(error.to_string().contains("review handoff rebind requires"));
}

#[test]
fn rebind_validation_rejects_current_existing_marker_as_noop() {
	let workflow = sample_workflow();
	let issue = sample_issue("In Review");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let worktree = sample_worktree(branch_name);
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
	let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
	let error = super::super::validate_existing_handoff_refresh(
		workflow.frontmatter().tracker(),
		&issue,
		&worktree,
		&handoff,
		Some(&orchestration),
		&landing_state,
		head_oid,
	)
	.expect_err("current existing marker should not be rebound");

	assert!(error.to_string().contains("no rebind is needed"));
}

#[test]
fn rebind_validation_completes_current_existing_marker_state_transition() {
	let workflow = sample_workflow();
	let issue = sample_issue("In Progress");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let worktree = sample_worktree(branch_name);
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
	let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
	let (run_id, attempt_number, mode) = super::super::validate_existing_handoff_refresh(
		workflow.frontmatter().tracker(),
		&issue,
		&worktree,
		&handoff,
		Some(&orchestration),
		&landing_state,
		head_oid,
	)
	.expect("current marker should allow state-only handoff completion");

	assert_eq!(run_id, "pub-718-attempt-1");
	assert_eq!(attempt_number, 1);
	assert_eq!(mode, super::super::RebindMode::CompleteExistingHandoffState);
}

#[test]
fn rebind_validation_completes_current_existing_marker_failure_state_drift() {
	let workflow = sample_workflow();
	let issue = sample_issue("Todo");
	let branch_name = "x/pubfi-pub-718";
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let head_oid = "1123456789abcdef0123456789abcdef01234567";
	let worktree = sample_worktree(branch_name);
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
	let landing_state = sample_landing_state(pr_url, branch_name, head_oid);
	let (run_id, attempt_number, mode) = super::super::validate_existing_handoff_refresh(
		workflow.frontmatter().tracker(),
		&issue,
		&worktree,
		&handoff,
		Some(&orchestration),
		&landing_state,
		head_oid,
	)
	.expect("current marker should allow failure-state drift completion");

	assert_eq!(run_id, "pub-718-attempt-1");
	assert_eq!(attempt_number, 1);
	assert_eq!(mode, super::super::RebindMode::CompleteExistingHandoffState);
}
