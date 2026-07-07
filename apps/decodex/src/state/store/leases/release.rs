use std::fs::File;
#[cfg(unix)]
use std::os::fd::FromRawFd;

use crate::{
	prelude::{Result, eyre},
	state::{
		runtime_records::GuardRetention,
		store::{
			self, DispatchSlotGuard, IssueClaimGuard, IssueLease, PreacquiredLeaseGuards,
			StateStore,
		},
	},
};

impl StateStore {
	/// Remove the run lease for one issue.
	pub fn clear_lease(&self, issue_id: &str) -> Result<()> {
		let mut state = self.lock()?;
		let _coordinator = match (
			state.issue_claim_guards.get(issue_id),
			state.dispatch_slot_guards.get(issue_id),
		) {
			(Some(guard), _) => Some(store::acquire_shared_lock_coordinator(guard.lock_root()?)?),
			(None, Some(guard)) => {
				Some(store::acquire_shared_lock_coordinator(guard.lock_root()?)?)
			},
			(None, None) => None,
		};
		let removed_lease = state.leases.remove(issue_id).is_some();
		let issue_claim_guard = state.issue_claim_guards.remove(issue_id);
		let dispatch_slot_guard = state.dispatch_slot_guards.remove(issue_id);

		if let Some(guard) = issue_claim_guard {
			guard.release_for_clear()?;
		}
		if let Some(guard) = dispatch_slot_guard {
			guard.release_for_clear()?;
		}

		if removed_lease {
			self.persist_runtime_state_locked(&state)?;
		}

		self.delete_lease_locked(issue_id)
	}

	/// Drop the current process-local dispatch-slot guard while keeping the local lease record.
	#[cfg(test)]
	pub fn release_dispatch_slot(&self, issue_id: &str) -> Result<()> {
		let mut state = self.lock()?;

		if let Some(guard) = state.dispatch_slot_guards.get(issue_id) {
			let _coordinator = store::acquire_shared_lock_coordinator(guard.lock_root()?)?;
			let guard = state
				.dispatch_slot_guards
				.remove(issue_id)
				.ok_or_else(|| eyre::eyre!("issue `{issue_id}` lost its dispatch-slot guard"))?;

			guard.release_for_clear()?;
		}

		Ok(())
	}

	/// Drop process-local lock guards after another process inherited them.
	pub fn release_handed_off_guards(&self, issue_id: &str) -> Result<()> {
		let mut state = self.lock()?;

		state.issue_claim_guards.remove(issue_id);
		state.dispatch_slot_guards.remove(issue_id);

		Ok(())
	}

	/// Duplicate the held dispatch-slot lock so a spawned child can inherit it across exec.
	#[cfg(unix)]
	pub fn clone_issue_claim_for_child(&self, issue_id: &str) -> Result<File> {
		let mut state = self.lock()?;
		let guard = state
			.issue_claim_guards
			.get_mut(issue_id)
			.ok_or_else(|| eyre::eyre!("issue `{issue_id}` does not hold an issue-claim guard"))?;

		guard.retention = GuardRetention::ParentAfterHandoff;

		let child_lock = guard.lock_file.try_clone()?;

		store::clear_close_on_exec(&child_lock)?;

		Ok(child_lock)
	}

	/// Duplicate the held dispatch-slot lock so a spawned child can inherit it across exec.
	#[cfg(unix)]
	pub fn clone_dispatch_slot_for_child(&self, issue_id: &str) -> Result<(File, usize)> {
		let mut state = self.lock()?;
		let guard = state
			.dispatch_slot_guards
			.get_mut(issue_id)
			.ok_or_else(|| eyre::eyre!("issue `{issue_id}` does not hold a dispatch-slot guard"))?;

		guard.retention = GuardRetention::ParentAfterHandoff;

		let child_lock = guard.lock_file.try_clone()?;

		store::clear_close_on_exec(&child_lock)?;

		Ok((child_lock, guard.slot_index))
	}

	/// Adopt an inherited dispatch-slot fd and local lease for a daemon child process.
	#[cfg(unix)]
	pub fn adopt_preacquired_lease(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		issue_state: &str,
		guards: PreacquiredLeaseGuards,
	) -> Result<()> {
		let issue_claim_lock_file = unsafe { File::from_raw_fd(guards.issue_claim_fd) };
		let lock_file = unsafe { File::from_raw_fd(guards.dispatch_slot_fd) };

		store::set_close_on_exec(&issue_claim_lock_file)?;
		store::set_close_on_exec(&lock_file)?;

		let mut state = self.lock()?;
		let dispatch_slot_config =
			state.dispatch_slot_configs.get(project_id).cloned().ok_or_else(|| {
				eyre::eyre!("project `{project_id}` has no shared dispatch-slot root")
			})?;

		state.issue_claim_guards.insert(
			issue_id.to_owned(),
			IssueClaimGuard {
				lock_path: store::issue_claim_lock_path(&dispatch_slot_config.root, issue_id),
				lock_file: issue_claim_lock_file,
				retention: GuardRetention::AdoptingChild,
			},
		);
		state.dispatch_slot_guards.insert(
			issue_id.to_owned(),
			DispatchSlotGuard {
				project_id: project_id.to_owned(),
				slot_index: guards.dispatch_slot_index,
				lock_path: store::dispatch_slot_lock_path(
					&dispatch_slot_config.root,
					guards.dispatch_slot_index,
				),
				lock_file,
				retention: GuardRetention::AdoptingChild,
			},
		);
		state.leases.insert(
			issue_id.to_owned(),
			IssueLease {
				project_id: project_id.to_owned(),
				issue_id: issue_id.to_owned(),
				run_id: run_id.to_owned(),
				issue_state: issue_state.to_owned(),
			},
		);
		state.remember_run_project(project_id, issue_id, Some(run_id));

		self.persist_runtime_state_locked(&state)
	}
}
