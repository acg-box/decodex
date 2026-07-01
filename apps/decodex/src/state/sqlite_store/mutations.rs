use super::{
	AutonomyObjectiveRuntimeRecord, AutonomyProposalRuntimeRecord, AutonomySignalRuntimeRecord,
	ChildAgentActivitySummary, ConnectorBackoff, DecisionContractRuntimeRecord,
	ExecutionProgramRuntimeRecord, IssueLease, LinearExecutionEventRuntimeRecord,
	OptionalExtension, PrivateExecutionEventRuntimeRecord, ProjectRegistration,
	ProtocolEventRecord, Result, RunActivitySummaryRecord, RunAttemptRecord,
	RunControlChannelRecord, SqliteStateStore, StateData, WorktreeMappingRecord,
	connector_backoff_from_row, eyre, params, persist, protocol_event_record_from_row,
};

mod autonomy;
mod cleanup;
mod project;
mod runs;

impl SqliteStateStore {
	pub(in crate::state) fn persist_runtime_state(&mut self, state: &StateData) -> Result<()> {
		let transaction = self.connection.transaction()?;

		persist::persist_projects(&transaction, state)?;
		persist::persist_leases(&transaction, state)?;
		persist::persist_run_attempts(&transaction, state)?;
		persist::persist_run_control_channels(&transaction, state)?;
		persist::persist_protocol_events(&transaction, state)?;
		persist::persist_run_activity_summaries(&transaction, state)?;
		persist::persist_worktrees(&transaction, state)?;
		persist::persist_linear_execution_events(&transaction, state)?;
		persist::persist_private_execution_events(&transaction, state)?;
		persist::persist_decision_contracts(&transaction, state)?;
		persist::persist_autonomy_objectives(&transaction, state)?;
		persist::persist_autonomy_signals(&transaction, state)?;
		persist::persist_autonomy_proposals(&transaction, state)?;
		persist::persist_execution_programs(&transaction, state)?;
		persist::persist_program_intake_state(&transaction, state)?;
		persist::persist_review_lifecycle_records(&transaction, state)?;
		persist::persist_review_policy_checkpoints(&transaction, state)?;
		persist::persist_evidence_artifacts(&transaction, state)?;
		persist::persist_loop_guardrail_checkpoints(&transaction, state)?;
		persist::persist_connector_backoffs(&transaction, state)?;

		transaction.commit()?;

		Ok(())
	}
}
