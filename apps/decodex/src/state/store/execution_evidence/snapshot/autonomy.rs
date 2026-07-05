use crate::{
	autonomy_objective::AutonomyObjectiveState,
	state::store::{
		AutonomyObjectiveRecord, AutonomyProposalRecord, AutonomySignalRecord,
		DecisionContractRecord, ProgramIntakePlanRecord, ProjectLoopEvidenceSnapshot,
	},
};

impl ProjectLoopEvidenceSnapshot {
	pub(crate) fn recent_autonomy_signals(&self, limit: usize) -> Vec<&AutonomySignalRecord> {
		self.autonomy_signals.iter().take(limit).collect()
	}

	pub(crate) fn recent_autonomy_proposals(&self, limit: usize) -> Vec<&AutonomyProposalRecord> {
		self.autonomy_proposals.iter().take(limit).collect()
	}

	pub(crate) fn autonomy_objective(
		&self,
		objective_id: &str,
		objective_version: u64,
	) -> Option<&AutonomyObjectiveRecord> {
		self.autonomy_objectives.iter().find(|record| {
			record.objective_id() == objective_id && record.version() == objective_version
		})
	}

	pub(crate) fn accepted_autonomy_objectives(&self) -> Vec<&AutonomyObjectiveRecord> {
		self.autonomy_objectives
			.iter()
			.filter(|record| record.state() == AutonomyObjectiveState::Accepted)
			.collect()
	}

	pub(crate) fn decision_contracts_for_autonomy_proposal(
		&self,
		proposal_id: &str,
	) -> Vec<&DecisionContractRecord> {
		self.decision_contracts
			.iter()
			.filter(|record| {
				record.contract().research_provenance().iter().any(|provenance| {
					provenance.kind() == "autonomy_proposal"
						&& provenance.reference() == proposal_id
				})
			})
			.collect()
	}

	pub(crate) fn program_intake_plans_for_contract(
		&self,
		contract_id: &str,
	) -> Vec<&ProgramIntakePlanRecord> {
		self.program_intake_plans
			.iter()
			.filter(|record| record.source_contract_id() == Some(contract_id))
			.collect()
	}
}
