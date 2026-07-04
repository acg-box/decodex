use crate::{
	autonomy_objective::AutonomyObjectiveState,
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalChallengeInput, AutonomyProposalDecisionBridgeAuthority,
		AutonomyProposalRefusalReason,
	},
	loop_contract::DecisionContractStatus,
	prelude::{Result, eyre},
	state::{
		AutonomyProposalRecord, DecisionContractRecord, StateStore,
		runtime_records::{
			AutonomyObjectiveKey, AutonomyProposalKey, AutonomyProposalRuntimeRecord,
			AutonomySignalKey,
		},
		runtime_row_parsers,
		store::validation,
	},
};

impl StateStore {
	/// Persist one autonomy proposal as non-executable dry-run evidence.
	pub(crate) fn record_autonomy_proposal(
		&self,
		project_id: &str,
		proposal: AutonomyProposal,
	) -> Result<AutonomyProposalRecord> {
		validation::validate_autonomy_proposal_record_inputs(project_id, &proposal)?;

		let now = runtime_row_parsers::timestamp_parts();
		let mut state = self.lock()?;

		if !proposal.has_refusal_reason(AutonomyProposalRefusalReason::MissingObjective) {
			let objective_key = AutonomyObjectiveKey::new(
				project_id,
				proposal.objective_id(),
				proposal.objective_version(),
			);
			let objective = state.autonomy_objectives.get(&objective_key).ok_or_else(|| {
				eyre::eyre!(
					"Autonomy proposal `{}` references missing objective `{}` version {}.",
					proposal.id(),
					proposal.objective_id(),
					proposal.objective_version()
				)
			})?;

			if objective.state != AutonomyObjectiveState::Accepted {
				eyre::bail!(
					"Autonomy proposal `{}` can only be recorded for an accepted objective version unless it carries missing_objective refusal; `{}` version {} is `{}`.",
					proposal.id(),
					proposal.objective_id(),
					proposal.objective_version(),
					objective.state.as_str()
				);
			}
		}

		for signal_id in proposal.source_signal_ids() {
			let signal = state
				.autonomy_signals
				.get(&AutonomySignalKey::new(project_id, signal_id))
				.ok_or_else(|| {
					eyre::eyre!(
						"Autonomy proposal `{}` references missing signal `{signal_id}`.",
						proposal.id()
					)
				})?;

			if signal.signal.objective_id() != proposal.objective_id()
				|| signal.signal.objective_version() != proposal.objective_version()
			{
				eyre::bail!(
					"Autonomy proposal `{}` signal `{signal_id}` is not tied to objective `{}` version {}.",
					proposal.id(),
					proposal.objective_id(),
					proposal.objective_version()
				);
			}
		}

		let key = AutonomyProposalKey::new(project_id, proposal.id());
		let (created_at, created_at_unix) = state.autonomy_proposals.get(&key).map_or_else(
			|| (now.text.clone(), now.unix),
			|record| (record.created_at.clone(), record.created_at_unix),
		);
		let record = AutonomyProposalRuntimeRecord {
			project_id: project_id.to_owned(),
			state: proposal.state(),
			proposal,
			created_at,
			created_at_unix,
			updated_at: now.text,
			updated_at_unix: now.unix,
		};

		state.autonomy_proposals.insert(record.key(), record.clone());
		self.upsert_autonomy_proposal_locked(&record)?;

		Ok(record.as_public())
	}

	/// Record challenge evidence against one persisted non-executable autonomy proposal.
	pub(crate) fn record_autonomy_proposal_challenge(
		&self,
		project_id: &str,
		proposal_id: &str,
		challenge: AutonomyProposalChallengeInput,
	) -> Result<AutonomyProposalRecord> {
		validation::validate_required_autonomy_proposal_field("project_id", project_id)?;
		validation::validate_required_autonomy_proposal_field("proposal_id", proposal_id)?;

		let now = runtime_row_parsers::timestamp_parts();
		let key = AutonomyProposalKey::new(project_id, proposal_id);
		let mut state = self.lock()?;
		let mut record = state
			.autonomy_proposals
			.get(&key)
			.cloned()
			.ok_or_else(|| eyre::eyre!("Autonomy proposal `{proposal_id}` does not exist."))?;

		record.proposal.record_challenge(challenge)?;

		record.state = record.proposal.state();
		record.updated_at = now.text;
		record.updated_at_unix = now.unix;

		state.autonomy_proposals.insert(key, record.clone());
		self.upsert_autonomy_proposal_locked(&record)?;

		Ok(record.as_public())
	}

	/// Accept one proposal into a normal latent Decision Contract candidate.
	pub(crate) fn accept_autonomy_proposal_as_decision_contract_candidate(
		&self,
		project_id: &str,
		proposal_id: &str,
		authority: AutonomyProposalDecisionBridgeAuthority,
	) -> Result<DecisionContractRecord> {
		validation::validate_required_autonomy_proposal_field("project_id", project_id)?;
		validation::validate_required_autonomy_proposal_field("proposal_id", proposal_id)?;

		let proposal_record = self
			.autonomy_proposal(project_id, proposal_id)?
			.ok_or_else(|| eyre::eyre!("Autonomy proposal `{proposal_id}` does not exist."))?;
		let contract = proposal_record.proposal().to_decision_contract_candidate(authority)?;
		let contract_id = contract.contract_id().to_owned();

		if let Some(existing) = self.decision_contract(project_id, &contract_id)? {
			let existing_contract = existing.contract();
			let has_generated_execution_links =
				!existing_contract.links().generated_issue_ids().is_empty()
					|| !existing_contract.links().generated_issue_identifiers().is_empty()
					|| !existing_contract.links().execution_program_node_ids().is_empty();

			if existing.status() == DecisionContractStatus::DraftLatent
				&& existing_contract.promotion().is_none()
				&& !has_generated_execution_links
			{
				return Ok(existing);
			}

			eyre::bail!(
				"Autonomy proposal `{proposal_id}` already has Decision Contract `{contract_id}` with status `{}`; acceptance will not replace promoted or generated execution authority.",
				existing.status().as_str()
			);
		}

		let source_issue_id = contract.source_intent().source_issue_identifier().map(str::to_owned);

		self.upsert_decision_contract(project_id, source_issue_id.as_deref(), contract)
	}
}
