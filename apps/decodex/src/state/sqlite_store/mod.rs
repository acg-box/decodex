use std::{
	fs,
	path::{Path, PathBuf},
	time::Duration,
};

use rusqlite::{self, Connection, OptionalExtension, Row, Transaction, params};
use serde_json::Value;

use crate::{
	prelude::{Result, eyre},
	tracker::records::LinearExecutionEventRecord,
};

use super::{
	ChildAgentActivitySummary, ConnectorBackoff, IssueLease, ProgramIntakePlanRecord,
	ProgramIssueMappingRecord, ProjectRegistration, StateData, derived_program_intake_plan_records,
	derived_program_issue_mapping_records,
	runtime_records::{
		AutonomyObjectiveRuntimeRecord, AutonomyProposalRuntimeRecord, AutonomySignalRuntimeRecord,
		DecisionContractRuntimeRecord, EvidenceArtifactKey, EvidenceArtifactRuntimeRecord,
		ExecutionProgramRuntimeRecord, LinearExecutionEventRuntimeRecord, LoopGuardrailKey,
		LoopGuardrailRuntimeRecord, PrivateExecutionEventRuntimeRecord, ProgramIntakePlanKey,
		ProgramIssueMappingKey, ProtocolEventRecord, ProtocolEventSummaryRecord,
		ReviewLifecycleKey, ReviewLifecycleRuntimeRecord, ReviewPolicyKey,
		ReviewPolicyRuntimeRecord, RunActivitySummaryRecord, RunAttemptRecord,
		RunControlChannelRecord, WorktreeMappingRecord,
	},
	runtime_row_parsers::{
		autonomy_objective_record_from_row_parts, autonomy_objective_runtime_row_parts,
		autonomy_proposal_record_from_row_parts, autonomy_proposal_runtime_row_parts,
		autonomy_signal_record_from_row_parts, autonomy_signal_runtime_row_parts,
		connector_backoff_from_row, decision_contract_record_from_row_parts,
		decision_contract_runtime_row_parts, execution_program_record_from_row_parts,
		execution_program_runtime_row_parts, migrate_legacy_decision_contract_payload,
		program_intake_plan_row, program_issue_mapping_row, protocol_event_record_from_row,
		run_activity_summary_record_from_row, run_attempt_record_from_row, sqlite_bool_value,
		timestamp_parts, worktree_mapping_record_from_row,
	},
};

mod mutations;
mod persist;
mod queries;
mod schema;

pub(super) struct SqliteStateStore {
	connection: Connection,
}
impl SqliteStateStore {
	pub(super) fn open(path: &Path) -> Result<Self> {
		if let Some(parent) = path.parent() {
			fs::create_dir_all(parent)?;
		}

		let connection = Connection::open(path)?;

		connection.busy_timeout(Duration::from_secs(5))?;

		let store = Self { connection };

		store.bootstrap_schema()?;

		Ok(store)
	}
}
