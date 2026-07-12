mod mutations;
mod persist;
mod queries;
mod schema;

use std::{fs, path::Path, time::Duration};

use rusqlite::{self, Connection, OptionalExtension, Transaction, params};

use crate::{
	prelude::{Result, eyre},
	state::{
		ChildAgentActivitySummary, ConnectorBackoff, ProjectRegistration, StateData,
		derived_program_intake_plan_records, derived_program_issue_mapping_records,
		runtime_records::{
			AutonomyObjectiveRuntimeRecord, AutonomyProposalRuntimeRecord,
			AutonomyRuntimePolicyRuntimeRecord, AutonomySignalRuntimeRecord,
			DecisionContractRuntimeRecord, ExecutionProgramRuntimeRecord,
			LinearExecutionEventRuntimeRecord, PrivateExecutionEventRuntimeRecord,
			ProtocolEventRecord, RunActivitySummaryRecord, RunAttemptRecord,
			RunControlChannelRecord, WorktreeMappingRecord,
		},
		runtime_row_parsers::{
			connector_backoff_from_row, execution_program_record_from_row_parts,
			execution_program_runtime_row_parts, protocol_event_record_from_row, sqlite_bool_value,
			timestamp_parts,
		},
	},
};

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

		store.reject_future_schema_version()?;
		store.bootstrap_schema()?;

		Ok(store)
	}
}
