use crate::state::{runtime_row_parsers, store};
use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveContract, AutonomyObjectiveRejection,
		AutonomyObjectiveState, AutonomyObjectiveSupersession,
	},
	prelude::{Result, eyre},
	state::{AutonomyObjectiveRecord, StateStore, runtime_records::AutonomyObjectiveKey},
};

impl StateStore {
	/// Accept one draft Objective Contract version as immutable runtime authority.
	#[allow(dead_code)]
	pub(crate) fn accept_autonomy_objective_version(
		&self,
		project_id: &str,
		objective_id: &str,
		version: u64,
		acceptance: AutonomyObjectiveAcceptance,
	) -> Result<AutonomyObjectiveRecord> {
		store::validate_required_autonomy_objective_field("project_id", project_id)?;
		store::validate_required_autonomy_objective_field("objective_id", objective_id)?;
		store::validate_autonomy_objective_version(version)?;

		let superseded_by = acceptance.accepted_by().to_owned();
		let superseded_at = acceptance.accepted_at().to_owned();
		let supersession_source = acceptance.acceptance_source().to_owned();
		let now = runtime_row_parsers::timestamp_parts();
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
		store::validate_required_autonomy_objective_field("project_id", project_id)?;
		store::validate_required_autonomy_objective_field("objective_id", objective_id)?;
		store::validate_autonomy_objective_version(version)?;

		let now = runtime_row_parsers::timestamp_parts();
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
