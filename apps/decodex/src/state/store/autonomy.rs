use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveContract, AutonomyObjectiveRejection,
		AutonomyObjectiveState, AutonomyObjectiveSupersession,
	},
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalChallengeInput, AutonomyProposalCompileInput,
		AutonomyProposalDecisionBridgeAuthority, AutonomyProposalRefusalReason,
	},
	autonomy_signal::AutonomySignal,
	loop_contract::DecisionContractStatus,
	prelude::{Result, eyre},
};

use super::{
	super::runtime_records::{
		AutonomyObjectiveKey, AutonomyObjectiveRuntimeRecord, AutonomyProposalKey,
		AutonomyProposalRuntimeRecord, AutonomySignalKey, AutonomySignalRuntimeRecord,
	},
	AutonomyObjectiveRecord, AutonomyProposalRecord, AutonomySignalRecord, DecisionContractRecord,
	StateStore, compare_autonomy_proposal_runtime_records, compare_autonomy_signal_runtime_records,
	compare_recent_autonomy_proposal_runtime_records,
	compare_recent_autonomy_signal_runtime_records, timestamp_parts,
	validate_autonomy_objective_record_inputs, validate_autonomy_objective_version,
	validate_autonomy_proposal_record_inputs, validate_autonomy_signal_record_inputs,
	validate_required_autonomy_objective_field, validate_required_autonomy_proposal_field,
	validate_required_autonomy_signal_field,
};

impl StateStore {
	/// Create or replace one draft Objective Contract authority payload.
	#[allow(dead_code)]
	pub(crate) fn upsert_autonomy_objective_draft(
		&self,
		project_id: &str,
		objective: AutonomyObjectiveContract,
	) -> Result<AutonomyObjectiveRecord> {
		validate_autonomy_objective_record_inputs(project_id, &objective)?;

		if objective.state() != AutonomyObjectiveState::Draft {
			eyre::bail!("Autonomy objective drafts must be stored with state `draft`.");
		}

		let now = timestamp_parts();
		let key = AutonomyObjectiveKey::new(project_id, objective.id(), objective.version());
		let mut state = self.lock()?;

		if let Some(existing) = state.autonomy_objectives.get(&key)
			&& existing.state != AutonomyObjectiveState::Draft
		{
			eyre::bail!(
				"Autonomy objective `{}` version {} is `{}` and cannot be replaced as a draft.",
				objective.id(),
				objective.version(),
				existing.state.as_str()
			);
		}

		let (created_at, created_at_unix) = state.autonomy_objectives.get(&key).map_or_else(
			|| (now.text.clone(), now.unix),
			|record| (record.created_at.clone(), record.created_at_unix),
		);
		let record = AutonomyObjectiveRuntimeRecord {
			project_id: project_id.to_owned(),
			state: objective.state(),
			objective,
			created_at,
			created_at_unix,
			updated_at: now.text,
			updated_at_unix: now.unix,
		};

		state.autonomy_objectives.insert(record.key(), record.clone());
		self.upsert_autonomy_objective_locked(&record)?;

		Ok(record.as_public())
	}

	/// Read one Objective Contract version by project, objective id, and version.
	#[allow(dead_code)]
	pub(crate) fn autonomy_objective(
		&self,
		project_id: &str,
		objective_id: &str,
		version: u64,
	) -> Result<Option<AutonomyObjectiveRecord>> {
		validate_required_autonomy_objective_field("project_id", project_id)?;
		validate_required_autonomy_objective_field("objective_id", objective_id)?;
		validate_autonomy_objective_version(version)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.autonomy_objective(project_id, objective_id, version)
				.map(|record| record.map(|record| record.as_public()));
		}

		let state = self.lock()?;

		Ok(state
			.autonomy_objectives
			.get(&AutonomyObjectiveKey::new(project_id, objective_id, version))
			.map(AutonomyObjectiveRuntimeRecord::as_public))
	}

	/// Read the current accepted Objective Contract version for one objective id.
	#[allow(dead_code)]
	pub(crate) fn current_accepted_autonomy_objective(
		&self,
		project_id: &str,
		objective_id: &str,
	) -> Result<Option<AutonomyObjectiveRecord>> {
		validate_required_autonomy_objective_field("project_id", project_id)?;
		validate_required_autonomy_objective_field("objective_id", objective_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.current_accepted_autonomy_objective(project_id, objective_id)
				.map(|record| record.map(|record| record.as_public()));
		}

		let state = self.lock()?;

		Ok(state
			.autonomy_objectives
			.values()
			.filter(|record| {
				record.project_id == project_id
					&& record.objective.id() == objective_id
					&& record.state == AutonomyObjectiveState::Accepted
			})
			.max_by_key(|record| record.objective.version())
			.map(AutonomyObjectiveRuntimeRecord::as_public))
	}

	/// List all Objective Contract versions for one objective id.
	#[allow(dead_code)]
	pub(crate) fn list_autonomy_objective_history(
		&self,
		project_id: &str,
		objective_id: &str,
	) -> Result<Vec<AutonomyObjectiveRecord>> {
		validate_required_autonomy_objective_field("project_id", project_id)?;
		validate_required_autonomy_objective_field("objective_id", objective_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.list_autonomy_objective_history(project_id, objective_id)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.autonomy_objectives
			.values()
			.filter(|record| {
				record.project_id == project_id && record.objective.id() == objective_id
			})
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by_key(|record| record.objective.version());

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List recent Objective Contract versions for one project for MCP/operator readback.
	#[allow(dead_code)]
	pub(crate) fn recent_autonomy_objectives_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomyObjectiveRecord>> {
		validate_required_autonomy_objective_field("project_id", project_id)?;

		if limit == 0 {
			return Ok(Vec::new());
		}

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.recent_autonomy_objectives_for_project(project_id, limit)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.autonomy_objectives
			.values()
			.filter(|record| record.project_id == project_id)
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(|left, right| {
			right
				.updated_at_unix
				.cmp(&left.updated_at_unix)
				.then_with(|| left.objective.id().cmp(right.objective.id()))
				.then_with(|| left.objective.version().cmp(&right.objective.version()))
		});
		records.truncate(limit);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// Persist one read-only autonomy signal against the currently accepted objective version.
	#[allow(dead_code)]
	pub(crate) fn record_autonomy_signal(
		&self,
		project_id: &str,
		signal: AutonomySignal,
	) -> Result<AutonomySignalRecord> {
		validate_autonomy_signal_record_inputs(project_id, &signal)?;

		let now = timestamp_parts();
		let mut state = self.lock()?;
		let objective_key = AutonomyObjectiveKey::new(
			project_id,
			signal.objective_id(),
			signal.objective_version(),
		);
		let objective = state.autonomy_objectives.get(&objective_key).ok_or_else(|| {
			eyre::eyre!(
				"Autonomy signal `{}` references missing objective `{}` version {}.",
				signal.id(),
				signal.objective_id(),
				signal.objective_version()
			)
		})?;

		if objective.state != AutonomyObjectiveState::Accepted {
			eyre::bail!(
				"Autonomy signal `{}` can only be recorded for an accepted objective version; `{}` version {} is `{}`.",
				signal.id(),
				signal.objective_id(),
				signal.objective_version(),
				objective.state.as_str()
			);
		}

		let key = AutonomySignalKey::new(project_id, signal.id());
		let (created_at, created_at_unix) = state.autonomy_signals.get(&key).map_or_else(
			|| (now.text.clone(), now.unix),
			|record| (record.created_at.clone(), record.created_at_unix),
		);
		let record = AutonomySignalRuntimeRecord {
			project_id: project_id.to_owned(),
			signal,
			created_at,
			created_at_unix,
			updated_at: now.text,
			updated_at_unix: now.unix,
		};

		state.autonomy_signals.insert(record.key(), record.clone());
		self.upsert_autonomy_signal_locked(&record)?;

		Ok(record.as_public())
	}

	/// Read one autonomy signal by stable signal id.
	#[allow(dead_code)]
	pub(crate) fn autonomy_signal(
		&self,
		project_id: &str,
		signal_id: &str,
	) -> Result<Option<AutonomySignalRecord>> {
		validate_required_autonomy_signal_field("project_id", project_id)?;
		validate_required_autonomy_signal_field("signal_id", signal_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.autonomy_signal(project_id, signal_id)
				.map(|record| record.map(|record| record.as_public()));
		}

		let state = self.lock()?;

		Ok(state
			.autonomy_signals
			.get(&AutonomySignalKey::new(project_id, signal_id))
			.map(AutonomySignalRuntimeRecord::as_public))
	}

	/// List autonomy signals tied to one exact Objective Contract version.
	#[allow(dead_code)]
	pub(crate) fn list_autonomy_signals_for_objective(
		&self,
		project_id: &str,
		objective_id: &str,
		objective_version: u64,
	) -> Result<Vec<AutonomySignalRecord>> {
		validate_required_autonomy_signal_field("project_id", project_id)?;
		validate_required_autonomy_signal_field("objective_id", objective_id)?;
		validate_autonomy_objective_version(objective_version)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.list_autonomy_signals_for_objective(project_id, objective_id, objective_version)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.autonomy_signals
			.values()
			.filter(|record| {
				record.project_id == project_id
					&& record.signal.objective_id() == objective_id
					&& record.signal.objective_version() == objective_version
			})
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_autonomy_signal_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List recent autonomy signals for one project for operator readback.
	#[allow(dead_code)]
	pub(crate) fn recent_autonomy_signals_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomySignalRecord>> {
		validate_required_autonomy_signal_field("project_id", project_id)?;

		if limit == 0 {
			return Ok(Vec::new());
		}

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.recent_autonomy_signals_for_project(project_id, limit)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.autonomy_signals
			.values()
			.filter(|record| record.project_id == project_id)
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_recent_autonomy_signal_runtime_records);
		records.truncate(limit);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

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
			validate_required_autonomy_proposal_field("signal_id", signal_id)?;

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
		validate_autonomy_proposal_record_inputs(project_id, &proposal)?;

		let now = timestamp_parts();
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
		validate_required_autonomy_proposal_field("project_id", project_id)?;
		validate_required_autonomy_proposal_field("proposal_id", proposal_id)?;

		let now = timestamp_parts();
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
		validate_required_autonomy_proposal_field("project_id", project_id)?;
		validate_required_autonomy_proposal_field("proposal_id", proposal_id)?;

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
		validate_required_autonomy_proposal_field("project_id", project_id)?;
		validate_required_autonomy_proposal_field("proposal_id", proposal_id)?;

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
		validate_required_autonomy_proposal_field("project_id", project_id)?;
		validate_required_autonomy_proposal_field("objective_id", objective_id)?;
		validate_autonomy_objective_version(objective_version)?;

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

		records.sort_by(compare_autonomy_proposal_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List recent autonomy proposals for one project for operator readback.
	#[allow(dead_code)]
	pub(crate) fn recent_autonomy_proposals_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomyProposalRecord>> {
		validate_required_autonomy_proposal_field("project_id", project_id)?;

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

		records.sort_by(compare_recent_autonomy_proposal_runtime_records);
		records.truncate(limit);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// Accept one draft Objective Contract version as immutable runtime authority.
	#[allow(dead_code)]
	pub(crate) fn accept_autonomy_objective_version(
		&self,
		project_id: &str,
		objective_id: &str,
		version: u64,
		acceptance: AutonomyObjectiveAcceptance,
	) -> Result<AutonomyObjectiveRecord> {
		validate_required_autonomy_objective_field("project_id", project_id)?;
		validate_required_autonomy_objective_field("objective_id", objective_id)?;
		validate_autonomy_objective_version(version)?;

		let superseded_by = acceptance.accepted_by().to_owned();
		let superseded_at = acceptance.accepted_at().to_owned();
		let supersession_source = acceptance.acceptance_source().to_owned();
		let now = timestamp_parts();
		let key = AutonomyObjectiveKey::new(project_id, objective_id, version);
		let mut state = self.lock()?;
		let mut record = state.autonomy_objectives.get(&key).cloned().ok_or_else(|| {
			eyre::eyre!("Autonomy objective `{objective_id}` version {version} does not exist.")
		})?;

		if let Some(current_version) = state
			.autonomy_objectives
			.values()
			.filter(|candidate| {
				candidate.project_id == project_id
					&& candidate.objective.id() == objective_id
					&& candidate.state == AutonomyObjectiveState::Accepted
			})
			.map(|candidate| candidate.objective.version())
			.max() && version <= current_version
		{
			eyre::bail!(
				"Autonomy objective `{objective_id}` version {version} must be greater than current accepted version {current_version}."
			);
		}

		record.objective.accept(acceptance)?;

		record.state = record.objective.state();
		record.updated_at = now.text.clone();
		record.updated_at_unix = now.unix;

		let superseded_keys = state
			.autonomy_objectives
			.iter()
			.filter(|(_, candidate)| {
				candidate.project_id == project_id
					&& candidate.objective.id() == objective_id
					&& candidate.state == AutonomyObjectiveState::Accepted
			})
			.map(|(key, _)| key.clone())
			.collect::<Vec<_>>();
		let mut changed_records = Vec::new();

		for superseded_key in superseded_keys {
			let supersession = AutonomyObjectiveSupersession::new(
				objective_id,
				version,
				superseded_by.clone(),
				superseded_at.clone(),
				supersession_source.clone(),
				format!("Accepted objective version {version} superseded this version."),
			)?;
			let mut superseded = state
				.autonomy_objectives
				.get(&superseded_key)
				.cloned()
				.expect("superseded key should exist");

			superseded.objective.supersede(supersession)?;

			superseded.state = superseded.objective.state();
			superseded.updated_at = now.text.clone();
			superseded.updated_at_unix = now.unix;

			state.autonomy_objectives.insert(superseded.key(), superseded.clone());
			changed_records.push(superseded);
		}

		state.autonomy_objectives.insert(key, record.clone());
		changed_records.push(record.clone());

		for changed_record in &changed_records {
			self.upsert_autonomy_objective_locked(changed_record)?;
		}

		Ok(record.as_public())
	}

	/// Reject one draft Objective Contract version with provenance.
	#[allow(dead_code)]
	pub(crate) fn reject_autonomy_objective_version(
		&self,
		project_id: &str,
		objective_id: &str,
		version: u64,
		rejection: AutonomyObjectiveRejection,
	) -> Result<AutonomyObjectiveRecord> {
		self.update_autonomy_objective(project_id, objective_id, version, |objective| {
			objective.reject(rejection)
		})
	}

	/// Supersede one draft or accepted Objective Contract version with provenance.
	#[allow(dead_code)]
	pub(crate) fn supersede_autonomy_objective_version(
		&self,
		project_id: &str,
		objective_id: &str,
		version: u64,
		supersession: AutonomyObjectiveSupersession,
	) -> Result<AutonomyObjectiveRecord> {
		self.update_autonomy_objective(project_id, objective_id, version, |objective| {
			objective.supersede(supersession)
		})
	}

	#[allow(dead_code)]
	pub(super) fn update_autonomy_objective(
		&self,
		project_id: &str,
		objective_id: &str,
		version: u64,
		update: impl FnOnce(&mut AutonomyObjectiveContract) -> Result<()>,
	) -> Result<AutonomyObjectiveRecord> {
		validate_required_autonomy_objective_field("project_id", project_id)?;
		validate_required_autonomy_objective_field("objective_id", objective_id)?;
		validate_autonomy_objective_version(version)?;

		let now = timestamp_parts();
		let key = AutonomyObjectiveKey::new(project_id, objective_id, version);
		let mut state = self.lock()?;
		let mut record = state.autonomy_objectives.get(&key).cloned().ok_or_else(|| {
			eyre::eyre!("Autonomy objective `{objective_id}` version {version} does not exist.")
		})?;

		update(&mut record.objective)?;

		record.objective.validate()?;

		record.state = record.objective.state();
		record.updated_at = now.text;
		record.updated_at_unix = now.unix;

		state.autonomy_objectives.insert(key, record.clone());
		self.upsert_autonomy_objective_locked(&record)?;

		Ok(record.as_public())
	}
}
