pub(in crate::state) mod retarget;

mod autonomy;
mod decision_contracts;
mod execution_evidence;
mod inputs;
mod leases;
mod persistence;
mod programs;
mod projects;

pub(crate) use self::{
	execution_evidence::ProjectLoopEvidenceSnapshot,
	inputs::{
		ConnectorBackoffInput, LoopGuardrailCheckpointInput, ReviewCheckpointArtifactLookup,
		ReviewPolicyCheckpointInput,
	},
};

use std::{path::Path, sync::Mutex};

use crate::{
	autonomy_objective::AutonomyObjectiveContract,
	autonomy_proposal::AutonomyProposal,
	autonomy_signal::AutonomySignal,
	execution_program::ExecutionProgram,
	loop_contract::DecisionContract,
	prelude::{Result, eyre},
	state::{
		AutonomyObjectiveRecord, AutonomyProposalRecord, AutonomySignalRecord,
		ChildAgentActivitySummary, ConnectorBackoff, DecisionContractRecord, DispatchSlotConfig,
		DispatchSlotGuard, ExecutionProgramRecord, IssueClaimGuard, IssueLease,
		PreacquiredLeaseGuards, PrivateExecutionEvent, ProgramIntakePlanRecord,
		ProgramIssueMappingRecord, ProjectRegistration, ProtocolActivitySummary,
		ReviewLifecycleRecord, ReviewPolicyCheckpoint, StateData, acquire_shared_lock_coordinator,
		apply_derived_program_intake_state, clear_close_on_exec,
		compare_autonomy_proposal_runtime_records, compare_autonomy_signal_runtime_records,
		compare_decision_contract_runtime_records, compare_execution_program_runtime_records,
		compare_linear_execution_event_runtime_records,
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

#[allow(dead_code)]
pub(super) fn validate_decision_contract_record_inputs(
	project_id: &str,
	source_issue_id: Option<&str>,
	contract: &DecisionContract,
) -> Result<()> {
	validate_required_decision_contract_field("project_id", project_id)?;

	if let Some(source_issue_id) = source_issue_id {
		validate_required_decision_contract_field("source_issue_id", source_issue_id)?;
	}

	contract.validate()
}

#[allow(dead_code)]
pub(super) fn validate_required_decision_contract_field(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("Decision contract {name} must not be empty.");
	}

	Ok(())
}

#[allow(dead_code)]
pub(super) fn validate_autonomy_objective_record_inputs(
	project_id: &str,
	objective: &AutonomyObjectiveContract,
) -> Result<()> {
	validate_required_autonomy_objective_field("project_id", project_id)?;

	if objective.project_id() != project_id {
		eyre::bail!(
			"Autonomy objective `{}` belongs to project `{}` but was stored for `{}`.",
			objective.id(),
			objective.project_id(),
			project_id
		);
	}

	objective.validate()
}

#[allow(dead_code)]
pub(super) fn validate_required_autonomy_objective_field(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("Autonomy objective {name} must not be empty.");
	}

	Ok(())
}

#[allow(dead_code)]
pub(super) fn validate_autonomy_objective_version(version: u64) -> Result<()> {
	if version == 0 {
		eyre::bail!("Autonomy objective version must be greater than zero.");
	}

	Ok(())
}

#[allow(dead_code)]
pub(super) fn validate_autonomy_signal_record_inputs(
	project_id: &str,
	signal: &AutonomySignal,
) -> Result<()> {
	validate_required_autonomy_signal_field("project_id", project_id)?;

	if signal.project_id() != project_id {
		eyre::bail!(
			"Autonomy signal `{}` belongs to project `{}` but was stored for `{}`.",
			signal.id(),
			signal.project_id(),
			project_id
		);
	}

	signal.validate()
}

#[allow(dead_code)]
pub(super) fn validate_required_autonomy_signal_field(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("Autonomy signal {name} must not be empty.");
	}

	Ok(())
}

#[allow(dead_code)]
pub(super) fn validate_autonomy_proposal_record_inputs(
	project_id: &str,
	proposal: &AutonomyProposal,
) -> Result<()> {
	validate_required_autonomy_proposal_field("project_id", project_id)?;

	if proposal.project_id() != project_id {
		eyre::bail!(
			"Autonomy proposal `{}` belongs to project `{}` but was stored for `{}`.",
			proposal.id(),
			proposal.project_id(),
			project_id
		);
	}

	proposal.validate()
}

#[allow(dead_code)]
pub(super) fn validate_required_autonomy_proposal_field(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("Autonomy proposal {name} must not be empty.");
	}

	Ok(())
}

#[allow(dead_code)]
pub(super) fn validate_execution_program_record_inputs(
	project_id: &str,
	program: &ExecutionProgram,
) -> Result<()> {
	validate_required_execution_program_field("project_id", project_id)?;

	program.validate()
}

#[allow(dead_code)]
pub(super) fn validate_required_execution_program_field(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("Execution program {name} must not be empty.");
	}

	Ok(())
}
