use std::{
	fs::{OpenOptions, TryLockError},
	io::ErrorKind,
};

use crate::{
	prelude::Result,
	state::{StateStore, store},
};

impl StateStore {
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
