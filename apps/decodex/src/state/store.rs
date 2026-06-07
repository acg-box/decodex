use crate::workflow::WorkflowConcurrencyLimit;

/// Input fields for recording a project-scoped external connector backoff.
pub(crate) struct ConnectorBackoffInput<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) connector: &'a str,
	pub(crate) sync_phase: &'a str,
	pub(crate) quota_class: &'a str,
	pub(crate) reset_unix_epoch: i64,
	pub(crate) reset_source: &'a str,
	pub(crate) warning: &'a str,
}

/// Input fields for recording the latest review-policy checkpoint.
pub(crate) struct ReviewPolicyCheckpointInput<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) phase: &'a str,
	pub(crate) status: &'a str,
	pub(crate) head_sha: &'a str,
	pub(crate) nonclean_rounds: i64,
}

/// Local runtime store for leases, attempts, worktrees, protocol events, and private evidence.
#[derive(Default)]
pub struct StateStore {
	inner: Mutex<StateData>,
	sqlite: Option<Mutex<SqliteStateStore>>,
}
impl StateStore {
	/// Open the local persistent runtime store.
	pub fn open(path: impl AsRef<Path>) -> Result<Self> {
		let sqlite = SqliteStateStore::open(path.as_ref())?;
		let state = sqlite.load_state()?;

		Ok(Self { inner: Mutex::new(state), sqlite: Some(Mutex::new(sqlite)) })
	}

	/// Open an in-memory runtime store for tests.
	pub fn open_in_memory() -> Result<Self> {
		Ok(Self::default())
	}

	/// Create or refresh a registered project row in the local control-plane registry.
	///
	/// Project refreshes preserve an existing enablement toggle. Use
	/// [`StateStore::set_project_enabled`] for explicit operator enable/disable changes.
	pub(crate) fn upsert_project(
		&self,
		registration: &ProjectRegistration,
	) -> Result<ProjectRegistration> {
		let mut state = self.lock()?;
		let mut registration = registration.clone();

		if let Some(enabled) = state.projects.get(registration.service_id()).map(ProjectRegistration::enabled)
			&& registration.enabled() != enabled
		{
			registration.set_enabled(enabled);
		}

		state
			.projects
			.insert(registration.service_id().to_owned(), registration.clone());
		self.persist_runtime_state_locked(&state)?;

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
		let now = timestamp_parts();
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

		state.connector_backoffs.insert(
			(input.project_id.to_owned(), input.connector.to_owned()),
			record.clone(),
		);
		self.persist_runtime_state_locked(&state)?;

		Ok(record)
	}

	/// Read a project-scoped connector backoff from the runtime store.
	pub(crate) fn connector_backoff(
		&self,
		project_id: &str,
		connector: &str,
	) -> Result<Option<ConnectorBackoff>> {
		let state = self.lock()?;

		Ok(state
			.connector_backoffs
			.get(&(project_id.to_owned(), connector.to_owned()))
			.cloned())
	}

	/// Clear a project-scoped connector backoff from the runtime store.
	pub(crate) fn clear_connector_backoff(
		&self,
		project_id: &str,
		connector: &str,
	) -> Result<()> {
		let mut state = self.lock()?;

		state.connector_backoffs.remove(&(project_id.to_owned(), connector.to_owned()));

		self.delete_connector_backoff_locked(project_id, connector)
	}

	/// Configure the shared cross-process dispatch-slot root for one project.
	pub(crate) fn configure_dispatch_slot_root(
		&self,
		project_id: &str,
		worktree_root: impl AsRef<Path>,
		slot_limit: impl Into<DispatchSlotLimit>,
	) -> Result<()> {
		let slot_limit = slot_limit.into();
		let worktree_root = worktree_root.as_ref().to_path_buf();
		let mut state = self.lock()?;

		slot_limit.validate()?;

		if state.issue_claim_guards.is_empty() && state.dispatch_slot_guards.is_empty() {
			prune_unlocked_shared_lock_files(&worktree_root)?;
		}

		state.dispatch_slot_configs.insert(
			project_id.to_owned(),
			DispatchSlotConfig { root: worktree_root, slot_limit },
		);

		Ok(())
	}

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

		retarget_review_handoff_issue(
			&mut state.review_handoffs,
			previous_issue_id,
			canonical_issue_id,
		);
		retarget_review_orchestration_issue(
			&mut state.review_orchestrations,
			previous_issue_id,
			canonical_issue_id,
		);
		retarget_review_policy_issue(
			&mut state.review_policy_checkpoints,
			previous_issue_id,
			canonical_issue_id,
		);

		if let Some(guard) = state.issue_claim_guards.remove(previous_issue_id) {
			state.issue_claim_guards.entry(canonical_issue_id.to_owned()).or_insert(guard);
		}
		if let Some(guard) = state.dispatch_slot_guards.remove(previous_issue_id) {
			state.dispatch_slot_guards.entry(canonical_issue_id.to_owned()).or_insert(guard);
		}

		for attempt in state
			.run_attempts
			.values_mut()
			.filter(|attempt| attempt.issue_id == previous_issue_id)
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

		self.retarget_issue_identity_locked(previous_issue_id, canonical_issue_id)
	}

	/// Create or replace the active lease for one issue.
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
			let issue_claim_lock_path =
				issue_claim_lock_path(&dispatch_slot_config.root, issue_id);
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
			let mut acquired_guard = None;
			let mut slot_index = 0;

			while dispatch_slot_config.slot_limit.includes(slot_index)? {
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
						acquired_guard = Some(DispatchSlotGuard {
							project_id: project_id.to_owned(),
							slot_index,
							lock_path: dispatch_slot_lock_path,
							lock_file,
							retention: GuardRetention::Local,
						});

						break;
					},
					Err(TryLockError::WouldBlock) => {},
					Err(TryLockError::Error(error)) => return Err(error.into()),
				}

				slot_index = slot_index
					.checked_add(1)
					.ok_or_else(|| eyre::eyre!("dispatch slot index overflowed usize"))?;
			}

			let Some(dispatch_slot_guard) = acquired_guard else {
				issue_claim_guard.unlock()?;

				return Ok(false);
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

	/// Read the active lease for one issue.
	pub fn lease_for_issue(&self, issue_id: &str) -> Result<Option<IssueLease>> {
		let state = self.lock()?;

		Ok(state.leases.get(issue_id).cloned())
	}

	/// List all active leases.
	pub fn list_leases(&self, project_id: &str) -> Result<Vec<IssueLease>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_state_locked(&mut state, project_id)?;

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

			self.refresh_project_run_state_locked(&mut state, project_id)?;

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

				drop(claim_lock_file);
				remove_lock_file_if_exists(&path)?;

				Ok(false)
			},
			Err(TryLockError::WouldBlock) => Ok(true),
			Err(TryLockError::Error(error)) => Err(error.into()),
		}
	}

	/// Remove the active lease for one issue.
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
		let dispatch_slot_config = state
			.dispatch_slot_configs
			.get(project_id)
			.cloned()
			.ok_or_else(|| eyre::eyre!("project `{project_id}` has no shared dispatch-slot root"))?;

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

	/// Insert or update a run attempt record.
	pub fn record_run_attempt(
		&self,
		run_id: &str,
		issue_id: &str,
		attempt_number: i64,
		status: &str,
	) -> Result<()> {
		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let project_id = state.project_id_for_run(issue_id, run_id);

		match state.run_attempts.get_mut(run_id) {
			Some(existing) => {
				let retained_project_id =
					(existing.issue_id == issue_id).then(|| existing.project_id.clone()).flatten();

				existing.issue_id = issue_id.to_owned();
				existing.project_id = project_id.or(retained_project_id);
				existing.attempt_number = attempt_number;
				existing.status = status.to_owned();
				existing.updated_at = now.text.clone();
				existing.updated_at_unix = now.unix;
			},
			None => {
				state.run_attempts.insert(
					run_id.to_owned(),
					RunAttemptRecord {
						run_id: run_id.to_owned(),
						project_id,
						issue_id: issue_id.to_owned(),
						attempt_number,
						status: status.to_owned(),
						thread_id: None,
						turn_id: None,
						updated_at: now.text,
						updated_at_unix: now.unix,
					},
				);
			},
		}

		let attempt = state
			.run_attempts
			.get(run_id)
			.ok_or_else(|| eyre::eyre!("Run attempt `{run_id}` was not recorded."))?
			.clone();

		self.upsert_run_attempt_locked(&attempt)
	}

	/// Publish the local control channel for an active attempt when the runtime owns it.
	pub(crate) fn publish_run_control_channel_for_active_attempt(
		&self,
		run_id: &str,
		attempt_number: i64,
		channel_path: &Path,
		transport: &str,
	) -> Result<Option<RunControlChannel>> {
		validate_run_control_channel_inputs(run_id, attempt_number, channel_path, transport)?;

		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let Some(attempt) = state.run_attempts.get(run_id).cloned() else {
			return Ok(None);
		};

		if attempt.attempt_number != attempt_number {
			return Ok(None);
		}

		let Some(lease) = state.leases.get(&attempt.issue_id) else {
			return Ok(None);
		};

		if lease.run_id != run_id {
			return Ok(None);
		}

		let (published_at, published_at_unix) = state
			.control_channels
			.get(run_id)
			.filter(|channel| channel.attempt_number == attempt_number)
			.map_or_else(|| (now.text.clone(), now.unix), |channel| {
				(channel.published_at.clone(), channel.published_at_unix)
			});
		let channel = RunControlChannelRecord {
			project_id: lease.project_id.clone(),
			issue_id: attempt.issue_id.clone(),
			run_id: run_id.to_owned(),
			attempt_number,
			transport: transport.to_owned(),
			channel_path: channel_path.to_path_buf(),
			status: RUN_CONTROL_CHANNEL_STATUS_ACTIVE.to_owned(),
			published_at,
			published_at_unix,
			updated_at: now.text,
			updated_at_unix: now.unix,
		};

		state.control_channels.insert(run_id.to_owned(), channel.clone());
		self.upsert_run_control_channel_locked(&channel)?;

		Ok(Some(channel.as_public()))
	}

	/// Mark a run-control channel as no longer active for an attempt.
	pub(crate) fn retire_run_control_channel_for_attempt(
		&self,
		run_id: &str,
		attempt_number: i64,
		status: &str,
	) -> Result<()> {
		validate_run_control_channel_status(status)?;

		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let Some(channel) = state.control_channels.get_mut(run_id) else {
			return Ok(());
		};

		if channel.attempt_number != attempt_number {
			return Ok(());
		}

		channel.status = status.to_owned();
		channel.updated_at = now.text;
		channel.updated_at_unix = now.unix;

		let channel = channel.clone();

		self.upsert_run_control_channel_locked(&channel)
	}

	/// Resolve a local run-control request against active runtime ownership and audit it.
	#[cfg_attr(not(test), allow(dead_code))]
	pub(crate) fn resolve_run_control_action(
		&self,
		request: RunControlActionRequest<'_>,
	) -> Result<RunControlActionReceipt> {
		validate_run_control_action_request(&request)?;

		let resolution = {
			let state = self.lock()?;

			resolve_run_control_action_locked(&state, &request)
		};
		let event = self.append_run_control_audit_event(
			&resolution.audit_target,
			&resolution.outcome,
			&resolution.reason,
			None,
		)?;
		let receipt_channel = resolution
			.channel
			.clone()
			.or_else(|| resolution.audit_target.channel.clone());

		Ok(RunControlActionReceipt {
			project_id: resolution.audit_target.project_id,
			issue_id: resolution.audit_target.issue_id,
			run_id: resolution.audit_target.run_id,
			attempt_number: resolution.audit_target.attempt_number,
			thread_id: resolution.audit_target.thread_id,
			turn_id: resolution.audit_target.turn_id,
			current_thread_id: resolution.audit_target.current_thread_id,
			current_turn_id: resolution.audit_target.current_turn_id,
			source: resolution.audit_target.source,
			action: resolution.audit_target.action,
			outcome: resolution.outcome,
			reason: resolution.reason,
			audit_record_id: event.record_id(),
			metadata: resolution.audit_target.metadata,
			context: resolution.audit_target.context,
			channel: receipt_channel,
		})
	}

	/// Append a follow-up audit outcome for an already resolved control request.
	#[cfg_attr(not(test), allow(dead_code))]
	pub(crate) fn record_run_control_action_outcome(
		&self,
		receipt: &RunControlActionReceipt,
		outcome: &str,
		reason: &str,
	) -> Result<PrivateExecutionEvent> {
		validate_run_control_action_outcome(outcome)?;
		validate_required_run_control_field("reason", reason)?;

		let target = RunControlAuditTarget {
			project_id: receipt.project_id.clone(),
			issue_id: receipt.issue_id.clone(),
			run_id: receipt.run_id.clone(),
			attempt_number: receipt.attempt_number,
			thread_id: receipt.thread_id.clone(),
			turn_id: receipt.turn_id.clone(),
			current_thread_id: receipt.current_thread_id.clone(),
			current_turn_id: receipt.current_turn_id.clone(),
			source: receipt.source.clone(),
			action: receipt.action.clone(),
			timeout_ms: None,
			metadata: receipt.metadata.clone(),
			context: receipt.context.clone(),
			attempt_status: None,
			branch_name: None,
			worktree_path: None,
			active_lease: None,
			event_count: None,
			last_event_type: None,
			last_event_at: None,
			channel: receipt.channel.clone(),
		};

		self.append_run_control_audit_event(
			&target,
			outcome,
			reason,
			Some(receipt.audit_record_id),
		)
	}

	/// Append a follow-up audit outcome for a control action handled from a channel request.
	#[cfg_attr(not(test), allow(dead_code))]
	pub(crate) fn record_run_control_action_delivery_outcome(
		&self,
		request: RunControlActionOutcomeRequest<'_>,
	) -> Result<PrivateExecutionEvent> {
		validate_run_control_action_outcome(request.outcome)?;
		validate_required_run_control_field("reason", request.reason)?;

		let target = RunControlAuditTarget {
			project_id: request.project_id.to_owned(),
			issue_id: request.issue_id.to_owned(),
			run_id: request.run_id.to_owned(),
			attempt_number: request.attempt_number,
			thread_id: request.thread_id.map(str::to_owned),
			turn_id: request.turn_id.map(str::to_owned),
			current_thread_id: request.current_thread_id.map(str::to_owned),
			current_turn_id: request.current_turn_id.map(str::to_owned),
			source: request.source.to_owned(),
			action: request.action.to_owned(),
			timeout_ms: request.timeout_ms,
			metadata: request.metadata.cloned(),
			context: None,
			attempt_status: None,
			branch_name: None,
			worktree_path: None,
			active_lease: None,
			event_count: None,
			last_event_type: None,
			last_event_at: None,
			channel: request.channel.cloned(),
		};

		self.append_run_control_audit_event(
			&target,
			request.outcome,
			request.reason,
			request.parent_record_id,
		)
	}

	#[cfg_attr(not(test), allow(dead_code))]
	fn append_run_control_audit_event(
		&self,
		target: &RunControlAuditTarget,
		outcome: &str,
		reason: &str,
		parent_record_id: Option<i64>,
	) -> Result<PrivateExecutionEvent> {
		validate_run_control_action_outcome(outcome)?;

		let channel = target.channel.as_ref();
		let failure_class = run_control_action_failure_class(&target.action, outcome, reason);
		let payload = serde_json::json!({
			"schema": "decodex.run_control_action/v1",
			"action": target.action,
			"source": target.source,
			"outcome": outcome,
			"reason": reason,
			"failure_class": failure_class,
			"parent_record_id": parent_record_id,
			"requested": {
				"project_id": target.project_id,
				"issue_id": target.issue_id,
				"run_id": target.run_id,
				"attempt_number": target.attempt_number,
				"thread_id": target.thread_id,
				"turn_id": target.turn_id,
				"timeout_ms": target.timeout_ms,
			},
			"observed": {
				"thread_id": target.current_thread_id.as_deref(),
				"turn_id": target.current_turn_id.as_deref(),
			},
			"lane": {
				"attempt_status": target.attempt_status.as_deref(),
				"active_lease": target.active_lease,
				"branch": target.branch_name.as_deref(),
				"worktree_path": target.worktree_path.as_ref().map(|path| path.display().to_string()),
				"event_count": target.event_count,
				"last_event_type": target.last_event_type.as_deref(),
				"last_event_at": target.last_event_at.as_deref(),
			},
			"metadata": target.metadata.as_ref(),
			"context": target.context.as_ref(),
			"channel": channel.map(|channel| serde_json::json!({
				"transport": channel.transport(),
				"channel_path": channel.channel_path().display().to_string(),
				"status": channel.status(),
				"published_at": channel.published_at(),
				"updated_at": channel.updated_at(),
				"path_exists": channel.channel_path().exists(),
			})),
		});

		self.append_private_execution_event(
			&target.project_id,
			&target.issue_id,
			&target.run_id,
			target.attempt_number,
			"control_action",
			payload,
		)
	}

	/// Compute the next attempt number for one issue.
	pub fn next_attempt_number(&self, issue_id: &str) -> Result<i64> {
		let state = self.lock()?;
		let next_attempt = state
			.run_attempts
			.values()
			.filter(|attempt| attempt.issue_id == issue_id)
			.map(|attempt| attempt.attempt_number)
			.max()
			.unwrap_or(0)
			+ 1;

		Ok(next_attempt)
	}

	/// Count attempts that consume the retry budget for one issue.
	pub fn retry_budget_attempt_count(&self, issue_id: &str) -> Result<i64> {
		if let Some(sqlite) = self.sqlite.as_ref() {
			let sqlite = sqlite
				.lock()
				.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

			return sqlite.retry_budget_attempt_count(issue_id);
		}

		let state = self.lock_without_refresh()?;
		let retry_budget_attempts = state
			.run_attempts
			.values()
			.filter(|attempt| {
				attempt.issue_id == issue_id
					&& matches!(
						attempt.status.as_str(),
						"failed" | "interrupted" | "terminal_guarded"
					)
			})
			.count() as i64;

		Ok(retry_budget_attempts)
	}

	/// Return whether a later attempt for one issue consumed retry budget.
	pub fn issue_has_retry_budget_attempt_after(
		&self,
		issue_id: &str,
		attempt_number: i64,
	) -> Result<bool> {
		if let Some(sqlite) = self.sqlite.as_ref() {
			let sqlite = sqlite
				.lock()
				.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

			return sqlite.issue_has_retry_budget_attempt_after(issue_id, attempt_number);
		}

		let state = self.lock_without_refresh()?;

		Ok(state.run_attempts.values().any(|attempt| {
			attempt.issue_id == issue_id
				&& attempt.attempt_number > attempt_number
				&& matches!(
					attempt.status.as_str(),
					"failed" | "interrupted" | "terminal_guarded"
				)
		}))
	}

	/// Attach the active thread identifier to a run attempt.
	pub fn update_run_thread(&self, run_id: &str, thread_id: &str) -> Result<()> {
		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;

		if let Some(attempt) = state.run_attempts.get_mut(run_id) {
			attempt.thread_id = Some(thread_id.to_owned());
			attempt.updated_at = now.text;
			attempt.updated_at_unix = now.unix;

			let attempt = attempt.clone();

			return self.upsert_run_attempt_locked(&attempt);
		}

		Ok(())
	}

	/// Attach the active turn identifier to a run attempt.
	pub fn update_run_turn(&self, run_id: &str, turn_id: &str) -> Result<()> {
		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;

		if let Some(attempt) = state.run_attempts.get_mut(run_id) {
			attempt.turn_id = Some(turn_id.to_owned());
			attempt.updated_at = now.text;
			attempt.updated_at_unix = now.unix;

			let attempt = attempt.clone();

			return self.upsert_run_attempt_locked(&attempt);
		}

		Ok(())
	}

	/// Update the status for one run attempt.
	pub fn update_run_status(&self, run_id: &str, status: &str) -> Result<()> {
		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;

		if let Some(attempt) = state.run_attempts.get_mut(run_id) {
			attempt.status = status.to_owned();
			attempt.updated_at = now.text;
			attempt.updated_at_unix = now.unix;

			let attempt = attempt.clone();

			return self.upsert_run_attempt_locked(&attempt);
		}

		Ok(())
	}

	/// Mark all active run attempts for one issue as succeeded.
	pub fn succeed_active_run_attempts_for_issue(&self, issue_id: &str) -> Result<usize> {
		let now = timestamp_parts();
		let mut state = self.lock()?;
		let mut updated_count = 0;

		for attempt in state
			.run_attempts
			.values_mut()
			.filter(|attempt| attempt.issue_id == issue_id)
			.filter(|attempt| active_run_attempt_status(&attempt.status))
		{
			attempt.status = "succeeded".to_owned();
			attempt.updated_at = now.text.clone();
			attempt.updated_at_unix = now.unix;
			updated_count += 1;
		}

		if updated_count > 0 {
			self.persist_runtime_state_locked(&state)?;
		}

		Ok(updated_count)
	}

	/// Read one run attempt.
	pub fn run_attempt(&self, run_id: &str) -> Result<Option<RunAttempt>> {
		let state = self.lock()?;

		Ok(state.run_attempts.get(run_id).map(RunAttemptRecord::as_public))
	}

	/// Read one run attempt by issue and attempt number.
	pub fn run_attempt_for_issue_attempt(
		&self,
		issue_id: &str,
		attempt_number: i64,
	) -> Result<Option<RunAttempt>> {
		let state = self.lock()?;
		let attempt = state
			.run_attempts
			.values()
			.filter(|attempt| {
				attempt.issue_id == issue_id && attempt.attempt_number == attempt_number
			})
			.max_by(|left, right| compare_attempt_records(left, right))
			.map(RunAttemptRecord::as_public);

		Ok(attempt)
	}

	/// Read the latest run attempt for one issue.
	pub fn latest_run_attempt_for_issue(&self, issue_id: &str) -> Result<Option<RunAttempt>> {
		let state = self.lock()?;
		let attempt = state
			.run_attempts
			.values()
			.filter(|attempt| attempt.issue_id == issue_id)
			.max_by(|left, right| compare_attempt_records(left, right))
			.map(RunAttemptRecord::as_public);

		Ok(attempt)
	}

	/// List all locally recorded run attempts for one issue.
	pub fn list_run_attempts_for_issue(&self, issue_id: &str) -> Result<Vec<RunAttempt>> {
		let state = self.lock()?;
		let mut attempts = state
			.run_attempts
			.values()
			.filter(|attempt| attempt.issue_id == issue_id)
			.map(RunAttemptRecord::as_public)
			.collect::<Vec<_>>();

		attempts.sort_by(|left, right| {
			left.attempt_number()
				.cmp(&right.attempt_number())
				.then_with(|| left.run_id().cmp(right.run_id()))
		});

		Ok(attempts)
	}

	/// Return whether one run already has a matching protocol event.
	pub fn run_has_protocol_event(&self, run_id: &str, event_type: &str) -> Result<bool> {
		let state = self.lock()?;

		Ok(state.events.get(run_id).is_some_and(|events| {
			events.iter().any(|event| event.event_type == event_type)
		}))
	}

	/// List recent run attempts for one project, including lease and protocol summary fields.
	pub fn list_recent_runs(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<ProjectRunStatus>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_state_locked(&mut state, project_id)?;

		let mut runs = state
			.run_attempts
			.values()
			.filter_map(|attempt| state.project_run_status(project_id, attempt))
			.collect::<Vec<_>>();

		runs.sort_by(compare_project_run_status);
		runs.truncate(limit);

		Ok(runs)
	}

	/// List active and recent run attempts for one project from one durable snapshot.
	pub(crate) fn list_project_runs(
		&self,
		project_id: &str,
		base_recent_limit: usize,
	) -> Result<(Vec<ProjectRunStatus>, Vec<ProjectRunStatus>)> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_state_locked(&mut state, project_id)?;

		let mut runs = state
			.run_attempts
			.values()
			.filter_map(|attempt| state.project_run_status(project_id, attempt))
			.collect::<Vec<_>>();

		runs.sort_by(compare_project_run_status);

		let active_runs = runs
			.iter()
			.filter(|status| status.active_lease())
			.cloned()
			.collect::<Vec<_>>();
		let recent_limit = base_recent_limit.saturating_add(active_runs.len());
		let mut recent_runs = runs;

		recent_runs.truncate(recent_limit);

		Ok((active_runs, recent_runs))
	}

	/// List all active leased runs for one project without applying the recent-run limit.
	pub fn list_active_runs(&self, project_id: &str) -> Result<Vec<ProjectRunStatus>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_state_locked(&mut state, project_id)?;

		let mut runs = state
			.run_attempts
			.values()
			.filter_map(|attempt| {
				let status = state.project_run_status(project_id, attempt)?;

				status.active_lease.then_some(status)
			})
			.collect::<Vec<_>>();

		runs.sort_by(compare_project_run_status);

		Ok(runs)
	}

	/// Append one protocol event to the journal for a run.
	pub fn append_event(
		&self,
		run_id: &str,
		sequence_number: i64,
		event_type: &str,
		_payload: &str,
	) -> Result<()> {
		let mut state = self.lock_without_refresh()?;
		let insert_index = {
			let events = state.events.entry(run_id.to_owned()).or_default();

			match events.binary_search_by_key(&sequence_number, |event| event.sequence_number) {
				Ok(_index) => {
					eyre::bail!(
						"Protocol event `{run_id}` sequence `{sequence_number}` already exists in the runtime journal."
					);
				},
				Err(index) => index,
			}
		};
		let now = timestamp_parts();
		let event = ProtocolEventRecord {
			sequence_number,
			event_type: event_type.to_owned(),
			created_at: now.text,
			created_at_unix: now.unix,
		};

		if !self.append_protocol_event_locked(run_id, &event)? {
			eyre::bail!(
				"Protocol event `{run_id}` sequence `{sequence_number}` already exists in the runtime journal."
			);
		}

		state
			.event_summaries
			.entry(run_id.to_owned())
			.or_default()
			.record_event(&event);
		state.events.entry(run_id.to_owned()).or_default().insert(insert_index, event);

		Ok(())
	}

	/// Persist a locally known Linear execution event in the runtime store.
	pub(crate) fn record_linear_execution_event(
		&self,
		record: &LinearExecutionEventRecord,
	) -> Result<bool> {
		records::validate_linear_execution_event_record(record).map_err(|error| eyre::eyre!(error))?;

		let now = timestamp_parts();
		let idempotency_key = record.idempotency_key.clone();
		let mut state = self.lock_without_refresh()?;

		if state.linear_execution_events.contains_key(&idempotency_key) {
			return Ok(false);
		}

		let runtime_record = LinearExecutionEventRuntimeRecord {
			record: record.clone(),
			event_unix: parse_linear_execution_event_unix(record),
			recorded_at: now.text,
			recorded_at_unix: now.unix,
		};
		let is_new = self.insert_linear_execution_event_if_absent_locked(&runtime_record)?;

		if is_new {
			state.linear_execution_events.insert(idempotency_key, runtime_record);
		}

		Ok(is_new)
	}

	pub(crate) fn forget_linear_execution_event(&self, idempotency_key: &str) -> Result<()> {
		let mut state = self.lock_without_refresh()?;

		state.linear_execution_events.remove(idempotency_key);

		self.delete_linear_execution_event_locked(idempotency_key)
	}

	/// List locally cached Linear execution events for one issue lane.
	pub(crate) fn list_linear_execution_events(
		&self,
		service_id: &str,
		issue_id: &str,
	) -> Result<Vec<LinearExecutionEventRecord>> {
		let mut records = match self.list_persisted_linear_execution_events(service_id, issue_id)? {
			Some(records) => records,
			None => {
				let state = self.lock_without_refresh()?;

				state
					.linear_execution_events
					.values()
					.filter(|record| {
						record.record.service_id == service_id && record.record.issue_id == issue_id
					})
					.cloned()
					.collect::<Vec<_>>()
			},
		};

		records.sort_by(compare_linear_execution_event_runtime_records);

		Ok(records.into_iter().map(|record| record.record).collect())
	}

	/// Append one private execution event to the local runtime evidence ledger.
	pub fn append_private_execution_event(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
		event_type: &str,
		payload: Value,
	) -> Result<PrivateExecutionEvent> {
		validate_private_execution_event_inputs(
			project_id,
			issue_id,
			run_id,
			attempt_number,
			event_type,
		)?;

		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let mut record = PrivateExecutionEventRuntimeRecord {
			record_id: 0,
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			run_id: run_id.to_owned(),
			attempt_number,
			event_type: event_type.to_owned(),
			payload,
			recorded_at: now.text,
			recorded_at_unix: now.unix,
		};

		record.record_id = match self.insert_private_execution_event_locked(&record)? {
			Some(record_id) => record_id,
			None => state.next_private_execution_event_id()?,
		};

		state.private_execution_events.push(record.clone());

		Ok(record.as_public())
	}

	/// List private execution events for one project/issue/run/attempt tuple.
	pub fn list_private_execution_events(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<Vec<PrivateExecutionEvent>> {
		let state = self.lock()?;
		let mut records = state
			.private_execution_events
			.iter()
			.filter(|record| {
				record.project_id == project_id
					&& record.issue_id == issue_id
					&& record.run_id == run_id
					&& record.attempt_number == attempt_number
			})
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_private_execution_event_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List private execution events for one project/run/attempt tuple.
	pub fn list_private_execution_events_for_run_attempt(
		&self,
		project_id: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<Vec<PrivateExecutionEvent>> {
		let state = self.lock()?;
		let mut records = state
			.private_execution_events
			.iter()
			.filter(|record| {
				record.project_id == project_id
					&& record.run_id == run_id
					&& record.attempt_number == attempt_number
			})
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_private_execution_event_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// Count protocol journal records for one run.
	pub fn event_count(&self, run_id: &str) -> Result<i64> {
		let state = self.lock()?;

		Ok(state.protocol_event_summary(run_id).event_count)
	}

	/// Read the latest recorded activity timestamp for one run as a Unix epoch.
	pub fn last_run_activity_unix_epoch(&self, run_id: &str) -> Result<Option<i64>> {
		let state = self.lock()?;
		let last_activity = state.run_attempts.get(run_id).map(|attempt| attempt.updated_at_unix);
		let last_event = state.protocol_event_summary(run_id).last_event_at_unix;

		Ok(match (last_activity, last_event) {
			(Some(run_activity), Some(event_activity)) => Some(run_activity.max(event_activity)),
			(Some(run_activity), None) => Some(run_activity),
			(None, Some(event_activity)) => Some(event_activity),
			(None, None) => None,
		})
	}

	/// Read the latest recorded protocol-event timestamp for one run as a Unix epoch.
	pub fn last_protocol_activity_unix_epoch(&self, run_id: &str) -> Result<Option<i64>> {
		let state = self.lock()?;

		Ok(state.protocol_event_summary(run_id).last_event_at_unix)
	}

	/// Create or replace the worktree mapping for one issue.
	pub fn upsert_worktree(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
		worktree_path: &str,
	) -> Result<()> {
		let mapping = WorktreeMappingRecord {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			branch_name: branch_name.to_owned(),
			worktree_path: PathBuf::from(worktree_path),
		};
		let mut state = self.lock_without_refresh()?;

		state.worktrees.insert(issue_id.to_owned(), mapping.clone());
		state.remember_run_project(project_id, issue_id, None);

		self.upsert_worktree_and_remember_run_project_locked(&mapping)
	}

	/// Create or replace the retained review handoff marker for one issue lane.
	pub(crate) fn upsert_review_handoff_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		marker: &ReviewHandoffMarker,
	) -> Result<()> {
		let now = timestamp_parts();
		let key = ReviewMarkerKey::new(project_id, issue_id, marker.branch_name());
		let mut state = self.lock()?;

		state.review_handoffs.insert(
			key,
			ReviewHandoffRuntimeRecord {
				project_id: project_id.to_owned(),
				issue_id: issue_id.to_owned(),
				branch_name: marker.branch_name().to_owned(),
				marker: marker.clone(),
				updated_at: now.text,
				updated_at_unix: now.unix,
			},
		);

		self.persist_runtime_state_locked(&state)
	}

	/// Read the retained review handoff marker for one issue branch from the runtime DB.
	pub(crate) fn review_handoff_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
	) -> Result<Option<ReviewHandoffMarker>> {
		let state = self.lock()?;
		let key = ReviewMarkerKey::new(project_id, issue_id, branch_name);

		Ok(state.review_handoffs.get(&key).map(|record| record.marker.clone()))
	}

	/// Create or replace the retained review orchestration marker for one issue lane.
	pub(crate) fn upsert_review_orchestration_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		marker: &ReviewOrchestrationMarker,
	) -> Result<()> {
		let now = timestamp_parts();
		let key = ReviewOrchestrationKey::new(
			project_id,
			issue_id,
			marker.branch_name(),
			marker.run_id(),
			marker.attempt_number(),
		);
		let mut state = self.lock()?;

		state.review_orchestrations.insert(
			key,
			ReviewOrchestrationRuntimeRecord {
				project_id: project_id.to_owned(),
				issue_id: issue_id.to_owned(),
				branch_name: marker.branch_name().to_owned(),
				run_id: marker.run_id().to_owned(),
				attempt_number: marker.attempt_number(),
				marker: marker.clone(),
				updated_at: now.text,
				updated_at_unix: now.unix,
			},
		);

		self.persist_runtime_state_locked(&state)
	}

	/// Read retained review orchestration for the current handoff identity.
	pub(crate) fn review_orchestration_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		review_handoff: &ReviewHandoffMarker,
	) -> Result<Option<ReviewOrchestrationMarker>> {
		let state = self.lock()?;
		let key = ReviewOrchestrationKey::new(
			project_id,
			issue_id,
			review_handoff.branch_name(),
			review_handoff.run_id(),
			review_handoff.attempt_number(),
		);

		Ok(state.review_orchestrations.get(&key).map(|record| record.marker.clone()))
	}

	/// Create or replace the latest review-policy checkpoint for one run phase.
	pub(crate) fn upsert_review_policy_checkpoint(
		&self,
		input: ReviewPolicyCheckpointInput<'_>,
	) -> Result<ReviewPolicyCheckpoint> {
		let now = timestamp_parts();
		let key = ReviewPolicyKey::new(
			input.project_id,
			input.issue_id,
			input.run_id,
			input.attempt_number,
			input.phase,
		);
		let record = ReviewPolicyRuntimeRecord {
			project_id: input.project_id.to_owned(),
			issue_id: input.issue_id.to_owned(),
			run_id: input.run_id.to_owned(),
			attempt_number: input.attempt_number,
			phase: input.phase.to_owned(),
			status: input.status.to_owned(),
			head_sha: input.head_sha.to_owned(),
			nonclean_rounds: input.nonclean_rounds,
			updated_at: now.text,
			updated_at_unix: now.unix,
		};
		let mut state = self.lock()?;

		state.review_policy_checkpoints.insert(key, record.clone());
		self.persist_runtime_state_locked(&state)?;

		Ok(record.as_public())
	}

	/// Read the latest runtime-owned review-policy checkpoint for one run phase.
	pub(crate) fn review_policy_checkpoint(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
		phase: &str,
	) -> Result<Option<ReviewPolicyCheckpoint>> {
		let state = self.lock()?;
		let key = ReviewPolicyKey::new(project_id, issue_id, run_id, attempt_number, phase);

		Ok(state.review_policy_checkpoints.get(&key).map(ReviewPolicyRuntimeRecord::as_public))
	}

	/// Clear review-policy checkpoints for one completed run attempt.
	pub(crate) fn clear_review_policy_checkpoints_for_run_attempt(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<()> {
		let mut state = self.lock()?;

		state.review_policy_checkpoints.retain(|key, _record| {
			key.project_id != project_id
				|| key.issue_id != issue_id
				|| key.run_id != run_id
				|| key.attempt_number != attempt_number
		});

		self.delete_review_policy_checkpoints_for_run_attempt_locked(
			project_id,
			issue_id,
			run_id,
			attempt_number,
		)
	}

	/// Remove retained review markers for one issue without clearing its worktree mapping.
	pub(crate) fn clear_review_markers(&self, issue_id: &str) -> Result<()> {
		let mut state = self.lock()?;

		state.review_handoffs.retain(|key, _record| key.issue_id != issue_id);
		state
			.review_orchestrations
			.retain(|key, _record| key.issue_id != issue_id);
		state
			.review_policy_checkpoints
			.retain(|key, _record| key.issue_id != issue_id);
		self.persist_runtime_state_locked(&state)?;

		self.delete_review_markers_locked(issue_id)
	}

	/// Remove the exact retained review markers created for one handoff identity.
	pub(crate) fn clear_review_markers_for_handoff(
		&self,
		project_id: &str,
		issue_id: &str,
		handoff_marker: &ReviewHandoffMarker,
		orchestration_marker: &ReviewOrchestrationMarker,
	) -> Result<()> {
		let handoff_key = ReviewMarkerKey::new(project_id, issue_id, handoff_marker.branch_name());
		let orchestration_key = ReviewOrchestrationKey::new(
			project_id,
			issue_id,
			orchestration_marker.branch_name(),
			orchestration_marker.run_id(),
			orchestration_marker.attempt_number(),
		);
		let mut state = self.lock()?;

		state.review_handoffs.remove(&handoff_key);
		state.review_orchestrations.remove(&orchestration_key);
		state.review_policy_checkpoints.retain(|key, _record| {
			key.project_id != project_id
				|| key.issue_id != issue_id
				|| key.run_id != orchestration_marker.run_id()
				|| key.attempt_number != orchestration_marker.attempt_number()
		});
		self.persist_runtime_state_locked(&state)?;

		self.delete_review_marker_identity_locked(
			project_id,
			issue_id,
			handoff_marker.branch_name(),
			orchestration_marker.run_id(),
			orchestration_marker.attempt_number(),
		)
	}

	/// Read the worktree mapping for one issue.
	pub fn worktree_for_issue(&self, issue_id: &str) -> Result<Option<WorktreeMapping>> {
		let state = self.lock()?;

		Ok(state.worktrees.get(issue_id).map(WorktreeMappingRecord::as_public))
	}

	/// List all known worktree mappings.
	pub fn list_worktrees(&self, project_id: &str) -> Result<Vec<WorktreeMapping>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_state_locked(&mut state, project_id)?;

		let mut mappings = state
			.worktrees
			.values()
			.filter(|mapping| mapping.project_id == project_id)
			.map(WorktreeMappingRecord::as_public)
			.collect::<Vec<_>>();

		mappings.sort_by(|left, right| left.issue_id.cmp(&right.issue_id));

		Ok(mappings)
	}

	/// Remove the worktree mapping for one issue.
	pub fn clear_worktree(&self, issue_id: &str) -> Result<()> {
		let mut state = self.lock()?;

		state.worktrees.remove(issue_id);
		state.review_handoffs.retain(|key, _record| key.issue_id != issue_id);
		state
			.review_orchestrations
			.retain(|key, _record| key.issue_id != issue_id);
		state
			.review_policy_checkpoints
			.retain(|key, _record| key.issue_id != issue_id);
		self.persist_runtime_state_locked(&state)?;

		self.delete_worktree_and_review_markers_locked(issue_id)
	}

	fn lock_without_refresh(&self) -> Result<MutexGuard<'_, StateData>> {
		self.inner.lock().map_err(|_| eyre::eyre!("StateStore mutex is poisoned."))
	}

	fn lock(&self) -> Result<MutexGuard<'_, StateData>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_runtime_state_locked(&mut state)?;

		Ok(state)
	}

	fn refresh_runtime_state_locked(&self, state: &mut StateData) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;
		let loaded = sqlite.load_state()?;

		state.replace_durable_state(loaded);

		Ok(())
	}

	fn refresh_project_run_state_locked(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;
		let loaded = sqlite.load_project_run_state_for_project(project_id)?;

		state.replace_project_run_state(loaded);

		Ok(())
	}

	fn refresh_project_registry_state_locked(&self, state: &mut StateData) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;
		let loaded = sqlite.load_project_registry_state()?;

		state.replace_project_registry_state(loaded);

		Ok(())
	}

	fn persist_runtime_state_locked(&self, state: &StateData) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.persist_runtime_state(state)
	}

	fn delete_project_locked(&self, service_id: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_project(service_id)
	}

	fn delete_connector_backoff_locked(&self, project_id: &str, connector: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_connector_backoff(project_id, connector)
	}

	fn upsert_run_attempt_locked(&self, attempt: &RunAttemptRecord) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_run_attempt(attempt)
	}

	fn upsert_run_control_channel_locked(&self, channel: &RunControlChannelRecord) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_run_control_channel(channel)
	}

	fn upsert_lease_and_remember_run_project_locked(&self, lease: &IssueLease) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_lease_and_remember_run_project(lease)
	}

	fn upsert_worktree_and_remember_run_project_locked(
		&self,
		mapping: &WorktreeMappingRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_worktree_and_remember_run_project(mapping)
	}

	fn append_protocol_event_locked(
		&self,
		run_id: &str,
		event: &ProtocolEventRecord,
	) -> Result<bool> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(true);
		};
		let sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.append_protocol_event(run_id, event)
	}

	fn insert_linear_execution_event_if_absent_locked(
		&self,
		record: &LinearExecutionEventRuntimeRecord,
	) -> Result<bool> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(true);
		};
		let sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.insert_linear_execution_event_if_absent(record)
	}

	fn delete_linear_execution_event_locked(&self, idempotency_key: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_linear_execution_event(idempotency_key)
	}

	fn list_persisted_linear_execution_events(
		&self,
		service_id: &str,
		issue_id: &str,
	) -> Result<Option<Vec<LinearExecutionEventRuntimeRecord>>> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(None);
		};
		let sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.list_linear_execution_events(service_id, issue_id).map(Some)
	}

	fn insert_private_execution_event_locked(
		&self,
		record: &PrivateExecutionEventRuntimeRecord,
	) -> Result<Option<i64>> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(None);
		};
		let sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.insert_private_execution_event(record).map(Some)
	}

	fn delete_lease_locked(&self, issue_id: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_lease(issue_id)
	}

	fn retarget_issue_identity_locked(
		&self,
		previous_issue_id: &str,
		canonical_issue_id: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.retarget_issue_identity(previous_issue_id, canonical_issue_id)
	}

	fn delete_worktree_and_review_markers_locked(&self, issue_id: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_worktree_and_review_markers(issue_id)
	}

	fn delete_review_markers_locked(&self, issue_id: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_review_markers(issue_id)
	}

	fn delete_review_marker_identity_locked(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_review_marker_identity(
			project_id,
			issue_id,
			branch_name,
			run_id,
			attempt_number,
		)
	}

	fn delete_review_policy_checkpoints_for_run_attempt_locked(
		&self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite = sqlite
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_review_policy_checkpoints_for_run_attempt(
			project_id,
			issue_id,
			run_id,
			attempt_number,
		)
	}
}

#[cfg_attr(not(test), allow(dead_code))]
struct RunControlActionResolution {
	audit_target: RunControlAuditTarget,
	outcome: String,
	reason: String,
	channel: Option<RunControlChannel>,
}

#[derive(Clone)]
#[cfg_attr(not(test), allow(dead_code))]
struct RunControlAuditTarget {
	project_id: String,
	issue_id: String,
	run_id: String,
	attempt_number: i64,
	attempt_status: Option<String>,
	thread_id: Option<String>,
	turn_id: Option<String>,
	source: String,
	action: String,
	timeout_ms: Option<i64>,
	current_thread_id: Option<String>,
	current_turn_id: Option<String>,
	metadata: Option<Value>,
	context: Option<Value>,
	branch_name: Option<String>,
	worktree_path: Option<PathBuf>,
	active_lease: Option<bool>,
	event_count: Option<i64>,
	last_event_type: Option<String>,
	last_event_at: Option<String>,
	channel: Option<RunControlChannel>,
}

/// Shared dispatch-slot capacity for one project.
#[derive(Clone, Copy)]
pub(crate) enum DispatchSlotLimit {
	/// Fixed number of cross-process dispatch slots.
	Limited(u32),
	/// Allocate dispatch slots on demand without a fixed project cap.
	Unlimited,
}
impl DispatchSlotLimit {
	fn validate(self) -> Result<()> {
		if matches!(self, Self::Limited(0)) {
			eyre::bail!("dispatch slot limit must be greater than zero or unlimited");
		}

		Ok(())
	}

	fn includes(self, slot_index: usize) -> Result<bool> {
		match self {
			Self::Unlimited => Ok(true),
			Self::Limited(limit) => Ok(slot_index
				< usize::try_from(limit)
					.map_err(|_error| eyre::eyre!("dispatch slot limit overflowed usize"))?),
		}
	}
}

impl From<u32> for DispatchSlotLimit {
	fn from(value: u32) -> Self {
		Self::Limited(value)
	}
}

impl From<Option<u32>> for DispatchSlotLimit {
	fn from(value: Option<u32>) -> Self {
		match value {
			Some(limit) => Self::Limited(limit),
			None => Self::Unlimited,
		}
	}
}

impl From<WorkflowConcurrencyLimit> for DispatchSlotLimit {
	fn from(value: WorkflowConcurrencyLimit) -> Self {
		Self::from(value.dispatch_slot_limit())
	}
}

fn retarget_review_handoff_issue(
	records: &mut HashMap<ReviewMarkerKey, ReviewHandoffRuntimeRecord>,
	previous_issue_id: &str,
	canonical_issue_id: &str,
) {
	let previous_keys = records
		.keys()
		.filter(|key| key.issue_id == previous_issue_id)
		.cloned()
		.collect::<Vec<_>>();

	for key in previous_keys {
		let Some(mut record) = records.remove(&key) else {
			continue;
		};

		record.issue_id = canonical_issue_id.to_owned();

		records
			.entry(ReviewMarkerKey::new(&key.project_id, canonical_issue_id, &key.branch_name))
			.or_insert(record);
	}
}

fn retarget_review_policy_issue(
	records: &mut HashMap<ReviewPolicyKey, ReviewPolicyRuntimeRecord>,
	previous_issue_id: &str,
	canonical_issue_id: &str,
) {
	let previous_keys = records
		.keys()
		.filter(|key| key.issue_id == previous_issue_id)
		.cloned()
		.collect::<Vec<_>>();

	for key in previous_keys {
		let Some(mut record) = records.remove(&key) else {
			continue;
		};

		record.issue_id = canonical_issue_id.to_owned();

		records
			.entry(ReviewPolicyKey::new(
				&key.project_id,
				canonical_issue_id,
				&key.run_id,
				key.attempt_number,
				&key.phase,
			))
			.or_insert(record);
	}
}

fn active_run_attempt_status(status: &str) -> bool {
	matches!(status, "starting" | "running")
}

#[cfg_attr(not(test), allow(dead_code))]
fn resolve_run_control_action_locked(
	state: &StateData,
	request: &RunControlActionRequest<'_>,
) -> RunControlActionResolution {
	let Some(attempt) = state.run_attempts.get(request.run_id) else {
		return rejected_run_control_resolution(request, None, "run_not_found");
	};
	let audit_project_id = state
		.control_channels
		.get(request.run_id)
		.map(|channel| channel.project_id.clone())
		.or_else(|| state.project_id_for_run(&attempt.issue_id, &attempt.run_id))
		.unwrap_or_else(|| request.project_id.to_owned());
	let project_run_status = state.project_run_status(&audit_project_id, attempt);
	let control_channel = project_run_status
		.as_ref()
		.and_then(|status| status.control_channel().cloned())
		.or_else(|| state.control_channels.get(request.run_id).map(RunControlChannelRecord::as_public));
	let audit_target = RunControlAuditTarget {
		project_id: audit_project_id,
		issue_id: attempt.issue_id.clone(),
		run_id: attempt.run_id.clone(),
		attempt_number: attempt.attempt_number,
		attempt_status: Some(attempt.status.clone()),
		thread_id: request.thread_id.map(str::to_owned),
		turn_id: request.turn_id.map(str::to_owned),
		current_thread_id: attempt.thread_id.clone(),
		current_turn_id: attempt.turn_id.clone(),
		source: request.source.to_owned(),
		action: request.action.to_owned(),
		timeout_ms: request.timeout_ms,
		metadata: request.metadata.cloned(),
		context: request.context.cloned(),
		branch_name: project_run_status
			.as_ref()
			.and_then(|status| status.branch_name().map(str::to_owned)),
		worktree_path: project_run_status
			.as_ref()
			.and_then(|status| status.worktree_path().map(Path::to_path_buf)),
		active_lease: project_run_status.as_ref().map(ProjectRunStatus::active_lease),
		event_count: project_run_status.as_ref().map(ProjectRunStatus::event_count),
		last_event_type: project_run_status
			.as_ref()
			.and_then(|status| status.last_event_type().map(str::to_owned)),
		last_event_at: project_run_status
			.as_ref()
			.and_then(|status| status.last_event_at().map(str::to_owned)),
		channel: control_channel.clone(),
	};

	if attempt.issue_id != request.issue_id {
		return rejected_run_control_resolution(request, Some(audit_target), "issue_mismatch");
	}
	if attempt.attempt_number != request.attempt_number {
		return rejected_run_control_resolution(request, Some(audit_target), "attempt_mismatch");
	}
	if request.thread_id.is_some()
		&& attempt.thread_id.as_deref() != request.thread_id
	{
		return rejected_run_control_resolution(request, Some(audit_target), "thread_mismatch");
	}
	if request.turn_id.is_some() && attempt.turn_id.as_deref() != request.turn_id {
		return rejected_run_control_resolution(request, Some(audit_target), "turn_mismatch");
	}

	let Some(lease) = state.leases.get(request.issue_id) else {
		return rejected_run_control_resolution(request, Some(audit_target), "active_lease_missing");
	};

	if lease.project_id != request.project_id {
		return rejected_run_control_resolution(request, Some(audit_target), "project_mismatch");
	}
	if lease.run_id != request.run_id {
		return rejected_run_control_resolution(request, Some(audit_target), "active_run_mismatch");
	}
	if !active_run_attempt_status(&attempt.status) {
		return rejected_run_control_resolution(request, Some(audit_target), "run_not_active");
	}

	let Some(channel) = control_channel else {
		return rejected_run_control_resolution(request, Some(audit_target), "control_channel_missing");
	};
	let audit_target = RunControlAuditTarget { channel: Some(channel.clone()), ..audit_target };

	if channel.project_id() != request.project_id
		|| channel.issue_id() != request.issue_id
		|| channel.attempt_number() != request.attempt_number
	{
		return rejected_run_control_resolution(
			request,
			Some(audit_target),
			"control_channel_identity_mismatch",
		);
	}
	if channel.status() != RUN_CONTROL_CHANNEL_STATUS_ACTIVE {
		return rejected_run_control_resolution(
			request,
			Some(audit_target),
			"control_channel_inactive",
		);
	}
	if !channel.channel_path().exists() {
		return rejected_run_control_resolution(
			request,
			Some(audit_target),
			"control_channel_missing",
		);
	}

	RunControlActionResolution {
		audit_target,
		outcome: RUN_CONTROL_ACTION_ACCEPTED.to_owned(),
		reason: String::from("active_run_control_channel_resolved"),
		channel: Some(channel),
	}
}

#[cfg_attr(not(test), allow(dead_code))]
fn rejected_run_control_resolution(
	request: &RunControlActionRequest<'_>,
	audit_target: Option<RunControlAuditTarget>,
	reason: &str,
) -> RunControlActionResolution {
	RunControlActionResolution {
		audit_target: audit_target.unwrap_or_else(|| RunControlAuditTarget {
			project_id: request.project_id.to_owned(),
			issue_id: request.issue_id.to_owned(),
			run_id: request.run_id.to_owned(),
			attempt_number: request.attempt_number,
			attempt_status: None,
			thread_id: request.thread_id.map(str::to_owned),
			turn_id: request.turn_id.map(str::to_owned),
			current_thread_id: None,
			current_turn_id: None,
			source: request.source.to_owned(),
			action: request.action.to_owned(),
			timeout_ms: request.timeout_ms,
			metadata: request.metadata.cloned(),
			context: request.context.cloned(),
			branch_name: None,
			worktree_path: None,
			active_lease: None,
			event_count: None,
			last_event_type: None,
			last_event_at: None,
			channel: None,
		}),
		outcome: RUN_CONTROL_ACTION_REJECTED.to_owned(),
		reason: reason.to_owned(),
		channel: None,
	}
}

fn validate_run_control_channel_inputs(
	run_id: &str,
	attempt_number: i64,
	channel_path: &Path,
	transport: &str,
) -> Result<()> {
	validate_required_run_control_field("run_id", run_id)?;
	validate_required_run_control_field("transport", transport)?;

	if attempt_number < 1 {
		eyre::bail!("run-control attempt_number must be positive");
	}
	if channel_path.as_os_str().is_empty() {
		eyre::bail!("run-control channel_path must not be empty");
	}

	Ok(())
}

#[cfg_attr(not(test), allow(dead_code))]
fn validate_run_control_action_request(request: &RunControlActionRequest<'_>) -> Result<()> {
	validate_required_run_control_field("project_id", request.project_id)?;
	validate_required_run_control_field("issue_id", request.issue_id)?;
	validate_required_run_control_field("run_id", request.run_id)?;
	validate_required_run_control_field("source", request.source)?;
	validate_required_run_control_field("action", request.action)?;

	if request.attempt_number < 1 {
		eyre::bail!("run-control attempt_number must be positive");
	}

	if let Some(timeout_ms) = request.timeout_ms
		&& timeout_ms < 0
	{
		eyre::bail!("run-control timeout_ms must not be negative");
	}

	Ok(())
}

fn validate_required_run_control_field(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("run-control {name} must not be empty");
	}

	Ok(())
}

fn validate_run_control_channel_status(status: &str) -> Result<()> {
	if !matches!(
		status,
		RUN_CONTROL_CHANNEL_STATUS_ACTIVE
			| RUN_CONTROL_CHANNEL_STATUS_COMPLETED
			| RUN_CONTROL_CHANNEL_STATUS_FAILED
	) {
		eyre::bail!("unsupported run-control channel status `{status}`");
	}

	Ok(())
}

#[cfg_attr(not(test), allow(dead_code))]
fn validate_run_control_action_outcome(outcome: &str) -> Result<()> {
	if !matches!(
		outcome,
		RUN_CONTROL_ACTION_ACCEPTED
			| RUN_CONTROL_ACTION_REJECTED
			| RUN_CONTROL_ACTION_COMPLETED
			| RUN_CONTROL_ACTION_FAILED
			| RUN_CONTROL_ACTION_TIMED_OUT
			| RUN_CONTROL_ACTION_FALLBACK
	) {
		eyre::bail!("unsupported run-control action outcome `{outcome}`");
	}

	Ok(())
}

fn run_control_action_failure_class(
	action: &str,
	outcome: &str,
	reason: &str,
) -> Option<&'static str> {
	if !matches!(
		outcome,
		RUN_CONTROL_ACTION_REJECTED
			| RUN_CONTROL_ACTION_FAILED
			| RUN_CONTROL_ACTION_TIMED_OUT
			| RUN_CONTROL_ACTION_FALLBACK
	) {
		return None;
	}
	if action == "steer" && reason == "turn_mismatch" {
		return Some("stale_expected_turn_id");
	}
	if action == "steer" && reason == "active_turn_not_steerable" {
		return Some("active_turn_not_steerable");
	}
	if action == "steer" && reason == "app_server_turn_steer_unsupported" {
		return Some("app_server_turn_steer_unsupported");
	}

	Some("run_control_action_failed")
}

fn retarget_review_orchestration_issue(
	records: &mut HashMap<ReviewOrchestrationKey, ReviewOrchestrationRuntimeRecord>,
	previous_issue_id: &str,
	canonical_issue_id: &str,
) {
	let previous_keys = records
		.keys()
		.filter(|key| key.issue_id == previous_issue_id)
		.cloned()
		.collect::<Vec<_>>();

	for key in previous_keys {
		let Some(mut record) = records.remove(&key) else {
			continue;
		};

		record.issue_id = canonical_issue_id.to_owned();

		records
			.entry(ReviewOrchestrationKey::new(
				&key.project_id,
				canonical_issue_id,
				&key.branch_name,
				&key.run_id,
				key.attempt_number,
			))
			.or_insert(record);
	}
}
