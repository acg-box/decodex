use crate::state::sqlite_store::{
	SqliteStateStore,
	queries::{Result, StateData},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_state(&self) -> Result<StateData> {
		let mut state = StateData::default();

		self.load_projects(&mut state)?;
		self.load_lanes(&mut state)?;
		self.load_lane_effects(&mut state)?;
		self.load_no_effective_delta_recoveries(&mut state)?;
		self.load_supersession_authority(&mut state)?;
		#[cfg(test)]
		self.load_leases(&mut state)?;
		self.load_run_attempts(&mut state)?;
		self.load_run_control_channels(&mut state)?;
		self.load_protocol_event_summaries(&mut state)?;
		self.load_run_activity_summaries(&mut state)?;
		#[cfg(test)]
		self.load_worktrees(&mut state)?;
		self.load_linear_execution_events(&mut state)?;
		self.load_private_execution_events(&mut state)?;
		self.load_decision_contracts(&mut state)?;
		self.load_autonomy_objectives(&mut state)?;
		self.load_autonomy_runtime_policies(&mut state)?;
		self.load_autonomy_signals(&mut state)?;
		self.load_autonomy_proposals(&mut state)?;
		self.load_execution_programs(&mut state)?;
		self.load_intake_authorities(&mut state)?;
		self.load_program_intake_state(&mut state)?;
		self.load_review_lifecycle_records(&mut state)?;
		self.load_review_policy_checkpoints(&mut state)?;
		self.load_evidence_artifacts(&mut state)?;
		self.load_loop_guardrail_checkpoints(&mut state)?;
		self.load_connector_backoffs(&mut state)?;

		Ok(state)
	}

	pub(in crate::state) fn load_project_run_metadata_for_project(
		&self,
		project_id: &str,
	) -> Result<StateData> {
		let mut state = StateData::default();

		#[cfg(test)]
		self.load_leases(&mut state)?;
		self.load_run_attempts_for_project(&mut state, project_id)?;
		self.load_run_activity_summaries_for_loaded_runs(&mut state)?;
		#[cfg(test)]
		self.load_worktrees(&mut state)?;
		self.load_run_control_channels_for_project(&mut state, project_id)?;

		Ok(state)
	}

	pub(in crate::state) fn load_project_loop_evidence_for_project(
		&self,
		project_id: &str,
	) -> Result<StateData> {
		let mut state = StateData::default();

		self.load_private_execution_events_for_project(&mut state, project_id)?;
		self.load_review_lifecycle_records_for_project(&mut state, project_id)?;
		self.load_review_policy_checkpoints_for_project(&mut state, project_id)?;
		self.load_evidence_artifacts_for_project(&mut state, project_id)?;
		self.load_autonomy_signals_for_project(&mut state, project_id)?;
		self.load_autonomy_proposals_for_project(&mut state, project_id)?;

		Ok(state)
	}

	pub(in crate::state) fn load_project_registry_state(&self) -> Result<StateData> {
		let mut state = StateData::default();

		self.load_projects(&mut state)?;

		Ok(state)
	}
}
