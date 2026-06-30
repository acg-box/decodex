#[cfg(unix)] use std::os::fd::FromRawFd;
use std::{
	collections::{HashMap, HashSet},
	fs::{self, File, OpenOptions, TryLockError},
	io::ErrorKind,
};

use crate::prelude::{Result, eyre};

use super::{
	super::runtime_records::GuardRetention, DispatchSlotGuard, IssueClaimGuard, IssueLease,
	PreacquiredLeaseGuards, StateStore, acquire_shared_lock_coordinator, clear_close_on_exec,
	dispatch_slot_lock_path, issue_claim_id_from_path, issue_claim_lock_path,
	read_issue_claim_record, remove_lock_file_if_exists, retarget_evidence_artifact_issue,
	retarget_loop_guardrail_issue, retarget_review_lifecycle_issue, retarget_review_policy_issue,
	set_close_on_exec, write_issue_claim_record,
};

impl StateStore {
	/// Retarget runtime records from a visible issue identifier to the canonical tracker id.
	pub fn canonicalize_issue_identity(
		&self,
		previous_issue_id: &str,
		canonical_issue_id: &str,
	) -> Result<()> {
		if previous_issue_id == canonical_issue_id {
			return Ok(());
		}

		let mut state = self.lock_without_refresh()?;

		if let Some(mut lease) = state.leases.remove(previous_issue_id) {
			lease.issue_id = canonical_issue_id.to_owned();

			state.leases.entry(canonical_issue_id.to_owned()).or_insert(lease);
		}
		if let Some(mut mapping) = state.worktrees.remove(previous_issue_id) {
			mapping.issue_id = canonical_issue_id.to_owned();

			state.worktrees.entry(canonical_issue_id.to_owned()).or_insert(mapping);
		}

		retarget_review_lifecycle_issue(
			&mut state.review_lifecycle_records,
			previous_issue_id,
			canonical_issue_id,
		);
		retarget_review_policy_issue(
			&mut state.review_policy_checkpoints,
			previous_issue_id,
			canonical_issue_id,
		);
		retarget_evidence_artifact_issue(
			&mut state.evidence_artifacts,
			previous_issue_id,
			canonical_issue_id,
		);
		retarget_loop_guardrail_issue(
			&mut state.loop_guardrail_checkpoints,
			previous_issue_id,
			canonical_issue_id,
		);

		if let Some(guard) = state.issue_claim_guards.remove(previous_issue_id) {
			state.issue_claim_guards.entry(canonical_issue_id.to_owned()).or_insert(guard);
		}
		if let Some(guard) = state.dispatch_slot_guards.remove(previous_issue_id) {
			state.dispatch_slot_guards.entry(canonical_issue_id.to_owned()).or_insert(guard);
		}

		for attempt in
			state.run_attempts.values_mut().filter(|attempt| attempt.issue_id == previous_issue_id)
		{
			attempt.issue_id = canonical_issue_id.to_owned();
		}
		for channel in state
			.control_channels
			.values_mut()
			.filter(|channel| channel.issue_id == previous_issue_id)
		{
			channel.issue_id = canonical_issue_id.to_owned();
		}
		for record in state
			.private_execution_events
			.iter_mut()
			.filter(|record| record.issue_id == previous_issue_id)
		{
			record.issue_id = canonical_issue_id.to_owned();
		}
		for record in state
			.decision_contracts
			.values_mut()
			.filter(|record| record.source_issue_id.as_deref() == Some(previous_issue_id))
		{
			record.source_issue_id = Some(canonical_issue_id.to_owned());
		}
		for record in state
			.program_issue_mappings
			.values_mut()
			.filter(|record| record.issue_id == previous_issue_id)
		{
			record.issue_id = canonical_issue_id.to_owned();
		}

		self.retarget_issue_identity_locked(previous_issue_id, canonical_issue_id)
	}

	/// Create or replace the run lease for one issue.
	pub fn upsert_lease(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		issue_state: &str,
	) -> Result<()> {
		let lease = IssueLease {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			run_id: run_id.to_owned(),
			issue_state: issue_state.to_owned(),
		};
		let mut state = self.lock_without_refresh()?;

		state.leases.insert(issue_id.to_owned(), lease.clone());
		state.remember_run_project(project_id, issue_id, Some(run_id));

		self.upsert_lease_and_remember_run_project_locked(&lease)
	}

	/// Try to acquire one issue claim plus one shared dispatch slot for one issue.
	pub fn try_acquire_lease(
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

			let _coordinator = acquire_shared_lock_coordinator(&dispatch_slot_config.root)?;
			let issue_claim_lock_path = issue_claim_lock_path(&dispatch_slot_config.root, issue_id);
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

			write_issue_claim_record(
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
					dispatch_slot_lock_path(&dispatch_slot_config.root, slot_index);
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
		let _coordinator = acquire_shared_lock_coordinator(&dispatch_slot_config.root)?;
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
			let Some(issue_id) = issue_claim_id_from_path(&path) else {
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
					remove_lock_file_if_exists(&path)?;
				},
				Err(TryLockError::WouldBlock) => {
					if let Some(lease) = read_issue_claim_record(&path)?
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

		let path = issue_claim_lock_path(&dispatch_slot_config.root, issue_id);
		let _coordinator = acquire_shared_lock_coordinator(&dispatch_slot_config.root)?;
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

		let path = issue_claim_lock_path(&dispatch_slot_config.root, issue_id);
		let _coordinator = acquire_shared_lock_coordinator(&dispatch_slot_config.root)?;
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
					remove_lock_file_if_exists(&path)?;
				}

				Ok(false)
			},
			Err(TryLockError::WouldBlock) => Ok(true),
			Err(TryLockError::Error(error)) => Err(error.into()),
		}
	}

	/// Remove the run lease for one issue.
	pub fn clear_lease(&self, issue_id: &str) -> Result<()> {
		let mut state = self.lock()?;
		let _coordinator = match (
			state.issue_claim_guards.get(issue_id),
			state.dispatch_slot_guards.get(issue_id),
		) {
			(Some(guard), _) => Some(acquire_shared_lock_coordinator(guard.lock_root()?)?),
			(None, Some(guard)) => Some(acquire_shared_lock_coordinator(guard.lock_root()?)?),
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
	pub fn release_dispatch_slot(&self, issue_id: &str) -> Result<()> {
		let mut state = self.lock()?;

		if let Some(guard) = state.dispatch_slot_guards.get(issue_id) {
			let _coordinator = acquire_shared_lock_coordinator(guard.lock_root()?)?;
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

		clear_close_on_exec(&child_lock)?;

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

		clear_close_on_exec(&child_lock)?;

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

		set_close_on_exec(&issue_claim_lock_file)?;
		set_close_on_exec(&lock_file)?;

		let mut state = self.lock()?;
		let dispatch_slot_config =
			state.dispatch_slot_configs.get(project_id).cloned().ok_or_else(|| {
				eyre::eyre!("project `{project_id}` has no shared dispatch-slot root")
			})?;

		state.issue_claim_guards.insert(
			issue_id.to_owned(),
			IssueClaimGuard {
				lock_path: issue_claim_lock_path(&dispatch_slot_config.root, issue_id),
				lock_file: issue_claim_lock_file,
				retention: GuardRetention::AdoptingChild,
			},
		);
		state.dispatch_slot_guards.insert(
			issue_id.to_owned(),
			DispatchSlotGuard {
				project_id: project_id.to_owned(),
				slot_index: guards.dispatch_slot_index,
				lock_path: dispatch_slot_lock_path(
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
