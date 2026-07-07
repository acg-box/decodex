use crate::{
	orchestrator::{
		self,
		tests::{self, FakeTracker, TEST_SERVICE_ID},
	},
	state::StateStore,
	tracker,
	worktree::WorktreeManager,
};

#[test]
fn candidate_selection_allows_dispatch_when_another_issue_has_active_lease() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_lease("pubfi", "issue-active", "run-1", "In Progress")
		.expect("lease should record");

	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![issue],
		&workflow,
		&state_store,
		"pubfi",
	)
	.expect("candidate selection should succeed");

	assert!(
		selected.is_some(),
		"another active lease must not impose a project-level dispatch cap"
	);
}

#[test]
fn candidate_selection_blocks_ordinary_dispatch_for_retained_review_lifecycle_authority() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("Todo", &[]);
	let worktree_manager =
		WorktreeManager::new(config.service_id(), config.repo_root(), config.worktree_root());
	let worktree =
		worktree_manager.ensure_worktree(&issue.identifier, false).expect("worktree should exist");
	let head_oid = tests::git_output(&worktree.path, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/174";

	state_store
		.upsert_worktree(
			config.service_id(),
			&issue.id,
			&worktree.branch_name,
			&worktree.path.display().to_string(),
		)
		.expect("worktree should record");

	tests::seed_review_lifecycle_handoff_fixture_value(
		&state_store,
		config.service_id(),
		&issue.id,
		&tests::sample_review_lifecycle_handoff_fixture(&worktree.branch_name, pr_url, &head_oid),
	);

	let tracker = FakeTracker::new(vec![issue.clone()]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![issue],
		&workflow,
		&state_store,
		config.service_id(),
	)
	.expect("candidate selection should succeed");

	assert!(
		selected.is_none(),
		"ordinary intake must not mint a duplicate attempt for a retained review handoff lane"
	);
}

#[test]
fn candidate_selection_breaks_ties_by_identifier_after_created_at() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let later_identifier = tests::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:16:17.133Z",
	);
	let earlier_identifier = tests::sample_issue_with_sort_fields(
		"issue-3",
		"PUB-101",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker = FakeTracker::new(vec![later_identifier.clone(), earlier_identifier.clone()]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![later_identifier, earlier_identifier],
		&workflow,
		&state_store,
		"pubfi",
	)
	.expect("candidate selection should succeed")
	.expect("one issue should be selected");

	assert_eq!(selected.identifier, "PUB-101");
}

#[test]
fn candidate_selection_does_not_requery_queue_label_for_truncated_candidates() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let queue_label = tracker::automation_queue_label(TEST_SERVICE_ID);
	let issue = tests::sample_issue("Todo", &[]);
	let tracker = FakeTracker::new(vec![issue.clone()])
		.with_label_lookup_issues(&queue_label, vec![issue.clone()]);
	let mut truncated_issue = issue.clone();

	truncated_issue.labels_complete = false;

	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![truncated_issue],
		&workflow,
		&state_store,
		TEST_SERVICE_ID,
	)
	.expect("candidate selection should succeed")
	.expect("queue candidate should remain selectable");

	assert_eq!(selected.identifier, issue.identifier);
	assert!(tracker.label_queries.borrow().is_empty());
}

#[test]
fn candidate_selection_skips_todo_issue_with_nonterminal_blockers() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut blocked_high_priority = tests::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:15:17.133Z",
	);

	blocked_high_priority.blockers =
		vec![tests::sample_blocker("issue-9", "PUB-109", "In Progress")];

	let unblocked_lower_priority = tests::sample_issue_with_sort_fields(
		"issue-3",
		"PUB-103",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:16:17.133Z",
	);
	let tracker =
		FakeTracker::new(vec![blocked_high_priority.clone(), unblocked_lower_priority.clone()]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![blocked_high_priority, unblocked_lower_priority],
		&workflow,
		&state_store,
		"pubfi",
	)
	.expect("candidate selection should succeed")
	.expect("one issue should be selected");

	assert_eq!(selected.identifier, "PUB-103");
}

#[test]
fn candidate_selection_sorts_by_priority_created_at_and_identifier() {
	let (_temp_dir, _config, workflow) = tests::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let high_priority = tests::sample_issue_with_sort_fields(
		"issue-2",
		"PUB-102",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:18:17.133Z",
	);
	let oldest_same_priority = tests::sample_issue_with_sort_fields(
		"issue-3",
		"PUB-103",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:15:17.133Z",
	);
	let newest_same_priority = tests::sample_issue_with_sort_fields(
		"issue-4",
		"PUB-104",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T04:19:17.133Z",
	);
	let no_priority = tests::sample_issue_with_sort_fields(
		"issue-5",
		"PUB-105",
		"Todo",
		&[],
		None,
		"2026-03-13T04:14:17.133Z",
	);
	let tracker = FakeTracker::new(vec![
		no_priority.clone(),
		newest_same_priority.clone(),
		oldest_same_priority.clone(),
		high_priority.clone(),
	]);
	let selected = orchestrator::select_issue_candidate(
		&tracker,
		vec![no_priority, newest_same_priority, oldest_same_priority, high_priority],
		&workflow,
		&state_store,
		"pubfi",
	)
	.expect("candidate selection should succeed")
	.expect("one issue should be selected");

	assert_eq!(selected.identifier, "PUB-102");
}
