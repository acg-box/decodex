use super::*;

#[test]
fn recovery_read_only_backoff_observer_does_not_clear_expired_backoff() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		sample_recovery_context(&temp_dir, super::super::RecoveryRuntimeMutationPolicy::ReadOnly);
	let expired_unix_epoch = time::OffsetDateTime::now_utc().unix_timestamp() - 1;

	context
		.state_store
		.upsert_connector_backoff(ConnectorBackoffInput {
			project_id: context.config.service_id(),
			connector: "linear",
			sync_phase: "ghost_lane_recovery",
			quota_class: "linear_graphql_rate_limit",
			reset_unix_epoch: expired_unix_epoch,
			reset_source: "test",
			warning: super::super::LINEAR_RATE_LIMIT_BACKOFF_WARNING,
		})
		.expect("backoff should persist");

	let message = super::super::active_recovery_tracker_backoff_message(&context)
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
		sample_recovery_context(&temp_dir, super::super::RecoveryRuntimeMutationPolicy::ReadOnly);
	let error = crate::prelude::eyre::eyre!("Linear connector timed out while testing");
	let message = super::super::remember_recovery_tracker_backoff_message(
		&context,
		&error,
		"ghost_lane_recovery",
	)
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
fn review_handoff_diagnose_skips_terminal_identifier_worktree_before_tracker_refresh() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		sample_recovery_context(&temp_dir, super::super::RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::missing();
	let stale_issue_id = "PUB-001";
	let stale_worktree_path = context.config.worktree_root().join(stale_issue_id);

	context
		.state_store
		.record_run_attempt("run-01", stale_issue_id, 1, super::super::GHOST_LANE_TERMINAL_STATUS)
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
		super::super::diagnose_all_retained_review_worktrees_with_tracker(&context, &tracker)
			.expect("retained review diagnostics should build");
	let diagnostic = diagnostics.first().expect("local residue diagnostic should render");

	assert_eq!(diagnostics.len(), 1);
	assert_eq!(
		diagnostic.classification,
		super::super::REVIEW_HANDOFF_STALE_TERMINAL_RESIDUE_CLASSIFICATION
	);
	assert_eq!(diagnostic.issue_id, stale_issue_id);
	assert_eq!(diagnostic.issue_state, "local_terminal_residue");
	assert!(
		tracker.refresh_queries.borrow().is_empty(),
		"terminal local identifier residue must not be sent to tracker refresh"
	);
}

#[test]
fn review_handoff_diagnose_targeted_terminal_identifier_worktree_before_tracker_lookup() {
	let temp_dir = TempDir::new().expect("tempdir should create");
	let context =
		sample_recovery_context(&temp_dir, super::super::RecoveryRuntimeMutationPolicy::ReadOnly);
	let tracker = GhostLaneTestTracker::identifier_error(
		"Linear GraphQL request failed: Argument Validation Error",
	);
	let stale_issue_id = "PUB-001";
	let stale_worktree_path = context.config.worktree_root().join(stale_issue_id);

	context
		.state_store
		.record_run_attempt("run-01", stale_issue_id, 1, super::super::GHOST_LANE_TERMINAL_STATUS)
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

	let diagnostic = super::super::diagnose_issue_with_tracker(&context, &tracker, stale_issue_id)
		.expect("targeted retained review diagnostic should classify local residue");

	assert_eq!(
		diagnostic.classification,
		super::super::REVIEW_HANDOFF_STALE_TERMINAL_RESIDUE_CLASSIFICATION
	);
	assert_eq!(diagnostic.issue_id, stale_issue_id);
	assert_eq!(diagnostic.issue_state, "local_terminal_residue");
	assert!(
		tracker.refresh_queries.borrow().is_empty(),
		"targeted terminal local residue must not be sent to tracker refresh"
	);
}
