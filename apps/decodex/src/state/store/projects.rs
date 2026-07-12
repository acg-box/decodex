use std::path::Path;

use crate::{
	prelude::{Result, eyre},
	state::store::{
		self, ConnectorBackoff, ConnectorBackoffInput, DispatchSlotConfig, ProjectRegistration,
		StateStore,
	},
};

impl StateStore {
	pub(crate) fn upsert_project(
		&self,
		registration: &ProjectRegistration,
	) -> Result<ProjectRegistration> {
		registration.validate_binding()?;
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_registry_state_locked(&mut state)?;

		let mut registration = registration.clone();

		if let Some(enabled) =
			state.projects.get(registration.service_id()).map(ProjectRegistration::enabled)
			&& registration.enabled() != enabled
		{
			registration.set_enabled(enabled);
		}

		state.projects.insert(registration.service_id().to_owned(), registration.clone());
		self.upsert_project_locked(&registration)?;

		Ok(registration)
	}

	/// List all registered projects known to this local Decodex installation.
	pub(crate) fn list_projects(&self) -> Result<Vec<ProjectRegistration>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_registry_state_locked(&mut state)?;

		let mut projects = state.projects.values().cloned().collect::<Vec<_>>();

		projects.sort_by(|left, right| left.service_id().cmp(right.service_id()));

		Ok(projects)
	}

	/// Remove one registered project from the local control-plane registry.
	pub(crate) fn remove_project(&self, service_id: &str) -> Result<ProjectRegistration> {
		let mut state = self.lock()?;
		let removed = state
			.projects
			.remove(service_id)
			.ok_or_else(|| eyre::eyre!("Decodex project `{service_id}` is not registered."))?;

		self.persist_runtime_state_locked(&state)?;
		self.delete_project_locked(service_id)?;

		Ok(removed)
	}

	/// Enable or disable one registered project.
	pub(crate) fn set_project_enabled(&self, service_id: &str, enabled: bool) -> Result<()> {
		let mut state = self.lock()?;
		let project = state
			.projects
			.get_mut(service_id)
			.ok_or_else(|| eyre::eyre!("Decodex project `{service_id}` is not registered."))?;

		project.set_enabled(enabled);

		self.persist_runtime_state_locked(&state)
	}

	/// Create or replace a project-scoped external connector backoff.
	pub(crate) fn upsert_connector_backoff(
		&self,
		input: ConnectorBackoffInput<'_>,
	) -> Result<ConnectorBackoff> {
		let now = store::timestamp_parts();
		let record = ConnectorBackoff {
			project_id: input.project_id.to_owned(),
			connector: input.connector.to_owned(),
			sync_phase: input.sync_phase.to_owned(),
			quota_class: input.quota_class.to_owned(),
			reset_unix_epoch: input.reset_unix_epoch,
			reset_source: input.reset_source.to_owned(),
			warning: input.warning.to_owned(),
			updated_at: now.text,
			updated_at_unix: now.unix,
		};
		let mut state = self.lock()?;

		state
			.connector_backoffs
			.insert((input.project_id.to_owned(), input.connector.to_owned()), record.clone());
		self.persist_runtime_state_locked(&state)?;

		Ok(record)
	}

	/// Read a project-scoped connector backoff from the runtime store.
	pub(crate) fn connector_backoff(
		&self,
		project_id: &str,
		connector: &str,
	) -> Result<Option<ConnectorBackoff>> {
		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite.connector_backoff(project_id, connector);
		}

		let state = self.lock()?;

		Ok(state.connector_backoffs.get(&(project_id.to_owned(), connector.to_owned())).cloned())
	}

	/// Clear a project-scoped connector backoff from the runtime store.
	pub(crate) fn clear_connector_backoff(&self, project_id: &str, connector: &str) -> Result<()> {
		let mut state = self.lock()?;

		state.connector_backoffs.remove(&(project_id.to_owned(), connector.to_owned()));

		self.delete_connector_backoff_locked(project_id, connector)
	}

	/// Configure the shared cross-process dispatch-slot root for one project.
	pub(crate) fn configure_dispatch_slot_root(
		&self,
		project_id: &str,
		worktree_root: impl AsRef<Path>,
	) -> Result<()> {
		let worktree_root = worktree_root.as_ref().to_path_buf();
		let mut state = self.lock_without_refresh()?;

		if state.issue_claim_guards.is_empty() && state.dispatch_slot_guards.is_empty() {
			store::prune_unlocked_shared_lock_files(&worktree_root)?;
		}

		state
			.dispatch_slot_configs
			.insert(project_id.to_owned(), DispatchSlotConfig { root: worktree_root });

		Ok(())
	}

	/// Observe the shared cross-process dispatch-slot root without pruning lock files.
	pub(crate) fn observe_dispatch_slot_root(
		&self,
		project_id: &str,
		worktree_root: impl AsRef<Path>,
	) -> Result<()> {
		let worktree_root = worktree_root.as_ref().to_path_buf();
		let mut state = self.lock_without_refresh()?;

		state
			.dispatch_slot_configs
			.insert(project_id.to_owned(), DispatchSlotConfig { root: worktree_root });

		Ok(())
	}
}
