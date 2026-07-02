use crate::{
	prelude::{Result, eyre},
	state::store::{ProjectRegistration, StateData, StateStore},
};

impl StateStore {
	pub(in crate::state) fn persist_runtime_state_locked(&self, state: &StateData) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.persist_runtime_state(state)
	}

	pub(in crate::state) fn delete_project_locked(&self, service_id: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_project(service_id)
	}

	pub(in crate::state) fn upsert_project_locked(
		&self,
		project: &ProjectRegistration,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_project(project)
	}

	pub(in crate::state) fn delete_connector_backoff_locked(
		&self,
		project_id: &str,
		connector: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_connector_backoff(project_id, connector)
	}
}
