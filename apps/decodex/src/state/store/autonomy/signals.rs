use crate::state::{runtime_row_parsers, store};
use crate::{
	autonomy_objective::AutonomyObjectiveState,
	autonomy_signal::AutonomySignal,
	prelude::{Result, eyre},
	state::{
		AutonomySignalRecord, StateStore,
		runtime_records::{AutonomyObjectiveKey, AutonomySignalKey, AutonomySignalRuntimeRecord},
	},
};

impl StateStore {
	/// Persist one read-only autonomy signal against the currently accepted objective version.
	#[allow(dead_code)]
	pub(crate) fn record_autonomy_signal(
		&self,
		project_id: &str,
		signal: AutonomySignal,
	) -> Result<AutonomySignalRecord> {
		store::validate_autonomy_signal_record_inputs(project_id, &signal)?;

		let now = runtime_row_parsers::timestamp_parts();
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
		store::validate_required_autonomy_signal_field("project_id", project_id)?;
		store::validate_required_autonomy_signal_field("signal_id", signal_id)?;

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
		store::validate_required_autonomy_signal_field("project_id", project_id)?;
		store::validate_required_autonomy_signal_field("objective_id", objective_id)?;
		store::validate_autonomy_objective_version(objective_version)?;

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

		records.sort_by(crate::state::store::compare_autonomy_signal_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List recent autonomy signals for one project for operator readback.
	#[allow(dead_code)]
	pub(crate) fn recent_autonomy_signals_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomySignalRecord>> {
		store::validate_required_autonomy_signal_field("project_id", project_id)?;

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

		records.sort_by(crate::state::store::compare_recent_autonomy_signal_runtime_records);
		records.truncate(limit);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}
}
