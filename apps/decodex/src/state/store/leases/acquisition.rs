use std::{
	collections::HashSet,
	fs::{self, OpenOptions, TryLockError},
};

use crate::{
	lane_authority::{LaneCommand, LaneId},
	prelude::{Result, eyre},
	state::{
		runtime_records::GuardRetention,
		store::{self, DispatchSlotGuard, IssueClaimGuard, IssueLease, StateStore},
	},
};

impl StateStore {
	/// Acquire the canonical project-qualified lane claim, then materialize its legacy lease
	/// projection.
	pub fn try_acquire_registered_lease(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		issue_state: &str,
	) -> Result<bool> {
		let lane_id = LaneId::new(project_id, issue_id)?;
		let previous = self.lane(&lane_id)?;
		let binding = match self.registered_project_binding(project_id)? {
			Some(binding) => binding,
			None => {
				#[cfg(not(test))]
				eyre::bail!("Project is not registered; lane claim is forbidden.");
				#[cfg(test)]
				crate::lane_authority::ProjectBinding::new(
					project_id,
					"test-owner",
					"test-repository",
					"team-test",
					&format!("decodex:queued:{project_id}"),
					previous
						.as_ref()
						.map_or_else(
							|| format!("test-binding:{project_id}"),
							|lane| lane.binding_fingerprint().to_owned(),
						)
						.as_str(),
				)?
			},
		};
		self.apply_lane_command(
			lane_id.clone(),
			binding.config_fingerprint(),
			LaneCommand::AcquireClaim { run_id: run_id.to_owned() },
		)?;
		let acquired = self.try_acquire_lease_projection(project_id, issue_id, run_id, issue_state);
		match acquired {
			Ok(true) => Ok(true),
			Ok(false) => {
				if previous.as_ref().and_then(|lane| lane.claim_run_id()).is_none() {
					self.apply_lane_command(
						lane_id,
						binding.config_fingerprint(),
						LaneCommand::ReleaseClaim { run_id: run_id.to_owned() },
					)?;
				}
				Ok(false)
			},
			Err(error) => {
				if previous.as_ref().and_then(|lane| lane.claim_run_id()).is_none() {
					let _ = self.apply_lane_command(
						lane_id,
						binding.config_fingerprint(),
						LaneCommand::ReleaseClaim { run_id: run_id.to_owned() },
					);
				}
				Err(error)
			},
		}
	}

	/// Try to acquire one issue claim plus one shared dispatch slot for one issue.
	#[cfg(test)]
	pub fn try_acquire_lease(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		issue_state: &str,
	) -> Result<bool> {
		self.try_acquire_lease_projection(project_id, issue_id, run_id, issue_state)
	}

	fn try_acquire_lease_projection(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		issue_state: &str,
	) -> Result<bool> {
		let mut state = self.lock()?;

		if state.leases.values().any(|lease| lease.issue_id == issue_id) {
			return Ok(false);
		}

		if let Some(dispatch_slot_config) = state.dispatch_slot_configs.get(project_id).cloned() {
			fs::create_dir_all(&dispatch_slot_config.root)?;

			let _coordinator = store::acquire_shared_lock_coordinator(&dispatch_slot_config.root)?;
			let issue_claim_lock_path =
				store::issue_claim_lock_path(&dispatch_slot_config.root, issue_id);
			let issue_claim_lock_file = OpenOptions::new()
				.read(true)
				.write(true)
				.create(true)
				.truncate(false)
				.open(&issue_claim_lock_path)?;

			match issue_claim_lock_file.try_lock() {
				Ok(()) => {},
				Err(TryLockError::WouldBlock) => return Ok(false),
				Err(TryLockError::Error(error)) => return Err(error.into()),
			}

			let mut issue_claim_guard = IssueClaimGuard {
				lock_path: issue_claim_lock_path,
				lock_file: issue_claim_lock_file,
				retention: GuardRetention::Local,
			};

			store::write_issue_claim_record(
				&mut issue_claim_guard.lock_file,
				project_id,
				issue_id,
				run_id,
				issue_state,
			)?;

			let held_slot_indexes = state
				.dispatch_slot_guards
				.values()
				.filter(|guard| guard.project_id == project_id)
				.map(|guard| guard.slot_index)
				.collect::<HashSet<_>>();
			let mut slot_index = 0;
			let dispatch_slot_guard = loop {
				if held_slot_indexes.contains(&slot_index) {
					slot_index = slot_index
						.checked_add(1)
						.ok_or_else(|| eyre::eyre!("dispatch slot index overflowed usize"))?;

					continue;
				}

				let dispatch_slot_lock_path =
					store::dispatch_slot_lock_path(&dispatch_slot_config.root, slot_index);
				let lock_file = OpenOptions::new()
					.read(true)
					.write(true)
					.create(true)
					.truncate(false)
					.open(&dispatch_slot_lock_path)?;

				match lock_file.try_lock() {
					Ok(()) => {
						break DispatchSlotGuard {
							project_id: project_id.to_owned(),
							slot_index,
							lock_path: dispatch_slot_lock_path,
							lock_file,
							retention: GuardRetention::Local,
						};
					},
					Err(TryLockError::WouldBlock) => {},
					Err(TryLockError::Error(error)) => return Err(error.into()),
				}

				slot_index = slot_index
					.checked_add(1)
					.ok_or_else(|| eyre::eyre!("dispatch slot index overflowed usize"))?;
			};

			state.issue_claim_guards.insert(issue_id.to_owned(), issue_claim_guard);
			state.dispatch_slot_guards.insert(issue_id.to_owned(), dispatch_slot_guard);
		}

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
		self.persist_runtime_state_locked(&state)?;

		Ok(true)
	}
}
