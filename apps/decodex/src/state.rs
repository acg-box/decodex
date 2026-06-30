//! Persistent single-machine runtime state for active Decodex execution.

#[cfg(unix)] use std::os::{
	fd::{AsRawFd, FromRawFd},
	unix::ffi::OsStrExt,
};
use std::{
	cmp,
	collections::{HashMap, HashSet},
	fs::{self, File, OpenOptions, TryLockError},
	io::{ErrorKind, Read, Seek, SeekFrom, Write},
	path::{Path, PathBuf},
	process,
	sync::{Mutex, MutexGuard, OnceLock},
	time::Duration,
};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use time::OffsetDateTime;

use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveContract, AutonomyObjectiveRejection,
		AutonomyObjectiveState, AutonomyObjectiveSupersession,
	},
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalCompileInput, AutonomyProposalDecisionBridgeAuthority,
		AutonomyProposalRefusalReason, AutonomyProposalState,
	},
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalKind, AutonomySignalPrivacy,
	},
	config::ServiceConfig,
	execution_program::ExecutionProgram,
	loop_contract::{DecisionContract, DecisionContractStatus, DecisionPromotion},
	prelude::{Result, eyre},
	tracker::records::{self, LinearExecutionEventRecord},
};

mod project_run_recovery;
mod runtime_row_parsers;
mod runtime_records;

use runtime_row_parsers::{
	autonomy_objective_record_from_row_parts, autonomy_objective_runtime_row_parts,
	autonomy_proposal_record_from_row_parts, autonomy_proposal_runtime_row_parts,
	autonomy_signal_record_from_row_parts, autonomy_signal_runtime_row_parts,
	compare_attempt_records, compare_autonomy_proposal_runtime_records,
	compare_autonomy_signal_runtime_records, compare_decision_contract_runtime_records,
	compare_execution_program_runtime_records, compare_linear_execution_event_runtime_records,
	compare_private_execution_event_runtime_records, compare_program_intake_plan_records,
	compare_program_issue_mapping_records, compare_recent_autonomy_proposal_runtime_records,
	compare_recent_autonomy_signal_runtime_records, connector_backoff_from_row,
	decision_contract_record_from_row_parts, decision_contract_runtime_row_parts,
	execution_program_record_from_row_parts, execution_program_runtime_row_parts,
	migrate_legacy_decision_contract_payload, program_intake_plan_row, program_issue_mapping_row,
	parse_linear_execution_event_unix, protocol_event_record_from_row,
	protocol_event_summary_from_events, run_activity_summary_record_from_row,
	run_attempt_record_from_row, sqlite_bool_value, timestamp_parts,
	validate_private_execution_event_inputs, worktree_mapping_record_from_row,
};
use runtime_records::{
	AutonomyObjectiveKey, AutonomyObjectiveRuntimeRecord, AutonomyObjectiveRuntimeRowParts,
	AutonomyProposalKey, AutonomyProposalRuntimeRecord, AutonomyProposalRuntimeRowParts,
	AutonomySignalKey, AutonomySignalRuntimeRecord, AutonomySignalRuntimeRowParts,
	DecisionContractKey, DecisionContractRuntimeRecord, DecisionContractRuntimeRowParts,
	EvidenceArtifactKey, EvidenceArtifactRuntimeRecord, ExecutionProgramKey,
	ExecutionProgramRuntimeRecord, ExecutionProgramRuntimeRowParts, GuardRetention,
	LinearExecutionEventRuntimeRecord, LoopGuardrailKey, LoopGuardrailRuntimeRecord,
	PrivateExecutionEventRuntimeRecord, ProgramIntakePlanKey, ProgramIssueMappingKey,
	ProtocolEventRecord, ProtocolEventSummaryRecord, ReviewLifecycleKey,
	ReviewLifecycleRuntimeRecord, ReviewPolicyKey, ReviewPolicyRuntimeRecord,
	RunActivitySummaryRecord, RunAttemptRecord, RunControlChannelRecord, TimestampParts,
	WorktreeMappingRecord,
};

include!("state/store.rs");

include!("state/models.rs");

include!("state/run_activity_marker.rs");

include!("state/internal.rs");

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
