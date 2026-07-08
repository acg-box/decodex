mod autonomy_lineage;
mod ghost_lane;
mod lifecycle;
mod liveness;
mod recovery_lineage;

use crate::orchestrator::tests::operator::status::{
	ChildAgentActivitySummary, Command, Connection, Duration, EffectiveRuntimeMarker, FakeTracker,
	HashMap, Instant, LinearExecutionEventIdentity, MODEL_EXECUTION_IDLE_TIMEOUT, OffsetDateTime,
	OperatorRunStatus, OperatorStatusSnapshot, PHASE_GOAL_RECOVERY_EVENT_TYPE, ProjectRegistration,
	ProtocolActivityMarker, ProtocolActivitySummary, RUN_ACTIVITY_MARKER_FILE,
	RUN_LEASE_IDLE_TIMEOUT, RUN_OPERATION_AGENT_RUN, RUN_OPERATION_RECONCILIATION, Read,
	RecoveredRuntimeState, ReviewLifecycleHandoffFixture, ReviewPolicyCheckpointInput,
	ServiceConfig, Shutdown, StateStore, TERMINAL_GUARDED_RUN_STATUS, TEST_SERVICE_ID, TcpListener,
	TempDir, TestEnvVarGuard, TrackerIssue, VALIDATION_EVIDENCE_EVENT_TYPE, WorktreeSpec, Write,
	commit_worktree_change, fs, git_status_success, load_service_config, orchestrator, panic,
	process, rewrite_run_activity_marker_host_boot_id,
	rewrite_run_activity_marker_process_start_identity, sample_issue,
	sample_issue_with_sort_fields, service_config_path, service_config_toml_for_config, state,
	temp_project_layout, thread, tracker, write_service_config,
};
use lifecycle::{
	assert_terminal_pending_interrupt_rejects_force, assert_terminal_pending_lane_inspect,
	assert_terminal_pending_status_projection,
};

#[derive(Clone, Copy)]
struct ReviewCheckpointSeed<'a> {
	issue_id: &'a str,
	run_id: &'a str,
	phase: &'a str,
	status: &'a str,
	head_sha: &'a str,
	nonclean_rounds: i64,
	details_json: &'a str,
}
