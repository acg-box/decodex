use std::{
	collections::HashMap,
	fs::{self, OpenOptions, TryLockError},
	io::ErrorKind,
};

use crate::{
	prelude::Result,
	state::{IssueLease, StateStore, store},
};

impl StateStore {
	/// Read the run lease for one issue.
	pub fn lease_for_issue(&self, issue_id: &str) -> Result<Option<IssueLease>> {
		let state = self.lock()?;

		Ok(state.leases.get(issue_id).cloned())
	}

	/// List all run leases.
	pub fn list_leases(&self, project_id: &str) -> Result<Vec<IssueLease>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_metadata_state_locked(&mut state, project_id)?;

		let mut leases = state
			.leases
			.values()
			.filter(|lease| lease.project_id == project_id)
			.cloned()
			.collect::<Vec<_>>();

		leases.sort_by(|left, right| left.issue_id.cmp(&right.issue_id));

		Ok(leases)
	}

	/// List all active shared leases by combining local claims with other processes' issue claims.
	pub fn list_active_shared_leases(&self, project_id: &str) -> Result<Vec<IssueLease>> {
		let (mut leases_by_issue, dispatch_slot_config) = {
			let mut state = self.lock_without_refresh()?;

			self.refresh_project_run_metadata_state_locked(&mut state, project_id)?;

			let leases = state
				.leases
				.values()
				.filter(|lease| lease.project_id == project_id)
				.cloned()
				.map(|lease| (lease.issue_id.clone(), lease))
				.collect::<HashMap<_, _>>();

			(leases, state.dispatch_slot_configs.get(project_id).cloned())
		};
		let Some(dispatch_slot_config) = dispatch_slot_config else {
			let mut leases = leases_by_issue.into_values().collect::<Vec<_>>();

			leases.sort_by(|left, right| left.issue_id.cmp(&right.issue_id));

			return Ok(leases);
		};
		let _coordinator = store::acquire_shared_lock_coordinator(&dispatch_slot_config.root)?;
		let read_dir = match fs::read_dir(&dispatch_slot_config.root) {
			Ok(read_dir) => read_dir,
			Err(error) if error.kind() == ErrorKind::NotFound => {
				let mut leases = leases_by_issue.into_values().collect::<Vec<_>>();

				leases.sort_by(|left, right| left.issue_id.cmp(&right.issue_id));

				return Ok(leases);
			},
			Err(error) => return Err(error.into()),
		};

		for entry in read_dir {
			let entry = entry?;
			let path = entry.path();
			let Some(issue_id) = store::issue_claim_id_from_path(&path) else {
				continue;
			};

			if leases_by_issue.contains_key(&issue_id) {
				continue;
			}

			let claim_lock_file = match OpenOptions::new()
				.read(true)
				.write(true)
				.create(false)
				.truncate(false)
				.open(&path)
			{
				Ok(file) => file,
				Err(error) if error.kind() == ErrorKind::NotFound => continue,
				Err(error) => return Err(error.into()),
			};

			match claim_lock_file.try_lock() {
				Ok(()) => {
					claim_lock_file.unlock()?;

					drop(claim_lock_file);

					store::remove_lock_file_if_exists(&path)?;
				},
				Err(TryLockError::WouldBlock) => {
					if let Some(lease) = store::read_issue_claim_record(&path)?
						&& lease.project_id == project_id
					{
						leases_by_issue.insert(issue_id, lease);
					}
				},
				Err(TryLockError::Error(error)) => return Err(error.into()),
			}
		}

		let mut leases = leases_by_issue.into_values().collect::<Vec<_>>();

		leases.sort_by(|left, right| left.issue_id.cmp(&right.issue_id));

		Ok(leases)
	}

	/// Report whether one issue is actively claimed by this or another process.
	pub fn issue_has_active_shared_claim(&self, project_id: &str, issue_id: &str) -> Result<bool> {
		self.issue_has_active_shared_claim_with_cleanup(project_id, issue_id, true)
	}

	/// Report whether one issue is actively claimed without deleting stale claim files.
	pub(crate) fn issue_has_active_shared_claim_read_only(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<bool> {
		self.issue_has_active_shared_claim_with_cleanup(project_id, issue_id, false)
	}

	/// Report whether another process actively holds the shared issue claim.
	pub(crate) fn issue_has_external_shared_claim_read_only(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<bool> {
		let state = self.lock_without_refresh()?;
		let Some(dispatch_slot_config) = state.dispatch_slot_configs.get(project_id).cloned()
		else {
			return Ok(false);
		};

		drop(state);

		let path = store::issue_claim_lock_path(&dispatch_slot_config.root, issue_id);
		let _coordinator = store::acquire_shared_lock_coordinator(&dispatch_slot_config.root)?;
		let claim_lock_file = match OpenOptions::new()
			.read(true)
			.write(true)
			.create(false)
			.truncate(false)
			.open(&path)
		{
			Ok(file) => file,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
			Err(error) => return Err(error.into()),
		};

		match claim_lock_file.try_lock() {
			Ok(()) => {
				claim_lock_file.unlock()?;

				Ok(false)
			},
			Err(TryLockError::WouldBlock) => Ok(true),
			Err(TryLockError::Error(error)) => Err(error.into()),
		}
	}

	pub(super) fn issue_has_active_shared_claim_with_cleanup(
		&self,
		project_id: &str,
		issue_id: &str,
		cleanup_unlocked_claim: bool,
	) -> Result<bool> {
		let state = self.lock_without_refresh()?;

		if state.leases.contains_key(issue_id) {
			return Ok(true);
		}

		let Some(dispatch_slot_config) = state.dispatch_slot_configs.get(project_id).cloned()
		else {
			return Ok(false);
		};

		drop(state);

		let path = store::issue_claim_lock_path(&dispatch_slot_config.root, issue_id);
		let _coordinator = store::acquire_shared_lock_coordinator(&dispatch_slot_config.root)?;
		let claim_lock_file = match OpenOptions::new()
			.read(true)
			.write(true)
			.create(false)
			.truncate(false)
			.open(&path)
		{
			Ok(file) => file,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
			Err(error) => return Err(error.into()),
		};

		match claim_lock_file.try_lock() {
			Ok(()) => {
				claim_lock_file.unlock()?;

				if cleanup_unlocked_claim {
					drop(claim_lock_file);

					store::remove_lock_file_if_exists(&path)?;
				}

				Ok(false)
			},
			Err(TryLockError::WouldBlock) => Ok(true),
			Err(TryLockError::Error(error)) => Err(error.into()),
		}
	}
}
