use std::fs;

use tempfile::TempDir;

use crate::{
	github::RepositoryContext,
	manual::{
		self, ManualAuthority, ManualLandContext, ManualLandLedgerContext, tests,
		tests::support::TestTracker,
	},
	state::{ReviewHandoffMarker, StateStore},
	tracker::{
		TrackerState, privacy_classifier::ConfiguredPublicProjectionPrivacyClassifier, records,
	},
	worktree::WorktreeManager,
};

#[test]
fn manual_closeout_runtime_clear_removes_lane_state() {
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = tests::sample_issue("issue-1", "XY-225", true, &["decodex:active:pubfi"]);
	let other_issue = tests::sample_issue("issue-2", "XY-226", true, &["decodex:active:pubfi"]);
	let handoff = ReviewHandoffMarker::new(
		"run-1-failed",
		1,
		"y/decodex-xy-225",
		"https://github.com/hack-ink/decodex/pull/67",
		"main",
		"y/decodex-xy-225",
		"deadbeef",
	);

	state_store
		.upsert_lease("decodex", &issue.id, "run-1", "In Progress")
		.expect("issue lease should persist");
	state_store
		.record_run_attempt("run-1", &issue.id, 1, "running")
		.expect("issue running attempt should persist");
	state_store
		.record_run_attempt("run-1-starting", &issue.id, 2, "starting")
		.expect("issue starting attempt should persist");
	state_store
		.record_run_attempt("run-1-failed", &issue.id, 3, "failed")
		.expect("issue terminal attempt should persist");
	state_store
		.upsert_worktree("decodex", &issue.id, "y/decodex-xy-225", "/tmp/worktrees/xy-225")
		.expect("issue worktree should persist");
	state_store
		.upsert_review_handoff_marker("decodex", &issue.id, &handoff)
		.expect("issue handoff should persist");
	state_store
		.upsert_lease("decodex", &other_issue.id, "run-2", "In Progress")
		.expect("other issue lease should persist");
	state_store
		.record_run_attempt("run-2", &other_issue.id, 1, "running")
		.expect("other issue running attempt should persist");

	manual::clear_manual_closeout_runtime_state(&state_store, &issue.id, handoff.run_id())
		.expect("manual closeout runtime state should clear");

	assert!(
		state_store
			.list_leases("decodex")
			.expect("leases should list")
			.iter()
			.all(|lease| lease.issue_id() != issue.id)
	);
	assert!(
		state_store
			.list_leases("decodex")
			.expect("leases should list")
			.iter()
			.any(|lease| lease.issue_id() == other_issue.id)
	);
	assert!(
		state_store
			.worktree_for_issue(&issue.id)
			.expect("worktree lookup should succeed")
			.is_none()
	);
	assert!(
		state_store
			.review_handoff_marker("decodex", &issue.id, "y/decodex-xy-225")
			.expect("handoff lookup should succeed")
			.is_none()
	);
	assert_eq!(
		state_store
			.run_attempt("run-1")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should remain")
			.status(),
		"succeeded"
	);
	assert_eq!(
		state_store
			.run_attempt("run-1-starting")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should remain")
			.status(),
		"succeeded"
	);
	assert_eq!(
		state_store
			.run_attempt("run-1-failed")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should remain")
			.status(),
		"succeeded"
	);
	assert_eq!(
		state_store
			.run_attempt("run-2")
			.expect("run attempt lookup should succeed")
			.expect("run attempt should remain")
			.status(),
		"running"
	);
}

#[test]
fn manual_land_issue_closeout_removes_managed_lane_worktree_and_branch() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = tests::init_git_checkout(&temp_dir, "repo");
	let worktree_root = repo_root.join(".worktrees");

	fs::write(repo_root.join("README.md"), "bootstrap\n").expect("readme should write");
	tests::git_add_and_commit(&repo_root, "README.md", "bootstrap repo");

	let worktree_manager = WorktreeManager::new("pubfi", &repo_root, &worktree_root);
	let worktree =
		worktree_manager.ensure_worktree("XY-225", false).expect("worktree should create");
	let (_path_guard, invocation_log_path) =
		tests::install_fake_admin_merge_gh(&temp_dir, "cafebabe");
	let context = ManualLandContext {
		cwd: worktree.path.clone(),
		current_branch: worktree.branch_name.clone(),
		worktree_root: worktree.path.clone(),
		project_worktree_root: worktree_root.clone(),
		canonical_repo_root: repo_root.clone(),
		authority: ManualAuthority::Issue(String::from("XY-225")),
		service_id: String::from("pubfi"),
		workflow: Some(tests::sample_workflow()),
		github_token_env_var: String::from("GITHUB_TOKEN"),
		github_token: String::from("test-token"),
		github_command_path: None,
		repository: RepositoryContext {
			owner: String::from("hack-ink"),
			name: String::from("decodex"),
			default_branch: String::from("main"),
			merge_commit_allowed: true,
		},
		prepared_closeout: None,
		review_handoff: None,
		pr_url: String::from("https://github.com/hack-ink/decodex/pull/64"),
		review_branch: worktree.branch_name.clone(),
		public_projection_privacy_classifier: ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	};

	manual::cleanup_manual_land_lane_checkout(&context)
		.expect("manual land cleanup should remove the lane checkout");

	let gh_invocations =
		fs::read_to_string(invocation_log_path).expect("fake gh invocation log should read");

	assert!(
		gh_invocations
			.contains("api --method DELETE --silent repos/hack-ink/decodex/git/refs/heads/"),
		"manual land cleanup should delete the remote branch through gh api"
	);
	assert!(!worktree.path.exists(), "manual land cleanup should remove the worktree");
	assert!(
		manual::run_git_capture(&repo_root, &["branch", "--list", &worktree.branch_name])
			.expect("local branch list should run")
			.is_empty(),
		"manual land cleanup should delete the local lane branch"
	);
}

#[test]
fn manual_land_manual_authority_removes_managed_lane_worktree_and_branch() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = tests::init_git_checkout(&temp_dir, "repo");
	let worktree_root = repo_root.join(".worktrees");

	fs::write(repo_root.join("README.md"), "bootstrap\n").expect("readme should write");
	tests::git_add_and_commit(&repo_root, "README.md", "bootstrap repo");

	let worktree_manager = WorktreeManager::new("decodex", &repo_root, &worktree_root);
	let worktree = worktree_manager
		.ensure_worktree("manual-land-cleanup", false)
		.expect("worktree should create");
	let (_path_guard, _invocation_log_path) =
		tests::install_fake_admin_merge_gh(&temp_dir, "cafebabe");
	let context = ManualLandContext {
		cwd: worktree.path.clone(),
		current_branch: worktree.branch_name.clone(),
		worktree_root: worktree.path.clone(),
		project_worktree_root: worktree_root.clone(),
		canonical_repo_root: repo_root.clone(),
		authority: ManualAuthority::Manual,
		service_id: String::from("decodex"),
		workflow: Some(tests::sample_workflow()),
		github_token_env_var: String::from("GITHUB_TOKEN"),
		github_token: String::from("test-token"),
		github_command_path: None,
		repository: RepositoryContext {
			owner: String::from("hack-ink"),
			name: String::from("decodex"),
			default_branch: String::from("main"),
			merge_commit_allowed: true,
		},
		prepared_closeout: None,
		review_handoff: None,
		pr_url: String::from("https://github.com/hack-ink/decodex/pull/65"),
		review_branch: worktree.branch_name.clone(),
		public_projection_privacy_classifier: ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	};

	manual::cleanup_manual_land_lane_checkout(&context)
		.expect("manual authority cleanup should remove the lane checkout");

	assert!(!worktree.path.exists(), "manual authority cleanup should remove the worktree");
	assert!(
		manual::run_git_capture(&repo_root, &["branch", "--list", &worktree.branch_name])
			.expect("local branch list should run")
			.is_empty(),
		"manual authority cleanup should delete the local lane branch"
	);
}

#[test]
fn manual_land_issue_closeout_requires_managed_lane_checkout() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let repo_root = tests::init_git_checkout(&temp_dir, "repo");
	let worktree_root = repo_root.join(".worktrees");

	fs::create_dir_all(&worktree_root).expect("worktree root should exist");

	let error =
		manual::ensure_manual_land_checkout_is_managed_lane(&repo_root, &worktree_root, "XY-225")
			.expect_err("issue closeout should require a managed lane checkout");

	assert!(error.to_string().contains("must run from a managed lane"));
	assert!(error.to_string().contains("XY-225"));
}

#[test]
fn manual_land_issue_closeout_writes_success_ledger_after_existing_marker() {
	let temp_dir = TempDir::new().expect("temp dir should create");
	let checkout = tests::init_git_checkout(&temp_dir, "repo");
	let tracker = TestTracker::new();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let mut issue = tests::sample_issue("issue-1", "PUB-1161", true, &[]);

	issue
		.team
		.states
		.push(TrackerState { id: String::from("state-done"), name: String::from("Done") });

	let handoff = ReviewHandoffMarker::new(
		String::from("pub-1161-attempt-1"),
		1,
		String::from("xy/pub-1161"),
		String::from("https://github.com/helixbox/pubfi-mono-v2/pull/95"),
		String::from("main"),
		String::from("xy/pub-1161"),
		String::from("3cf2d24033527a774340c7d70c5ce437c90afe55"),
	);

	state_store
		.record_run_attempt(handoff.run_id(), &issue.id, handoff.attempt_number(), "failed")
		.expect("failed handoff attempt should record");

	let merge_commit = "81e90b530148a0be69afa5bd33ce6ab84d485a3a";
	let landed_change_record =
		r#"{"schema":"decodex/commit/1","summary":"Land PUB-1161","authority":"PUB-1161"}"#;

	manual::write_manual_land_closeout_marker(
		&checkout,
		"https://github.com/helixbox/pubfi-mono-v2/pull/95",
		merge_commit,
		"xy/pub-1161",
		landed_change_record,
	)
	.expect("existing closeout marker should write");

	let ledger = ManualLandLedgerContext {
		service_id: "pubfi",
		issue: &issue,
		state_store: &state_store,
		handoff: &handoff,
		pr_url: "https://github.com/helixbox/pubfi-mono-v2/pull/95",
		merge_commit,
		branch_name: "xy/pub-1161",
		worktree_path: ".worktrees/PUB-1161",
		completed_state: "Done",
		default_branch: "main",
		privacy_classifier: &ConfiguredPublicProjectionPrivacyClassifier::Disabled,
	};

	manual::apply_closeout(&checkout, &tracker, "Done", &ledger, landed_change_record)
		.expect("manual closeout should write landed and closeout events");
	manual::write_manual_land_cleanup_complete_event(&tracker, &ledger)
		.expect("manual cleanup should write cleanup_complete event");

	let comments = tracker.comments.borrow();
	let records = comments
		.iter()
		.filter_map(|comment| records::parse_linear_execution_event_record(comment))
		.collect::<Vec<_>>();
	let event_types = records.iter().map(|record| record.event_type.as_str()).collect::<Vec<_>>();

	assert_eq!(
		tracker.state_updates.borrow().as_slice(),
		&[vec![String::from("issue-1"), String::from("state-done"),]]
	);
	assert_eq!(event_types, vec!["landed", "closeout", "cleanup_complete"]);
	assert!(
		comments.iter().all(|comment| !comment.starts_with("decodex land completed")),
		"matching legacy closeout marker should not replay the ordinary closeout comment"
	);
	assert!(comments.iter().all(|comment| {
		comment.contains("- run_sequence_attempt: `1` (not retry-budget count)")
			&& !comment.contains("- attempt:")
	}));
	assert!(records.iter().all(|record| record.run_id == "pub-1161-attempt-1"));
	assert!(records.iter().all(|record| record.attempt_number == 1));
	assert_eq!(records[0].pr_head_sha.as_deref(), Some(handoff.pr_head_oid()));
	assert_eq!(records[0].commit_sha.as_deref(), Some(merge_commit));
	assert_eq!(records[1].target_state.as_deref(), Some("Done"));
	assert_eq!(records[2].cleanup_status.as_deref(), Some("completed"));

	let cached_records = state_store
		.list_linear_execution_events("pubfi", "issue-1")
		.expect("local ledger cache should read");
	let cached_event_types =
		cached_records.iter().map(|record| record.event_type.as_str()).collect::<Vec<_>>();

	assert_eq!(cached_event_types, vec!["landed", "closeout", "cleanup_complete"]);
	assert_eq!(
		state_store
			.run_attempt(handoff.run_id())
			.expect("run attempt lookup should succeed")
			.expect("handoff attempt should exist")
			.status(),
		"succeeded"
	);
}
