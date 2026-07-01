use crate::state::{runtime_row_parsers, store};
use crate::{
	autonomy_objective::{AutonomyObjectiveContract, AutonomyObjectiveState},
	prelude::{Result, eyre},
	state::{
		AutonomyObjectiveRecord, StateStore,
		runtime_records::{AutonomyObjectiveKey, AutonomyObjectiveRuntimeRecord},
	},
};

impl StateStore {
	/// Create or replace one draft Objective Contract authority payload.
	#[allow(dead_code)]
	pub(crate) fn upsert_autonomy_objective_draft(
		&self,
		project_id: &str,
		objective: AutonomyObjectiveContract,
	) -> Result<AutonomyObjectiveRecord> {
		store::validate_autonomy_objective_record_inputs(project_id, &objective)?;

		if objective.state() != AutonomyObjectiveState::Draft {
			eyre::bail!("Autonomy objective drafts must be stored with state `draft`.");
		}

		let now = runtime_row_parsers::timestamp_parts();
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
		store::validate_required_autonomy_objective_field("project_id", project_id)?;
		store::validate_required_autonomy_objective_field("objective_id", objective_id)?;
		store::validate_autonomy_objective_version(version)?;

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
		store::validate_required_autonomy_objective_field("project_id", project_id)?;
		store::validate_required_autonomy_objective_field("objective_id", objective_id)?;

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
		store::validate_required_autonomy_objective_field("project_id", project_id)?;
		store::validate_required_autonomy_objective_field("objective_id", objective_id)?;

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
		store::validate_required_autonomy_objective_field("project_id", project_id)?;

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
}
