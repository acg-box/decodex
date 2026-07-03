use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, HashMap, StateStore, TEST_SERVICE_ID, WorktreeManager, orchestrator, tracker,
};

#[test]
fn live_operator_status_snapshot_includes_queued_candidates_with_dispatch_classification() {
	let workflow_markdown =
		status::sample_workflow_markdown("pubfi", &[], "Follow the repository policy.", 1);
	let (_temp_dir, config, workflow) =
		status::temp_project_layout_with_workflow_markdown(&workflow_markdown);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let ready_issue = status::sample_issue_with_sort_fields(
		"issue-ready",
		"PUB-101",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let mut blocked_issue = status::sample_issue_with_sort_fields(
		"issue-blocked",
		"PUB-102",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T05:16:17.133Z",
	);

	blocked_issue.description = String::from("```json\n{}\n```");

	let claimed_issue = status::sample_issue_with_sort_fields(
		"issue-claimed",
		"PUB-103",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T06:16:17.133Z",
	);
	let closed_issue = status::sample_issue_with_sort_fields(
		"issue-closed",
		"PUB-104",
		"Done",
		&[],
		Some(4),
		"2026-03-13T07:16:17.133Z",
	);
	let canceled_issue = status::sample_issue_with_sort_fields(
		"issue-canceled",
		"PUB-105",
		"Canceled",
		&[],
		Some(5),
		"2026-03-13T08:16:17.133Z",
	);

	state_store
		.upsert_lease(config.service_id(), &claimed_issue.id, "run-claimed", "In Progress")
		.expect("lease should record");

	let tracker = FakeTracker::new(vec![
		claimed_issue.clone(),
		blocked_issue.clone(),
		closed_issue.clone(),
		canceled_issue.clone(),
		ready_issue.clone(),
	]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");

	assert_eq!(snapshot.queued_candidates.len(), 3);

	let queued_by_issue = snapshot
		.queued_candidates
		.iter()
		.map(|candidate| (candidate.issue_identifier.as_str(), candidate))
		.collect::<HashMap<_, _>>();

	assert_eq!(
		queued_by_issue.get("PUB-101").expect("ready queued issue should exist").classification,
		"ready"
	);
	assert_eq!(
		queued_by_issue.get("PUB-101").expect("ready queued issue should exist").reason,
		"eligible_for_dispatch"
	);
	assert_eq!(
		queued_by_issue.get("PUB-102").expect("blocked queued issue should exist").classification,
		"blocked"
	);
	assert_eq!(
		queued_by_issue.get("PUB-102").expect("blocked queued issue should exist").reason,
		"missing_dispatch_briefing"
	);
	assert_eq!(
		queued_by_issue.get("PUB-103").expect("claimed queued issue should exist").classification,
		"claimed"
	);
	assert_eq!(
		queued_by_issue.get("PUB-103").expect("claimed queued issue should exist").reason,
		"shared_claim_present"
	);
	assert!(
		!queued_by_issue.contains_key("PUB-104"),
		"terminal queued echoes should not appear in operator intake candidates"
	);
	assert!(
		!queued_by_issue.contains_key("PUB-105"),
		"canceled queued echoes should not appear in operator intake candidates"
	);
}

#[test]
fn live_operator_status_snapshot_routes_retained_success_state_lane_out_of_queue() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = status::sample_issue_with_sort_fields(
		"issue-review",
		"PUB-106",
		"In Review",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let worktree_path = config.worktree_root().join(&issue.identifier);

	issue.blockers = vec![status::sample_blocker("issue-done", "PUB-105", "Done")];

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			"x/pubfi-pub-106",
			&worktree_path.display().to_string(),
		)
		.expect("retained review worktree should record");

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let project = snapshot.projects.first().expect("project summary should exist");
	let lane = snapshot
		.post_review_lanes
		.iter()
		.find(|lane| lane.issue_identifier == "PUB-106")
		.expect("retained success-state worktree should be owned by post-review status");

	assert!(
		snapshot.queued_candidates.iter().all(|candidate| candidate.issue_identifier != "PUB-106"),
		"post-review retained lanes must not also appear as queue intake blockers"
	);
	assert_eq!(lane.reason, "missing_review_handoff_record");
	assert_eq!(
		project.queued_candidate_count, 0,
		"post-review retained lanes must not inflate intake backlog"
	);
}

#[test]
fn live_operator_status_snapshot_blocks_ordinary_queue_for_retained_handoff_marker() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let active_label = tracker::automation_active_label(TEST_SERVICE_ID);
	let issue = status::sample_issue_with_sort_fields(
		"issue-review",
		"PUB-106",
		"In Progress",
		&[active_label.as_str()],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = status::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/174";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	status::seed_review_handoff_marker_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&status::sample_review_handoff_marker(&worktree.branch_name, pr_url, &head_oid),
	);

	let tracker = FakeTracker::new(vec![issue]);
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
		.find(|candidate| candidate.issue_identifier == "PUB-106")
		.expect("retained handoff queue candidate should stay visible");

	assert_eq!(candidate.classification, "blocked");
	assert_eq!(candidate.reason, "review_handoff_state_transition_pending");
}

#[test]
fn live_operator_status_snapshot_reports_only_open_tracker_blockers() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = status::sample_issue_with_sort_fields(
		"issue-blocked",
		"PUB-107",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);

	issue.blockers = vec![
		status::sample_blocker("issue-done", "PUB-106", "Done"),
		status::sample_blocker("issue-open", "PUB-105", "Todo"),
	];

	let tracker = FakeTracker::new(vec![issue]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");
	let candidate = snapshot.queued_candidates.first().expect("blocked queued issue should exist");

	assert_eq!(candidate.reason, "open_tracker_blockers");
	assert_eq!(candidate.blocker_identifiers, vec![String::from("PUB-105")]);
}
