pub(in crate::state) mod retarget;

mod autonomy;
mod decision_contracts;
mod execution_evidence;
mod inputs;
mod leases;
mod persistence;
mod programs;
mod projects;
mod validation;

pub(crate) use self::{
	execution_evidence::ProjectLoopEvidenceSnapshot,
	inputs::{
		ConnectorBackoffInput, LoopGuardrailCheckpointInput, ReviewCheckpointArtifactLookup,
		ReviewPolicyCheckpointInput,
	},
	programs::{ProgramIntakeAttemptClaim, ProgramIntakeAttemptStatus},
};

use std::{path::Path, sync::Mutex};

use crate::{
	prelude::Result,
	state::{
		AutonomyObjectiveRecord, AutonomyProposalRecord, AutonomySignalRecord,
		ChildAgentActivitySummary, ConnectorBackoff, DecisionContractRecord, DispatchSlotConfig,
		DispatchSlotGuard, ExecutionProgramRecord, IssueClaimGuard, IssueLease,
		PreacquiredLeaseGuards, PrivateExecutionEvent, ProgramIntakePlanRecord,
		ProgramIssueMappingRecord, ProjectRegistration, ProtocolActivitySummary,
		ReviewLifecycleRecord, ReviewPolicyCheckpoint, StateData, acquire_shared_lock_coordinator,
		apply_derived_program_intake_state, clear_close_on_exec,
		compare_autonomy_signal_runtime_records, compare_decision_contract_runtime_records,
		compare_execution_program_runtime_records, compare_linear_execution_event_runtime_records,
		compare_private_execution_event_runtime_records, compare_program_intake_plan_records,
		compare_program_issue_mapping_records, compare_recent_autonomy_proposal_runtime_records,
		compare_recent_autonomy_signal_runtime_records, dispatch_slot_lock_path,
		issue_claim_id_from_path, issue_claim_lock_path, parse_linear_execution_event_unix,
		prune_unlocked_shared_lock_files, read_issue_claim_record,
		read_run_activity_marker_snapshot, remove_lock_file_if_exists, set_close_on_exec,
		sqlite_store::SqliteStateStore, timestamp_parts, validate_private_execution_event_inputs,
		write_issue_claim_record,
	},
};

/// Local runtime store for leases, attempts, worktrees, protocol events, and private evidence.
#[derive(Default)]
pub struct StateStore {
	pub(super) inner: Mutex<StateData>,
	pub(super) sqlite: Option<Mutex<SqliteStateStore>>,
}
impl StateStore {
	/// Open the local persistent runtime store.
	pub fn open(path: impl AsRef<Path>) -> Result<Self> {
		let sqlite = SqliteStateStore::open(path.as_ref())?;
		let state = sqlite.load_state()?;

		Ok(Self { inner: Mutex::new(state), sqlite: Some(Mutex::new(sqlite)) })
	}

	/// Open the local persistent runtime store without preloading durable rows.
	pub fn open_lazy(path: impl AsRef<Path>) -> Result<Self> {
		let sqlite = SqliteStateStore::open(path.as_ref())?;

		Ok(Self { inner: Mutex::new(StateData::default()), sqlite: Some(Mutex::new(sqlite)) })
	}

	/// Open an in-memory runtime store for tests.
	pub fn open_in_memory() -> Result<Self> {
		Ok(Self::default())
	}
}
