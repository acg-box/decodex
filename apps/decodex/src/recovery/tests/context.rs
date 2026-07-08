use tempfile::TempDir;
use time::OffsetDateTime;

use crate::{
	prelude::eyre,
	recovery::{
		GHOST_LANE_TERMINAL_STATUS, LINEAR_RATE_LIMIT_BACKOFF_WARNING,
		RecoveryRuntimeMutationPolicy,
		tests::{self, GhostLaneTestTracker},
	},
	state::{ConnectorBackoffInput, ReviewLifecycleHandoffFixture},
};

#[test]
fn recovery_read_only_backoff_observer_does_not_clear_expired_backoff() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let expired_unix_epoch = OffsetDateTime::now_utc().unix_timestamp() - 1;

	context
		.state_store
		.upsert_connector_backoff(ConnectorBackoffInput {
			project_id: context.config.service_id(),
			connector: "linear",
			sync_phase: "ghost_lane_recovery",
			quota_class: "linear_graphql_rate_limit",
			reset_unix_epoch: expired_unix_epoch,
			reset_source: "test",
			warning: LINEAR_RATE_LIMIT_BACKOFF_WARNING,
		})
		.expect("backoff should persist");

	let message = super::active_recovery_tracker_backoff_message(&context)
		.expect("backoff observer should run");

	assert_eq!(message, None);
	assert!(
		context
			.state_store
			.connector_backoff(context.config.service_id(), "linear")
			.expect("backoff should read")
			.is_some(),
		"read-only recovery diagnostics must not clear stored connector backoff"
	);
}

#[test]
fn recovery_read_only_backoff_recorder_does_not_persist_new_backoff() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let error = eyre::eyre!("Linear connector timed out while testing");
	let message =
		super::remember_recovery_tracker_backoff_message(&context, &error, "ghost_lane_recovery")
			.expect("timeout should produce backoff message");

	assert!(message.contains("Linear connector is in backoff"));
	assert!(
		context
			.state_store
			.connector_backoff(context.config.service_id(), "linear")
			.expect("backoff should read")
			.is_none(),
		"read-only recovery diagnostics must not persist new connector backoff"
	);
}

#[test]
fn skips_terminal_identifier_worktree_before_refresh() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::missing();
	let stale_issue_id = "PUB-001";
	let stale_worktree_path = context.config.worktree_root().join(stale_issue_id);

	context
		.state_store
		.record_run_attempt("run-01", stale_issue_id, 1, GHOST_LANE_TERMINAL_STATUS)
		.expect("terminal attempt should record");
	context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			stale_issue_id,
			"x/pubfi-pub-001",
			&stale_worktree_path.display().to_string(),
		)
		.expect("stale worktree mapping should record");

	let diagnostics =
		super::diagnose_all_retained_review_worktrees_with_tracker(&context, &tracker)
			.expect("retained review diagnostics should build");
	let diagnostic = diagnostics.first().expect("local residue diagnostic should render");

	assert_eq!(diagnostics.len(), 1);
	assert_eq!(diagnostic.classification, super::super::STALE_TERMINAL_RESIDUE_CLASSIFICATION);
	assert_eq!(diagnostic.issue_id, stale_issue_id);
	assert_eq!(diagnostic.issue_state, "local_terminal_residue");
	assert!(
		tracker.refresh_queries.borrow().is_empty(),
		"terminal local identifier residue must not be sent to tracker refresh"
	);
}

#[test]
fn diagnose_terminal_identifier_worktree_before_lookup() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::identifier_error(
		"Linear GraphQL request failed: Argument Validation Error",
	);
	let stale_issue_id = "PUB-001";
	let stale_worktree_path = context.config.worktree_root().join(stale_issue_id);

	context
		.state_store
		.record_run_attempt("run-01", stale_issue_id, 1, GHOST_LANE_TERMINAL_STATUS)
		.expect("terminal attempt should record");
	context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			stale_issue_id,
			"x/pubfi-pub-001",
			&stale_worktree_path.display().to_string(),
		)
		.expect("stale worktree mapping should record");

	let diagnostic = super::diagnose_issue_with_tracker(&context, &tracker, stale_issue_id)
		.expect("targeted retained review diagnostic should classify local residue");

	assert_eq!(diagnostic.classification, super::super::STALE_TERMINAL_RESIDUE_CLASSIFICATION);
	assert_eq!(diagnostic.issue_id, stale_issue_id);
	assert_eq!(diagnostic.issue_state, "local_terminal_residue");
	assert!(
		tracker.refresh_queries.borrow().is_empty(),
		"targeted terminal local residue must not be sent to tracker refresh"
	);
}

#[test]
fn review_handoff_diagnose_includes_failure_state_retained_lane_and_pr_read_error() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		tests::sample_recovery_context(&temp_dir, RecoveryRuntimeMutationPolicy::ReadOnly);
	let issue = tests::sample_issue_with_labels("Todo", &[String::from("decodex:needs-attention")]);
	let tracker = GhostLaneTestTracker::with_issues(vec![issue.clone()]);
	let pr_url = "https://github.com/hack-ink/pubfi-mono-v2/pull/14";
	let branch_name = "x/pubfi-pub-718";
	let (worktree_dir, _original_head, head_oid) = tests::temp_git_worktree(branch_name);

	context
		.state_store
		.upsert_worktree(
			context.config.service_id(),
			&issue.id,
			branch_name,
			&worktree_dir.path().display().to_string(),
		)
		.expect("retained worktree should record");
	context
		.state_store
		.record_run_attempt("pub-718-attempt-1", &issue.id, 1, "failed")
		.expect("failed attempt should record");
	context
		.state_store
		.upsert_review_lifecycle_handoff_fixture(
			context.config.service_id(),
			&issue.id,
			&ReviewLifecycleHandoffFixture::new(
				"pub-718-attempt-1",
				1,
				branch_name,
				pr_url,
				"main",
				branch_name,
				&head_oid,
			),
		)
		.expect("review lifecycle handoff fixture should record");

	let diagnostics =
		super::diagnose_all_retained_review_worktrees_with_tracker(&context, &tracker)
			.expect("retained review diagnostics should build");
	let diagnostic = diagnostics.first().expect("retained failure-state diagnostic should render");

	assert_eq!(diagnostics.len(), 1);
	assert_eq!(diagnostic.issue_identifier, "PUB-718");
	assert_eq!(diagnostic.issue_state, "Todo");
	assert_eq!(diagnostic.reason, "pull_request_state_read_failed");
	assert_eq!(diagnostic.existing_pr_url.as_deref(), Some(pr_url));
	assert_eq!(diagnostic.local_head_oid.as_deref(), Some(head_oid.as_str()));
	assert_eq!(diagnostic.active_label_present, Some(false));
	assert!(
		diagnostic.pr_read_error.as_deref().is_some_and(|error| !error.trim().is_empty()),
		"diagnose should report the concrete external PR read failure"
	);
}
