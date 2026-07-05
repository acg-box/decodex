use std::cell::RefCell;

use crate::{
	config::ReviewLevel,
	orchestrator::{
		self,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker,
			intake_candidate_selection::support,
		},
	},
	state::StateStore,
};

#[test]
fn non_github_review_retained_drain_stops_cleanly_when_checks_are_pending() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let config = tests::service_config_with_review_level(&config, ReviewLevel::Standard);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = support::candidate_selection_service_owned_issue("In Review");
	let tracker = FakeTracker::with_refresh_snapshots(
		vec![issue.clone()],
		vec![vec![issue.clone()], vec![issue.clone()]],
	);
	let repo_root = config.repo_root().to_path_buf();
	let head_oid = tests::git_output(&repo_root, &["rev-parse", "HEAD"]);
	let pr_url = "https://github.com/hack-ink/decodex/pull/177";

	state_store
		.upsert_worktree(config.service_id(), &issue.id, "main", &repo_root.display().to_string())
		.expect("worktree should record");

	tests::seed_review_handoff_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
		&tests::sample_review_handoff_marker("main", pr_url, &head_oid),
	);

	let handoff_summary = support::sample_handoff_summary(&issue, &repo_root);
	let closeout_dispatches = RefCell::new(Vec::new());
	let drained = orchestrator::drain_non_github_review_retained_tail_with_inspector(
		&tracker,
		&config,
		&workflow,
		&state_store,
		&handoff_summary,
		&FakePullRequestReviewStateInspector::new(vec![
			Ok(tests::sample_pull_request_review_state(
				pr_url,
				"main",
				&head_oid,
				Some("APPROVED"),
				"MERGEABLE",
				"CLEAN",
				Some("PENDING"),
				0,
			)),
			Ok(tests::sample_pull_request_review_state(
				pr_url,
				"main",
				&head_oid,
				Some("APPROVED"),
				"MERGEABLE",
				"CLEAN",
				Some("PENDING"),
				0,
			)),
		]),
		|source_summary| {
			closeout_dispatches.borrow_mut().push(source_summary.issue_id.clone());

			Ok(None)
		},
	)
	.expect("pending checks should stop the retained drain cleanly");

	assert!(drained.is_none());
	assert!(closeout_dispatches.borrow().is_empty());

	let marker = tests::persisted_review_orchestration_marker_for_path(
		&state_store,
		config.service_id(),
		&repo_root,
	);

	assert_eq!(marker.phase(), "request_pending");
}
