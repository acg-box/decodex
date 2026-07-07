use std::cell::RefCell;

use crate::{
	config::ReviewLevel,
	orchestrator::{
		self, IssueDispatchMode, RetainedReviewRunIdentity, RunSummary,
		TERMINAL_GUARDED_RUN_STATUS,
		tests::{
			self, FakePullRequestReviewStateInspector, FakeTracker,
			intake_candidate_selection::support,
		},
	},
	state::{ReviewPolicyCheckpointInput, StateStore},
};

#[test]
fn non_github_review_retained_drain_handles_same_issue_closeout_after_merge_visibility() {
	for closeout_available in [true, false] {
		let (temp_dir, config, workflow) = tests::temp_project_layout();
		let repo_root = config.repo_root().to_path_buf();
		let pr_url = "https://github.com/hack-ink/decodex/pull/176";
		let merge_subject = r#"{"schema":"decodex/commit/2","change":"current retained handoff","authority":"PUB-101","impact":"compatible"}"#;
		let landed_merge_subject = r#"{"schema":"decodex/commit/2","change":"Land current retained handoff","authority":"PUB-101","impact":"compatible"}"#;
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
		state_store
			.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
				project_id: config.service_id(),
				issue_id: &issue.id,
				run_id: "runtime-review",
				attempt_number: 1,
				phase: "handoff",
				review_level: "standard",
				status: "clean",
				head_sha: &head_oid,
				nonclean_rounds: 0,
				details_json: "{}",
			})
			.expect("runtime clean review checkpoint should seed");

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

		assert_eq!(marker.phase(), "landed");

		support::assert_admin_merge_invocation(
			&invocation_log_path,
			&head_oid,
			landed_merge_subject,
			pr_url,
		);
	}
}

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

#[test]
fn retained_closeout_identity_reuse_respects_attempt_history() {
	{
		let state_store = StateStore::open_in_memory().expect("state store should open");
		let issue = support::candidate_selection_service_owned_issue("In Review");
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
		let issue = support::candidate_selection_service_owned_issue("In Review");
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
		let issue = support::candidate_selection_service_owned_issue("In Review");
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
