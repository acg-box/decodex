use std::cell::RefCell;

use crate::{
	config::ReviewLevel,
	orchestrator::{
		self, IssueDispatchMode, RunSummary,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker,
			intake_candidate_selection::support,
		},
	},
	state::StateStore,
};

#[test]
fn non_github_review_retained_drain_handles_same_issue_closeout_after_merge_visibility() {
	for closeout_available in [true, false] {
		let (temp_dir, config, workflow) = tests::temp_project_layout();
		let repo_root = config.repo_root().to_path_buf();
		let pr_url = "https://github.com/hack-ink/decodex/pull/176";
		let merge_subject = r#"{"schema":"decodex/commit/1","summary":"current retained handoff","authority":"PUB-101"}"#;
		let landed_merge_subject = r#"{"schema":"decodex/commit/1","summary":"Land current retained handoff","authority":"PUB-101"}"#;
		let head_oid =
			tests::commit_worktree_change(&repo_root, "retained.txt", "ready\n", merge_subject);
		let (gh_command_path, invocation_log_path) =
			tests::install_fake_admin_merge_gh_response_with_merge_exit_code(
				&temp_dir, &head_oid, 0,
			);
		let config = tests::service_config_with_review_level(
			&tests::service_config_with_github_token_env_var_and_command_path(
				&config,
				"PATH",
				&gh_command_path,
			),
			ReviewLevel::Standard,
		);
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = support::candidate_selection_service_owned_issue("In Review");
		let tracker = FakeTracker::with_refresh_snapshots(
			vec![issue.clone()],
			vec![
				vec![issue.clone()],
				vec![issue.clone()],
				vec![issue.clone()],
				vec![issue.clone()],
			],
		);

		state_store
			.upsert_worktree(
				config.service_id(),
				&issue.id,
				"main",
				&repo_root.display().to_string(),
			)
			.expect("worktree should record");

		tests::seed_review_handoff_marker_for_path(
			&state_store,
			config.service_id(),
			&repo_root,
			&tests::sample_review_handoff_marker("main", pr_url, &head_oid),
		);

		let open_review_state = tests::sample_pull_request_review_state(
			pr_url,
			"main",
			&head_oid,
			Some("APPROVED"),
			"MERGEABLE",
			"CLEAN",
			Some("SUCCESS"),
			0,
		);
		let mut merged_review_state = open_review_state.clone();

		merged_review_state.state = String::from("MERGED");

		let handoff_summary = support::sample_handoff_summary(&issue, &repo_root);
		let closeout_summary = RunSummary {
			dispatch_mode: IssueDispatchMode::Closeout,
			issue_state: String::from("In Review"),
			initial_issue_state: String::from("In Review"),
			..handoff_summary.clone()
		};
		let closeout_dispatches = RefCell::new(Vec::new());
		let drained = orchestrator::drain_non_github_review_retained_tail_with_inspector(
			&tracker,
			&config,
			&workflow,
			&state_store,
			&handoff_summary,
			&FakePullRequestReviewStateInspector::new(vec![
				Ok(open_review_state.clone()),
				Ok(open_review_state),
				Ok(merged_review_state.clone()),
				Ok(merged_review_state),
			]),
			|source_summary| {
				closeout_dispatches.borrow_mut().push(source_summary.issue_id.clone());

				assert_eq!(source_summary.run_id, handoff_summary.run_id);
				assert_eq!(source_summary.attempt_number, handoff_summary.attempt_number);

				if closeout_available { Ok(Some(closeout_summary.clone())) } else { Ok(None) }
			},
		)
		.expect("non-GitHub-review retained drain should succeed");

		assert_eq!(drained, closeout_available.then_some(closeout_summary.clone()));
		assert_eq!(*closeout_dispatches.borrow(), vec![issue.id.clone()]);

		let marker = tests::persisted_review_orchestration_marker_for_path(
			&state_store,
			config.service_id(),
			&repo_root,
		);

		assert_eq!(marker.phase(), "waiting_for_merge");

		support::assert_admin_merge_invocation(
			&invocation_log_path,
			&head_oid,
			landed_merge_subject,
			pr_url,
		);
	}
}
