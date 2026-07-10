use crate::{
	prelude::{Result, eyre},
	state::{
		AutonomyProposalRecord, StateStore,
		runtime_records::{AutonomyProposalKey, AutonomyProposalRuntimeRecord},
		store,
		store::validation,
	},
};

impl StateStore {
	/// Read one autonomy proposal by stable proposal id.
	pub(crate) fn autonomy_proposal(
		&self,
		project_id: &str,
		proposal_id: &str,
	) -> Result<Option<AutonomyProposalRecord>> {
		validation::validate_required_autonomy_proposal_field("project_id", project_id)?;
		validation::validate_required_autonomy_proposal_field("proposal_id", proposal_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.autonomy_proposal(project_id, proposal_id)
				.map(|record| record.map(|record| record.as_public()));
		}

		let state = self.lock()?;

		Ok(state
			.autonomy_proposals
			.get(&AutonomyProposalKey::new(project_id, proposal_id))
			.map(AutonomyProposalRuntimeRecord::as_public))
	}

	pub(crate) fn autonomy_proposal_for_decision_contract(
		&self,
		project_id: &str,
		contract_id: &str,
	) -> Result<Option<AutonomyProposalRecord>> {
		validation::validate_required_autonomy_proposal_field("project_id", project_id)?;
		validation::validate_required_autonomy_proposal_field("contract_id", contract_id)?;

		let prefix = contract_id
			.strip_prefix("autonomy-decision-")
			.filter(|value| value.len() == 32)
			.ok_or_else(|| eyre::eyre!("Decision Contract is not autonomy-proposal-derived."))?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.autonomy_proposal_for_contract(project_id, prefix)
				.map(|record| record.map(|record| record.as_public()));
		}

		let state = self.lock()?;
		let mut matches = state.autonomy_proposals.values().filter(|record| {
			record.project_id == project_id && record.proposal.decision_contract_id() == contract_id
		});
		let first = matches.next().cloned();

		if matches.next().is_some() {
			eyre::bail!("Decision Contract matches multiple autonomy proposals.");
		}

		Ok(first.map(|record| record.as_public()))
	}

	/// List recent autonomy proposals for one project for operator readback.
	pub(crate) fn recent_autonomy_proposals_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomyProposalRecord>> {
		validation::validate_required_autonomy_proposal_field("project_id", project_id)?;

		if limit == 0 {
			return Ok(Vec::new());
		}

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.recent_autonomy_proposals_for_project(project_id, limit)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.autonomy_proposals
			.values()
			.filter(|record| record.project_id == project_id)
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(store::compare_recent_autonomy_proposal_runtime_records);
		records.truncate(limit);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}
}
