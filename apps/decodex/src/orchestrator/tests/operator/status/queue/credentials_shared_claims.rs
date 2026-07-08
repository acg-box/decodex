use crate::orchestrator::tests::{
	operator::{
		status,
		status::{
			FakeTracker, HashMap, RUN_OPERATION_GIT_CREDENTIALS, StateStore, Value,
			WorktreeManager, orchestrator, state,
		},
	},
	recovery_terminal_support,
};

#[test]
fn live_operator_status_snapshot_surfaces_git_credential_failures() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = status::sample_issue_with_sort_fields(
		"issue-needs-attention",
		"PUB-105",
		"Todo",
		&["decodex:needs-attention"],
		Some(2),
		"2026-03-13T09:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);
	let tracker = FakeTracker::new(vec![issue.clone()]);

	state::write_run_operation_marker(
		&worktree_path,
		"run-missing-credentials",
		1,
		RUN_OPERATION_GIT_CREDENTIALS,
	)
	.expect("credential preflight marker should write");

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let candidate = snapshot
		.queued_candidates
		.iter()
		.find(|candidate| candidate.issue_identifier == "PUB-105")
		.expect("needs-attention queued issue should exist");
	let attention = candidate.attention.as_ref().expect("attention details should render");

	assert!(snapshot.current_lanes.is_empty());
	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "issue_needs_attention");
	assert_eq!(attention.current_operation.as_deref(), Some(state::RUN_OPERATION_GIT_CREDENTIALS));
	assert_eq!(attention.summary, "Git credential preflight failed; operator recovery required.");
}

#[test]
fn recovers_shared_claims_for_fresh_stores() {
	let workflow_markdown =
		status::sample_workflow_markdown("pubfi", &[], "Follow the repository policy.", 1);
	let (_temp_dir, config, workflow) =
		status::temp_project_layout_with_workflow_markdown(&workflow_markdown);
	let remote_store = StateStore::open_in_memory().expect("remote state store should open");
	let observer_store = StateStore::open_in_memory().expect("observer state store should open");
	let claimed_issue = status::sample_issue_with_sort_fields(
		"issue-claimed",
		"PUB-103",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T06:16:17.133Z",
	);
	let ready_issue = status::sample_issue_with_sort_fields(
		"issue-ready",
		"PUB-101",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![claimed_issue.clone(), ready_issue]);

	remote_store
		.configure_dispatch_slot_root(config.service_id(), config.worktree_root())
		.expect("remote store should configure dispatch-slot root");

	assert!(
		remote_store
			.try_acquire_lease(
				config.service_id(),
				&claimed_issue.id,
				"run-claimed",
				workflow.frontmatter().tracker().in_progress_state(),
			)
			.expect("remote store should acquire the shared issue claim")
	);

	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&observer_store,
		10,
	)
	.expect("snapshot should build");
	let queued_by_issue = snapshot
		.queued_candidates
		.iter()
		.map(|candidate| (candidate.issue_identifier.as_str(), candidate))
		.collect::<HashMap<_, _>>();

	assert_eq!(
		queued_by_issue.get("PUB-103").expect("claimed queued issue should exist").classification,
		"claimed"
	);
	assert_eq!(
		queued_by_issue.get("PUB-103").expect("claimed queued issue should exist").reason,
		"shared_claim_present"
	);
	assert!(
		snapshot.current_lanes.is_empty(),
		"fresh observer stores should not invent local running lanes while reconstructing the shared claim view"
	);
}

#[test]
fn reconstructs_shared_view_for_fresh_stores() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let active_issue = recovery_terminal_support::sample_active_issue("In Progress");
	let closed_issue = status::sample_issue_with_sort_fields(
		"issue-closed",
		"PUB-104",
		"Done",
		&[],
		Some(4),
		"2026-03-13T07:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![active_issue.clone(), closed_issue]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree = worktree_manager
		.ensure_worktree(&active_issue.identifier, false)
		.expect("retained worktree should exist");

	state::write_run_activity_marker(&worktree.path, "run-1", 1)
		.expect("activity marker should write");

	let build_view = |state_store: &StateStore| -> Value {
		let recovered = orchestrator::recover_runtime_state_from_tracker_and_worktrees(
			&tracker,
			&config,
			&workflow,
			state_store,
		)
		.expect("runtime recovery should succeed");

		orchestrator::hydrate_status_snapshot_state(&config, state_store, recovered)
			.expect("status hydration should succeed");

		let snapshot = orchestrator::build_live_operator_status_snapshot(
			&tracker,
			&config,
			&workflow,
			state_store,
			10,
		)
		.expect("snapshot should build");

		serde_json::json!({
			"current_lanes": snapshot.current_lanes.iter().map(|run| {
				serde_json::json!({
					"run_id": run.run_id,
					"issue_id": run.issue_id,
					"phase": run.phase,
					"current_operation": run.current_operation,
					"run_lease": run.run_lease,
					"branch_name": run.branch_name,
					"worktree_path": run.worktree_path,
				})
			}).collect::<Vec<_>>(),
			"queued_candidates": snapshot.queued_candidates,
			"worktrees": snapshot.worktrees,
			"post_review_lanes": snapshot.post_review_lanes,
		})
	};
	let first_store = StateStore::open_in_memory().expect("first state store should open");
	let second_store = StateStore::open_in_memory().expect("second state store should open");

	assert_eq!(build_view(&first_store), build_view(&second_store));
}
