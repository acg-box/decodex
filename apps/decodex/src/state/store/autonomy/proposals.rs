use crate::{
	autonomy_objective::AutonomyObjectiveState,
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalChallengeInput, AutonomyProposalCompileInput,
		AutonomyProposalDecisionBridgeAuthority, AutonomyProposalRefusalReason,
	},
	loop_contract::DecisionContractStatus,
	prelude::{Result, eyre},
	state::{
		AutonomyProposalRecord, DecisionContractRecord, StateStore,
		runtime_records::{
			AutonomyObjectiveKey, AutonomyProposalKey, AutonomyProposalRuntimeRecord,
			AutonomySignalKey,
		},
		runtime_row_parsers, store,
	},
};

impl StateStore {
	/// Compile a non-mutating autonomy proposal dry-run from persisted objective and signal rows.
	#[allow(dead_code)]
	pub(crate) fn compile_autonomy_proposal_dry_run(
		&self,
		input: AutonomyProposalCompileInput,
		signal_ids: &[String],
	) -> Result<AutonomyProposal> {
		let objective = self
			.autonomy_objective(&input.project_id, &input.objective_id, input.objective_version)?
			.map(|record| record.objective().clone());
		let mut signals = Vec::new();

		for signal_id in signal_ids {
			store::validate_required_autonomy_proposal_field("signal_id", signal_id)?;

			let signal = self.autonomy_signal(&input.project_id, signal_id)?.ok_or_else(|| {
				eyre::eyre!("Autonomy proposal signal `{signal_id}` does not exist.")
			})?;

			signals.push(signal.signal().clone());
		}

		AutonomyProposal::compile_dry_run(objective.as_ref(), &signals, input)
	}

	/// Persist one autonomy proposal as non-executable dry-run evidence.
	#[allow(dead_code)]
	pub(crate) fn record_autonomy_proposal(
		&self,
		project_id: &str,
		proposal: AutonomyProposal,
	) -> Result<AutonomyProposalRecord> {
		store::validate_autonomy_proposal_record_inputs(project_id, &proposal)?;

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
	#[allow(dead_code)]
	pub(crate) fn record_autonomy_proposal_challenge(
		&self,
		project_id: &str,
		proposal_id: &str,
		challenge: AutonomyProposalChallengeInput,
	) -> Result<AutonomyProposalRecord> {
		store::validate_required_autonomy_proposal_field("project_id", project_id)?;
		store::validate_required_autonomy_proposal_field("proposal_id", proposal_id)?;

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
	#[allow(dead_code)]
	pub(crate) fn accept_autonomy_proposal_as_decision_contract_candidate(
		&self,
		project_id: &str,
		proposal_id: &str,
		authority: AutonomyProposalDecisionBridgeAuthority,
	) -> Result<DecisionContractRecord> {
		store::validate_required_autonomy_proposal_field("project_id", project_id)?;
		store::validate_required_autonomy_proposal_field("proposal_id", proposal_id)?;

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

	/// Read one autonomy proposal by stable proposal id.
	#[allow(dead_code)]
	pub(crate) fn autonomy_proposal(
		&self,
		project_id: &str,
		proposal_id: &str,
	) -> Result<Option<AutonomyProposalRecord>> {
		store::validate_required_autonomy_proposal_field("project_id", project_id)?;
		store::validate_required_autonomy_proposal_field("proposal_id", proposal_id)?;

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

	/// List autonomy proposals tied to one exact Objective Contract version.
	#[allow(dead_code)]
	pub(crate) fn list_autonomy_proposals_for_objective(
		&self,
		project_id: &str,
		objective_id: &str,
		objective_version: u64,
	) -> Result<Vec<AutonomyProposalRecord>> {
		store::validate_required_autonomy_proposal_field("project_id", project_id)?;
		store::validate_required_autonomy_proposal_field("objective_id", objective_id)?;
		store::validate_autonomy_objective_version(objective_version)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.list_autonomy_proposals_for_objective(project_id, objective_id, objective_version)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.autonomy_proposals
			.values()
			.filter(|record| {
				record.project_id == project_id
					&& record.proposal.objective_id() == objective_id
					&& record.proposal.objective_version() == objective_version
			})
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(crate::state::store::compare_autonomy_proposal_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List recent autonomy proposals for one project for operator readback.
	#[allow(dead_code)]
	pub(crate) fn recent_autonomy_proposals_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomyProposalRecord>> {
		store::validate_required_autonomy_proposal_field("project_id", project_id)?;

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

		records.sort_by(crate::state::store::compare_recent_autonomy_proposal_runtime_records);
		records.truncate(limit);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}
}
