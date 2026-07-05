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
			return Ok(sorted_leases(leases_by_issue));
		};
		let _coordinator = store::acquire_shared_lock_coordinator(&dispatch_slot_config.root)?;
		let read_dir = match fs::read_dir(&dispatch_slot_config.root) {
			Ok(read_dir) => read_dir,
			Err(error) if error.kind() == ErrorKind::NotFound => {
				return Ok(sorted_leases(leases_by_issue));
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

		Ok(sorted_leases(leases_by_issue))
	}
}

fn sorted_leases(leases_by_issue: HashMap<String, IssueLease>) -> Vec<IssueLease> {
	let mut leases = leases_by_issue.into_values().collect::<Vec<_>>();

	leases.sort_by(|left, right| left.issue_id.cmp(&right.issue_id));

	leases
}
