use crate::{
	prelude::Result,
	state::store::{StateStore, execution_evidence::snapshot::ProjectLoopEvidenceSnapshot},
};

impl StateStore {
	/// Build one project-scoped loop evidence snapshot for operator status rendering.
	pub(crate) fn project_loop_evidence_snapshot(
		&self,
		project_id: &str,
	) -> Result<ProjectLoopEvidenceSnapshot> {
		let mut state = self.lock_without_refresh()?;
		let mut snapshot = ProjectLoopEvidenceSnapshot::default();

		self.refresh_project_loop_evidence_state_locked(&mut state, project_id)?;

		for record in
			state.private_execution_events.iter().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_private_event(record.as_public());
		}
		for record in
			state.review_lifecycle_records.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_review_lifecycle_record(record.as_public());
		}
		for record in state
			.review_policy_checkpoints
			.values()
			.filter(|record| record.project_id == project_id)
		{
			snapshot.insert_review_checkpoint(record.as_public());
		}
		for record in
			state.decision_contracts.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_decision_contract(record.as_public());
		}
		for record in
			state.autonomy_objectives.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_autonomy_objective(record.as_public());
		}
		for record in
			state.autonomy_signals.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_autonomy_signal(record.as_public());
		}
		for record in
			state.autonomy_proposals.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_autonomy_proposal(record.as_public());
		}
		for record in
			state.program_intake_plans.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_program_intake_plan(record.clone());
		}

		snapshot.sort_private_events();
		snapshot.sort_decision_contracts();
		snapshot.sort_autonomy_objectives();
		snapshot.sort_autonomy_signals();
		snapshot.sort_autonomy_proposals();
		snapshot.sort_program_intake_plans();

		Ok(snapshot)
	}
}
