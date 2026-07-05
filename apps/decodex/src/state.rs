//! Persistent single-machine runtime state for active Decodex execution.

mod internal;
mod models;
mod project_run_recovery;
mod protocol_events;
mod review_records;
mod run_activity_marker;
mod run_attempts;
mod runtime_records;
mod runtime_row_parsers;
mod sqlite_store;
mod store;
mod store_run_control;

pub(crate) use self::{
	models::{
		AutonomyObjectiveRecord, AutonomyProposalRecord, AutonomySignalRecord,
		ChildAgentActivityBucket, ChildAgentActivitySummary, CodexAccountActivitySummary,
		CodexAccountProfileDailyUsageSummary, CodexAccountResetCreditSummary, ConnectorBackoff,
		DecisionContractRecord, ExecutionProgramRecord, IssueLease, LoopGuardrailCheckpoint,
		PreacquiredLeaseGuards, PrivateExecutionEvent, ProgramIntakePlanRecord,
		ProgramIssueMappingRecord, ProjectRegistration, ProjectRunStatus,
		ProtocolActivityEventSummary, ProtocolActivitySummary, ReviewHandoffMarker,
		ReviewLifecycleRecord, ReviewOrchestrationMarker, ReviewPolicyCheckpoint,
		RunActivityMarker, RunAttempt, RunControlActionOutcomeRequest, RunControlActionReceipt,
		RunControlActionRequest, RunControlChannel, WORKTREE_PROVENANCE_FILESYSTEM_SCAN,
		WORKTREE_PROVENANCE_GIT_HYGIENE_SCAN, WORKTREE_PROVENANCE_LEGACY_UNKNOWN,
		WORKTREE_PROVENANCE_RUNTIME_RECORDED, WORKTREE_PROVENANCE_RUNTIME_RECOVERED,
		WorktreeMapping, worktree_provenance,
	},
	run_activity_marker::{
		clear_run_retry_schedule, current_host_boot_id, process_start_identity,
		protocol_event_counts_as_work_progress, read_run_activity_marker,
		read_run_activity_marker_snapshot, read_run_protocol_activity_marker,
		read_run_retry_budget_attempt_count, write_run_account_marker,
		write_run_effective_runtime_marker, write_run_operation_marker,
		write_run_operation_marker_for_process, write_run_operation_marker_preserving_activity,
		write_run_protocol_activity_marker, write_run_retry_budget_attempt_count,
		write_run_retry_schedule, write_run_thread_marker, write_run_thread_status_marker,
		write_run_turn_marker,
	},
	store::{
		ConnectorBackoffInput, LoopGuardrailCheckpointInput, ProjectLoopEvidenceSnapshot,
		ReviewCheckpointArtifactLookup, ReviewPolicyCheckpointInput,
	},
};
pub(crate) use internal::{CodexAccountMarker, EffectiveRuntimeMarker, ProtocolActivityMarker};
#[allow(unused_imports)] pub(crate) use models::WorktreeProvenance;
#[cfg(test)]
pub(crate) use run_activity_marker::{
	current_process_start_identity, read_run_activity_marker_record, write_run_activity_marker,
	write_run_activity_marker_at, write_run_activity_marker_for_process,
	write_run_activity_marker_record,
};
pub(crate) use store::StateStore;

use std::{fs, path::Path};

use serde_json::Value;

use crate::prelude::{Result, eyre};
use internal::{
	DispatchSlotConfig, DispatchSlotGuard, IssueClaimGuard, StateData,
	acquire_shared_lock_coordinator, apply_derived_program_intake_state, clear_close_on_exec,
	compare_project_run_status, derived_program_intake_plan_records,
	derived_program_issue_mapping_records, dispatch_slot_lock_path, issue_claim_id_from_path,
	issue_claim_lock_path, prune_unlocked_shared_lock_files, read_issue_claim_record,
	remove_lock_file_if_exists, set_close_on_exec, write_issue_claim_record,
};
use runtime_records::{
	AutonomyObjectiveRuntimeRecord, AutonomyObjectiveRuntimeRowParts,
	AutonomyProposalRuntimeRecord, AutonomyProposalRuntimeRowParts, AutonomySignalRuntimeRecord,
	AutonomySignalRuntimeRowParts, DecisionContractRuntimeRecord, DecisionContractRuntimeRowParts,
	ExecutionProgramRuntimeRecord, ExecutionProgramRuntimeRowParts,
	LinearExecutionEventRuntimeRecord, PrivateExecutionEventRuntimeRecord, ProtocolEventRecord,
	ProtocolEventSummaryRecord, RunActivitySummaryRecord, RunAttemptRecord,
	RunControlChannelRecord, TimestampParts, WorktreeMappingRecord,
};
use runtime_row_parsers::{
	compare_autonomy_signal_runtime_records, compare_decision_contract_runtime_records,
	compare_execution_program_runtime_records, compare_linear_execution_event_runtime_records,
	compare_private_execution_event_runtime_records, compare_program_intake_plan_records,
	compare_program_issue_mapping_records, compare_recent_autonomy_proposal_runtime_records,
	compare_recent_autonomy_signal_runtime_records, parse_linear_execution_event_unix,
	timestamp_parts, validate_private_execution_event_inputs,
};
use store::retarget::running_run_attempt_status;

pub(crate) const RUN_ACTIVITY_MARKER_FILE: &str = ".decodex-run-activity";
pub(crate) const RUN_OPERATION_IDLE: &str = "idle";
pub(crate) const RUN_OPERATION_GIT_CREDENTIALS: &str = "git_credentials";
pub(crate) const RUN_OPERATION_APP_SERVER_PREFLIGHT: &str = "app_server_preflight";
pub(crate) const RUN_OPERATION_AGENT_RUN: &str = "agent_run";
pub(crate) const RUN_OPERATION_REPO_GATE: &str = "repo_gate";
pub(crate) const RUN_OPERATION_REVIEW_WRITEBACK: &str = "review_writeback";
pub(crate) const RUN_OPERATION_WAITING_EXTERNAL: &str = "waiting_external";
pub(crate) const RUN_OPERATION_RECONCILIATION: &str = "reconciliation";
pub(crate) const RUN_CONTROL_CHANNEL_DIR: &str = ".decodex-run-control";
pub(crate) const RUN_CONTROL_CHANNEL_TRANSPORT_LOCAL_FILE: &str = "local_file";
pub(crate) const RUN_CONTROL_CHANNEL_STATUS_ACTIVE: &str = "active";
pub(crate) const RUN_CONTROL_CHANNEL_STATUS_COMPLETED: &str = "completed";
pub(crate) const RUN_CONTROL_CHANNEL_STATUS_FAILED: &str = "failed";
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const RUN_CONTROL_ACTION_ACCEPTED: &str = "accepted";
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const RUN_CONTROL_ACTION_REJECTED: &str = "rejected";
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const RUN_CONTROL_ACTION_COMPLETED: &str = "completed";
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const RUN_CONTROL_ACTION_FAILED: &str = "failed";
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const RUN_CONTROL_ACTION_TIMED_OUT: &str = "timed_out";
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const RUN_CONTROL_ACTION_FALLBACK: &str = "fallback";

const DISPATCH_SLOT_LOCK_FILE_PREFIX: &str = ".decodex-dispatch-slot";
const ISSUE_CLAIM_LOCK_FILE_PREFIX: &str = ".decodex-issue-claim";

pub(crate) fn is_untracked_decodex_runtime_artifact_status_line(line: &str) -> bool {
	let Some(path) = line.trim_end().strip_prefix("?? ") else {
		return false;
	};

	is_decodex_runtime_artifact_relative_path(Path::new(path))
}

pub(crate) fn retained_path_contains_only_decodex_runtime_artifacts(path: &Path) -> Result<bool> {
	if !path.try_exists()? {
		return Ok(true);
	}
	if !fs::metadata(path)?.is_dir() {
		return Ok(false);
	}

	for entry in fs::read_dir(path)? {
		let entry = entry?;
		let entry_path = entry.path();
		let relative = entry_path.strip_prefix(path).unwrap_or(entry_path.as_path());

		if !is_decodex_runtime_artifact_relative_path(relative) {
			return Ok(false);
		}
	}

	Ok(true)
}

pub(crate) fn is_decodex_runtime_artifact_relative_path(path: &Path) -> bool {
	path == Path::new(RUN_ACTIVITY_MARKER_FILE)
		|| path == Path::new(RUN_CONTROL_CHANNEL_DIR)
		|| path.starts_with(RUN_CONTROL_CHANNEL_DIR)
}

#[cfg(test)] mod tests;
