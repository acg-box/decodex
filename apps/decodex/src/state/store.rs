use crate::autonomy_proposal::AutonomyProposalChallengeInput;

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
	pub(crate) review_level: &'a str,
	pub(crate) status: &'a str,
	pub(crate) head_sha: &'a str,
	pub(crate) nonclean_rounds: i64,
	pub(crate) details_json: &'a str,
}

/// Input fields for looking up a review checkpoint by its reusable evidence key.
pub(crate) struct ReviewCheckpointArtifactLookup<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) phase: &'a str,
	pub(crate) review_level: &'a str,
	pub(crate) head_sha: &'a str,
}

/// Project-scoped loop evidence cached for one operator status render.
#[derive(Clone, Debug, Default)]
pub(crate) struct ProjectLoopEvidenceSnapshot {
	private_events: HashMap<(String, String, i64), Vec<PrivateExecutionEvent>>,
	review_lifecycle_records: HashMap<(String, String), ReviewLifecycleRecord>,
	review_checkpoints: HashMap<(String, String, i64, String), ReviewPolicyCheckpoint>,
	decision_contracts: Vec<DecisionContractRecord>,
	autonomy_objectives: Vec<AutonomyObjectiveRecord>,
	autonomy_signals: Vec<AutonomySignalRecord>,
	autonomy_proposals: Vec<AutonomyProposalRecord>,
	program_intake_plans: Vec<ProgramIntakePlanRecord>,
}
impl ProjectLoopEvidenceSnapshot {
	fn insert_private_event(&mut self, event: PrivateExecutionEvent) {
		self.private_events
			.entry((event.issue_id().to_owned(), event.run_id().to_owned(), event.attempt_number()))
			.or_default()
			.push(event);
	}

	fn insert_review_lifecycle_record(&mut self, record: ReviewLifecycleRecord) {
		self.review_lifecycle_records
			.insert((record.issue_id().to_owned(), record.branch_name().to_owned()), record);
	}

	fn insert_review_checkpoint(&mut self, checkpoint: ReviewPolicyCheckpoint) {
		self.review_checkpoints.insert(
			(
				checkpoint.issue_id().to_owned(),
				checkpoint.run_id().to_owned(),
				checkpoint.attempt_number(),
				checkpoint.phase().to_owned(),
			),
			checkpoint,
		);
	}

	fn insert_decision_contract(&mut self, contract: DecisionContractRecord) {
		self.decision_contracts.push(contract);
	}

	fn insert_autonomy_objective(&mut self, objective: AutonomyObjectiveRecord) {
		self.autonomy_objectives.push(objective);
	}

	fn insert_autonomy_signal(&mut self, signal: AutonomySignalRecord) {
		self.autonomy_signals.push(signal);
	}

	fn insert_autonomy_proposal(&mut self, proposal: AutonomyProposalRecord) {
		self.autonomy_proposals.push(proposal);
	}

	fn insert_program_intake_plan(&mut self, plan: ProgramIntakePlanRecord) {
		self.program_intake_plans.push(plan);
	}

	fn sort_private_events(&mut self) {
		for events in self.private_events.values_mut() {
			events.sort_by(|left, right| {
				left.recorded_at_unix()
					.cmp(&right.recorded_at_unix())
					.then_with(|| left.record_id().cmp(&right.record_id()))
			});
		}
	}

	fn sort_decision_contracts(&mut self) {
		self.decision_contracts.sort_by(|left, right| {
			right
				.updated_at_unix()
				.cmp(&left.updated_at_unix())
				.then_with(|| left.contract_id().cmp(right.contract_id()))
		});
	}

	fn sort_autonomy_objectives(&mut self) {
		self.autonomy_objectives.sort_by(|left, right| {
			right
				.updated_at_unix()
				.cmp(&left.updated_at_unix())
				.then_with(|| left.objective_id().cmp(right.objective_id()))
				.then_with(|| left.version().cmp(&right.version()))
		});
	}

	fn sort_autonomy_signals(&mut self) {
		self.autonomy_signals.sort_by(|left, right| {
			right
				.updated_at_unix()
				.cmp(&left.updated_at_unix())
				.then_with(|| left.signal_id().cmp(right.signal_id()))
		});
	}

	fn sort_autonomy_proposals(&mut self) {
		self.autonomy_proposals.sort_by(|left, right| {
			right
				.updated_at_unix()
				.cmp(&left.updated_at_unix())
				.then_with(|| left.proposal_id().cmp(right.proposal_id()))
		});
	}

	fn sort_program_intake_plans(&mut self) {
		self.program_intake_plans.sort_by(|left, right| {
			right
				.updated_at_unix()
				.cmp(&left.updated_at_unix())
				.then_with(|| left.program_id().cmp(right.program_id()))
				.then_with(|| left.plan_id().cmp(right.plan_id()))
		});
	}

	pub(crate) fn private_events(
		&self,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
	) -> &[PrivateExecutionEvent] {
		self.private_events
			.get(&(issue_id.to_owned(), run_id.to_owned(), attempt_number))
			.map(Vec::as_slice)
			.unwrap_or(&[])
	}

	#[cfg_attr(not(test), allow(dead_code))]
	pub(crate) fn review_lifecycle_record(
		&self,
		issue_id: &str,
		branch_name: &str,
	) -> Option<&ReviewLifecycleRecord> {
		self.review_lifecycle_records.get(&(issue_id.to_owned(), branch_name.to_owned()))
	}

	pub(crate) fn review_lifecycle_records_for_issue(
		&self,
		issue_id: &str,
	) -> Vec<&ReviewLifecycleRecord> {
		let mut records = self
			.review_lifecycle_records
			.iter()
			.filter(|((record_issue_id, _), _)| record_issue_id == issue_id)
			.map(|(_, record)| record)
			.collect::<Vec<_>>();

		records.sort_by(|left, right| {
			left.updated_at_unix()
				.cmp(&right.updated_at_unix())
				.then_with(|| left.branch_name().cmp(right.branch_name()))
		});

		records
	}

	pub(crate) fn private_events_for_issue(&self, issue_id: &str) -> Vec<&PrivateExecutionEvent> {
		let mut events = self
			.private_events
			.iter()
			.filter(|((event_issue_id, _, _), _)| event_issue_id == issue_id)
			.flat_map(|(_, events)| events.iter())
			.collect::<Vec<_>>();

		events.sort_by(|left, right| {
			left.recorded_at_unix()
				.cmp(&right.recorded_at_unix())
				.then_with(|| left.record_id().cmp(&right.record_id()))
		});

		events
	}

	pub(crate) fn review_policy_checkpoint(
		&self,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
		phase: &str,
	) -> Option<&ReviewPolicyCheckpoint> {
		self.review_checkpoints.get(&(
			issue_id.to_owned(),
			run_id.to_owned(),
			attempt_number,
			phase.to_owned(),
		))
	}

	pub(crate) fn recent_autonomy_signals(&self, limit: usize) -> Vec<&AutonomySignalRecord> {
		self.autonomy_signals.iter().take(limit).collect()
	}

	pub(crate) fn recent_autonomy_proposals(&self, limit: usize) -> Vec<&AutonomyProposalRecord> {
		self.autonomy_proposals.iter().take(limit).collect()
	}

	pub(crate) fn autonomy_objective(
		&self,
		objective_id: &str,
		objective_version: u64,
	) -> Option<&AutonomyObjectiveRecord> {
		self.autonomy_objectives.iter().find(|record| {
			record.objective_id() == objective_id && record.version() == objective_version
		})
	}

	pub(crate) fn accepted_autonomy_objectives(&self) -> Vec<&AutonomyObjectiveRecord> {
		self.autonomy_objectives
			.iter()
			.filter(|record| record.state() == AutonomyObjectiveState::Accepted)
			.collect()
	}

	pub(crate) fn decision_contracts_for_autonomy_proposal(
		&self,
		proposal_id: &str,
	) -> Vec<&DecisionContractRecord> {
		self.decision_contracts
			.iter()
			.filter(|record| {
				record.contract().research_provenance().iter().any(|provenance| {
					provenance.kind() == "autonomy_proposal"
						&& provenance.reference() == proposal_id
				})
			})
			.collect()
	}

	pub(crate) fn program_intake_plans_for_contract(
		&self,
		contract_id: &str,
	) -> Vec<&ProgramIntakePlanRecord> {
		self.program_intake_plans
			.iter()
			.filter(|record| record.source_contract_id() == Some(contract_id))
			.collect()
	}
}

/// Input fields for recording the latest loop-guardrail checkpoint.
pub(crate) struct LoopGuardrailCheckpointInput<'a> {
	pub(crate) project_id: &'a str,
	pub(crate) issue_id: &'a str,
	pub(crate) reason: &'a str,
	pub(crate) fingerprint: &'a str,
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) details_json: &'a str,
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

	/// Open the local persistent runtime store without preloading durable rows.
	pub fn open_lazy(path: impl AsRef<Path>) -> Result<Self> {
		let sqlite = SqliteStateStore::open(path.as_ref())?;

		Ok(Self { inner: Mutex::new(StateData::default()), sqlite: Some(Mutex::new(sqlite)) })
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
			prune_unlocked_shared_lock_files(&worktree_root)?;
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

	fn issue_has_active_shared_claim_with_cleanup(
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

	pub(crate) fn record_run_activity_summary(
		&self,
		run_id: &str,
		attempt_number: i64,
		child_agent_activity: Option<&ChildAgentActivitySummary>,
		protocol_activity: Option<&ProtocolActivitySummary>,
	) -> Result<()> {
		let now = timestamp_parts();
		let summary = RunActivitySummaryRecord {
			run_id: run_id.to_owned(),
			attempt_number,
			child_agent_activity: child_agent_activity
				.cloned()
				.map(ChildAgentActivitySummary::sealed_durable),
			protocol_activity: protocol_activity.cloned(),
			updated_at: now.text,
			updated_at_unix: now.unix,
		};
		let mut state = self.lock_without_refresh()?;

		state.run_activity_summaries.insert(run_id.to_owned(), summary.clone());

		self.upsert_run_activity_summary_locked(&summary)
	}

	/// Persist a locally known Linear execution event in the runtime store.
	pub(crate) fn record_linear_execution_event(
		&self,
		record: &LinearExecutionEventRecord,
	) -> Result<bool> {
		records::validate_linear_execution_event_record(record)
			.map_err(|error| eyre::eyre!(error))?;

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

	/// List private execution events for one project/issue tuple.
	pub(crate) fn list_private_execution_events_for_issue(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<Vec<PrivateExecutionEvent>> {
		let state = self.lock()?;
		let mut records = state
			.private_execution_events
			.iter()
			.filter(|record| record.project_id == project_id && record.issue_id == issue_id)
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

	/// Build one project-scoped loop evidence snapshot for operator status rendering.
	pub(crate) fn project_loop_evidence_snapshot(
		&self,
		project_id: &str,
	) -> Result<ProjectLoopEvidenceSnapshot> {
		let mut state = self.lock_without_refresh()?;
		let mut snapshot = ProjectLoopEvidenceSnapshot::default();

		self.refresh_project_loop_evidence_state_locked(&mut state, project_id)?;

		for record in
			state.private_execution_events.iter().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_private_event(record.as_public());
		}
		for record in
			state.review_lifecycle_records.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_review_lifecycle_record(record.as_public());
		}
		for record in state
			.review_policy_checkpoints
			.values()
			.filter(|record| record.project_id == project_id)
		{
			snapshot.insert_review_checkpoint(record.as_public());
		}
		for record in
			state.decision_contracts.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_decision_contract(record.as_public());
		}
		for record in
			state.autonomy_objectives.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_autonomy_objective(record.as_public());
		}
		for record in
			state.autonomy_signals.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_autonomy_signal(record.as_public());
		}
		for record in
			state.autonomy_proposals.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_autonomy_proposal(record.as_public());
		}
		for record in
			state.program_intake_plans.values().filter(|record| record.project_id == project_id)
		{
			snapshot.insert_program_intake_plan(record.clone());
		}

		snapshot.sort_private_events();
		snapshot.sort_decision_contracts();
		snapshot.sort_autonomy_objectives();
		snapshot.sort_autonomy_signals();
		snapshot.sort_autonomy_proposals();
		snapshot.sort_program_intake_plans();

		Ok(snapshot)
	}

	/// Create or replace one local Loop/Decision Contract payload.
	#[allow(dead_code)]
	pub(crate) fn upsert_decision_contract(
		&self,
		project_id: &str,
		source_issue_id: Option<&str>,
		contract: DecisionContract,
	) -> Result<DecisionContractRecord> {
		validate_decision_contract_record_inputs(project_id, source_issue_id, &contract)?;

		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let key = DecisionContractKey::new(project_id, contract.contract_id());
		let (created_at, created_at_unix) = state.decision_contracts.get(&key).map_or_else(
			|| (now.text.clone(), now.unix),
			|record| (record.created_at.clone(), record.created_at_unix),
		);
		let record = DecisionContractRuntimeRecord {
			project_id: project_id.to_owned(),
			source_issue_id: source_issue_id.map(str::to_owned),
			status: contract.status(),
			contract,
			created_at,
			created_at_unix,
			updated_at: now.text,
			updated_at_unix: now.unix,
		};

		state.decision_contracts.insert(record.key(), record.clone());
		self.upsert_decision_contract_locked(&record)?;

		Ok(record.as_public())
	}

	/// Read one local Loop/Decision Contract by project and contract id.
	#[allow(dead_code)]
	pub(crate) fn decision_contract(
		&self,
		project_id: &str,
		contract_id: &str,
	) -> Result<Option<DecisionContractRecord>> {
		validate_required_decision_contract_field("project_id", project_id)?;
		validate_required_decision_contract_field("contract_id", contract_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.decision_contract(project_id, contract_id)
				.map(|record| record.map(|record| record.as_public()));
		}

		let state = self.lock()?;

		Ok(state
			.decision_contracts
			.get(&DecisionContractKey::new(project_id, contract_id))
			.map(DecisionContractRuntimeRecord::as_public))
	}

	/// Read one local Loop/Decision Contract for non-mutating readback/reconciliation only.
	///
	/// Readback and scheduler reconciliation treat quarantined legacy contract payloads as
	/// absent so stale Programs cannot crash operator surfaces or direct Program selection,
	/// while strict execution-facing reads still fail closed on removed contract shapes.
	pub(crate) fn decision_contract_for_readback(
		&self,
		project_id: &str,
		contract_id: &str,
	) -> Result<Option<DecisionContractRecord>> {
		validate_required_decision_contract_field("project_id", project_id)?;
		validate_required_decision_contract_field("contract_id", contract_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.decision_contract_for_readback(project_id, contract_id)
				.map(|record| record.map(|record| record.as_public()));
		}

		let state = self.lock()?;

		Ok(state
			.decision_contracts
			.get(&DecisionContractKey::new(project_id, contract_id))
			.map(DecisionContractRuntimeRecord::as_public))
	}

	/// List local Loop/Decision Contracts sourced from one tracker issue.
	#[allow(dead_code)]
	pub(crate) fn list_decision_contracts_for_issue(
		&self,
		project_id: &str,
		source_issue_id: &str,
	) -> Result<Vec<DecisionContractRecord>> {
		validate_required_decision_contract_field("project_id", project_id)?;
		validate_required_decision_contract_field("source_issue_id", source_issue_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.list_decision_contracts_for_issue(project_id, source_issue_id)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.decision_contracts
			.values()
			.filter(|record| {
				record.project_id == project_id
					&& record.source_issue_id.as_deref() == Some(source_issue_id)
			})
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_decision_contract_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List local Loop/Decision Contracts for one project.
	#[allow(dead_code)]
	pub(crate) fn list_decision_contracts_for_project(
		&self,
		project_id: &str,
	) -> Result<Vec<DecisionContractRecord>> {
		validate_required_decision_contract_field("project_id", project_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.list_decision_contracts_for_project(project_id)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.decision_contracts
			.values()
			.filter(|record| record.project_id == project_id)
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_decision_contract_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// Promote a latent Loop/Decision Contract into accepted execution authority.
	#[allow(dead_code)]
	pub(crate) fn promote_decision_contract(
		&self,
		project_id: &str,
		contract_id: &str,
		promotion: DecisionPromotion,
	) -> Result<DecisionContractRecord> {
		self.update_decision_contract(project_id, contract_id, |contract| {
			contract.promote(promotion)
		})
	}

	/// Mark a latent Loop/Decision Contract as waiting for more human direction.
	#[allow(dead_code)]
	pub(crate) fn mark_decision_contract_needs_human_decision(
		&self,
		project_id: &str,
		contract_id: &str,
		reason: &str,
	) -> Result<DecisionContractRecord> {
		self.update_decision_contract(project_id, contract_id, |contract| {
			contract.require_human_decision(reason.to_owned())
		})
	}

	/// Reject or supersede a Loop/Decision Contract.
	#[allow(dead_code)]
	pub(crate) fn reject_decision_contract(
		&self,
		project_id: &str,
		contract_id: &str,
		superseded_by_contract_id: Option<String>,
	) -> Result<DecisionContractRecord> {
		self.update_decision_contract(project_id, contract_id, |contract| {
			contract.reject_or_supersede(superseded_by_contract_id)
		})
	}

	#[allow(dead_code)]
	fn update_decision_contract(
		&self,
		project_id: &str,
		contract_id: &str,
		update: impl FnOnce(&mut DecisionContract) -> Result<()>,
	) -> Result<DecisionContractRecord> {
		validate_required_decision_contract_field("project_id", project_id)?;
		validate_required_decision_contract_field("contract_id", contract_id)?;

		let now = timestamp_parts();
		let key = DecisionContractKey::new(project_id, contract_id);
		let mut state = self.lock()?;
		let mut record = state
			.decision_contracts
			.get(&key)
			.cloned()
			.ok_or_else(|| eyre::eyre!("Decision contract `{contract_id}` does not exist."))?;

		update(&mut record.contract)?;

		record.contract.validate()?;

		record.status = record.contract.status();
		record.updated_at = now.text;
		record.updated_at_unix = now.unix;

		state.decision_contracts.insert(key, record.clone());
		self.upsert_decision_contract_locked(&record)?;

		Ok(record.as_public())
	}

	/// Create or replace one draft Objective Contract authority payload.
	#[allow(dead_code)]
	pub(crate) fn upsert_autonomy_objective_draft(
		&self,
		project_id: &str,
		objective: AutonomyObjectiveContract,
	) -> Result<AutonomyObjectiveRecord> {
		validate_autonomy_objective_record_inputs(project_id, &objective)?;

		if objective.state() != AutonomyObjectiveState::Draft {
			eyre::bail!("Autonomy objective drafts must be stored with state `draft`.");
		}

		let now = timestamp_parts();
		let key = AutonomyObjectiveKey::new(project_id, objective.id(), objective.version());
		let mut state = self.lock()?;

		if let Some(existing) = state.autonomy_objectives.get(&key)
			&& existing.state != AutonomyObjectiveState::Draft
		{
			eyre::bail!(
				"Autonomy objective `{}` version {} is `{}` and cannot be replaced as a draft.",
				objective.id(),
				objective.version(),
				existing.state.as_str()
			);
		}

		let (created_at, created_at_unix) = state.autonomy_objectives.get(&key).map_or_else(
			|| (now.text.clone(), now.unix),
			|record| (record.created_at.clone(), record.created_at_unix),
		);
		let record = AutonomyObjectiveRuntimeRecord {
			project_id: project_id.to_owned(),
			state: objective.state(),
			objective,
			created_at,
			created_at_unix,
			updated_at: now.text,
			updated_at_unix: now.unix,
		};

		state.autonomy_objectives.insert(record.key(), record.clone());
		self.upsert_autonomy_objective_locked(&record)?;

		Ok(record.as_public())
	}

	/// Read one Objective Contract version by project, objective id, and version.
	#[allow(dead_code)]
	pub(crate) fn autonomy_objective(
		&self,
		project_id: &str,
		objective_id: &str,
		version: u64,
	) -> Result<Option<AutonomyObjectiveRecord>> {
		validate_required_autonomy_objective_field("project_id", project_id)?;
		validate_required_autonomy_objective_field("objective_id", objective_id)?;
		validate_autonomy_objective_version(version)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.autonomy_objective(project_id, objective_id, version)
				.map(|record| record.map(|record| record.as_public()));
		}

		let state = self.lock()?;

		Ok(state
			.autonomy_objectives
			.get(&AutonomyObjectiveKey::new(project_id, objective_id, version))
			.map(AutonomyObjectiveRuntimeRecord::as_public))
	}

	/// Read the current accepted Objective Contract version for one objective id.
	#[allow(dead_code)]
	pub(crate) fn current_accepted_autonomy_objective(
		&self,
		project_id: &str,
		objective_id: &str,
	) -> Result<Option<AutonomyObjectiveRecord>> {
		validate_required_autonomy_objective_field("project_id", project_id)?;
		validate_required_autonomy_objective_field("objective_id", objective_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.current_accepted_autonomy_objective(project_id, objective_id)
				.map(|record| record.map(|record| record.as_public()));
		}

		let state = self.lock()?;

		Ok(state
			.autonomy_objectives
			.values()
			.filter(|record| {
				record.project_id == project_id
					&& record.objective.id() == objective_id
					&& record.state == AutonomyObjectiveState::Accepted
			})
			.max_by_key(|record| record.objective.version())
			.map(AutonomyObjectiveRuntimeRecord::as_public))
	}

	/// List all Objective Contract versions for one objective id.
	#[allow(dead_code)]
	pub(crate) fn list_autonomy_objective_history(
		&self,
		project_id: &str,
		objective_id: &str,
	) -> Result<Vec<AutonomyObjectiveRecord>> {
		validate_required_autonomy_objective_field("project_id", project_id)?;
		validate_required_autonomy_objective_field("objective_id", objective_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.list_autonomy_objective_history(project_id, objective_id)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.autonomy_objectives
			.values()
			.filter(|record| {
				record.project_id == project_id && record.objective.id() == objective_id
			})
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by_key(|record| record.objective.version());

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List recent Objective Contract versions for one project for MCP/operator readback.
	#[allow(dead_code)]
	pub(crate) fn recent_autonomy_objectives_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomyObjectiveRecord>> {
		validate_required_autonomy_objective_field("project_id", project_id)?;

		if limit == 0 {
			return Ok(Vec::new());
		}

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.recent_autonomy_objectives_for_project(project_id, limit)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.autonomy_objectives
			.values()
			.filter(|record| record.project_id == project_id)
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(|left, right| {
			right
				.updated_at_unix
				.cmp(&left.updated_at_unix)
				.then_with(|| left.objective.id().cmp(right.objective.id()))
				.then_with(|| left.objective.version().cmp(&right.objective.version()))
		});
		records.truncate(limit);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// Persist one read-only autonomy signal against the currently accepted objective version.
	#[allow(dead_code)]
	pub(crate) fn record_autonomy_signal(
		&self,
		project_id: &str,
		signal: AutonomySignal,
	) -> Result<AutonomySignalRecord> {
		validate_autonomy_signal_record_inputs(project_id, &signal)?;

		let now = timestamp_parts();
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
		validate_required_autonomy_signal_field("project_id", project_id)?;
		validate_required_autonomy_signal_field("signal_id", signal_id)?;

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
		validate_required_autonomy_signal_field("project_id", project_id)?;
		validate_required_autonomy_signal_field("objective_id", objective_id)?;
		validate_autonomy_objective_version(objective_version)?;

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

		records.sort_by(compare_autonomy_signal_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List recent autonomy signals for one project for operator readback.
	#[allow(dead_code)]
	pub(crate) fn recent_autonomy_signals_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomySignalRecord>> {
		validate_required_autonomy_signal_field("project_id", project_id)?;

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

		records.sort_by(compare_recent_autonomy_signal_runtime_records);
		records.truncate(limit);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// Compile a non-mutating autonomy proposal dry-run from persisted objective and signal rows.
	#[allow(dead_code)]
	pub(crate) fn compile_autonomy_proposal_dry_run(
		&self,
		input: AutonomyProposalCompileInput,
		signal_ids: &[String],
	) -> Result<AutonomyProposal> {
		let objective = self
			.autonomy_objective(&input.project_id, &input.objective_id, input.objective_version)?
			.map(|record| record.objective().clone());
		let mut signals = Vec::new();

		for signal_id in signal_ids {
			validate_required_autonomy_proposal_field("signal_id", signal_id)?;

			let signal = self.autonomy_signal(&input.project_id, signal_id)?.ok_or_else(|| {
				eyre::eyre!("Autonomy proposal signal `{signal_id}` does not exist.")
			})?;

			signals.push(signal.signal().clone());
		}

		AutonomyProposal::compile_dry_run(objective.as_ref(), &signals, input)
	}

	/// Persist one autonomy proposal as non-executable dry-run evidence.
	#[allow(dead_code)]
	pub(crate) fn record_autonomy_proposal(
		&self,
		project_id: &str,
		proposal: AutonomyProposal,
	) -> Result<AutonomyProposalRecord> {
		validate_autonomy_proposal_record_inputs(project_id, &proposal)?;

		let now = timestamp_parts();
		let mut state = self.lock()?;

		if !proposal.has_refusal_reason(AutonomyProposalRefusalReason::MissingObjective) {
			let objective_key = AutonomyObjectiveKey::new(
				project_id,
				proposal.objective_id(),
				proposal.objective_version(),
			);
			let objective = state.autonomy_objectives.get(&objective_key).ok_or_else(|| {
				eyre::eyre!(
					"Autonomy proposal `{}` references missing objective `{}` version {}.",
					proposal.id(),
					proposal.objective_id(),
					proposal.objective_version()
				)
			})?;

			if objective.state != AutonomyObjectiveState::Accepted {
				eyre::bail!(
					"Autonomy proposal `{}` can only be recorded for an accepted objective version unless it carries missing_objective refusal; `{}` version {} is `{}`.",
					proposal.id(),
					proposal.objective_id(),
					proposal.objective_version(),
					objective.state.as_str()
				);
			}
		}

		for signal_id in proposal.source_signal_ids() {
			let signal = state
				.autonomy_signals
				.get(&AutonomySignalKey::new(project_id, signal_id))
				.ok_or_else(|| {
					eyre::eyre!(
						"Autonomy proposal `{}` references missing signal `{signal_id}`.",
						proposal.id()
					)
				})?;

			if signal.signal.objective_id() != proposal.objective_id()
				|| signal.signal.objective_version() != proposal.objective_version()
			{
				eyre::bail!(
					"Autonomy proposal `{}` signal `{signal_id}` is not tied to objective `{}` version {}.",
					proposal.id(),
					proposal.objective_id(),
					proposal.objective_version()
				);
			}
		}

		let key = AutonomyProposalKey::new(project_id, proposal.id());
		let (created_at, created_at_unix) = state.autonomy_proposals.get(&key).map_or_else(
			|| (now.text.clone(), now.unix),
			|record| (record.created_at.clone(), record.created_at_unix),
		);
		let record = AutonomyProposalRuntimeRecord {
			project_id: project_id.to_owned(),
			state: proposal.state(),
			proposal,
			created_at,
			created_at_unix,
			updated_at: now.text,
			updated_at_unix: now.unix,
		};

		state.autonomy_proposals.insert(record.key(), record.clone());
		self.upsert_autonomy_proposal_locked(&record)?;

		Ok(record.as_public())
	}

	/// Record challenge evidence against one persisted non-executable autonomy proposal.
	#[allow(dead_code)]
	pub(crate) fn record_autonomy_proposal_challenge(
		&self,
		project_id: &str,
		proposal_id: &str,
		challenge: AutonomyProposalChallengeInput,
	) -> Result<AutonomyProposalRecord> {
		validate_required_autonomy_proposal_field("project_id", project_id)?;
		validate_required_autonomy_proposal_field("proposal_id", proposal_id)?;

		let now = timestamp_parts();
		let key = AutonomyProposalKey::new(project_id, proposal_id);
		let mut state = self.lock()?;
		let mut record = state
			.autonomy_proposals
			.get(&key)
			.cloned()
			.ok_or_else(|| eyre::eyre!("Autonomy proposal `{proposal_id}` does not exist."))?;

		record.proposal.record_challenge(challenge)?;

		record.state = record.proposal.state();
		record.updated_at = now.text;
		record.updated_at_unix = now.unix;

		state.autonomy_proposals.insert(key, record.clone());
		self.upsert_autonomy_proposal_locked(&record)?;

		Ok(record.as_public())
	}

	/// Accept one proposal into a normal latent Decision Contract candidate.
	#[allow(dead_code)]
	pub(crate) fn accept_autonomy_proposal_as_decision_contract_candidate(
		&self,
		project_id: &str,
		proposal_id: &str,
		authority: AutonomyProposalDecisionBridgeAuthority,
	) -> Result<DecisionContractRecord> {
		validate_required_autonomy_proposal_field("project_id", project_id)?;
		validate_required_autonomy_proposal_field("proposal_id", proposal_id)?;

		let proposal_record = self
			.autonomy_proposal(project_id, proposal_id)?
			.ok_or_else(|| eyre::eyre!("Autonomy proposal `{proposal_id}` does not exist."))?;
		let contract = proposal_record.proposal().to_decision_contract_candidate(authority)?;
		let contract_id = contract.contract_id().to_owned();

		if let Some(existing) = self.decision_contract(project_id, &contract_id)? {
			let existing_contract = existing.contract();
			let has_generated_execution_links =
				!existing_contract.links().generated_issue_ids().is_empty()
					|| !existing_contract.links().generated_issue_identifiers().is_empty()
					|| !existing_contract.links().execution_program_node_ids().is_empty();

			if existing.status() == DecisionContractStatus::DraftLatent
				&& existing_contract.promotion().is_none()
				&& !has_generated_execution_links
			{
				return Ok(existing);
			}

			eyre::bail!(
				"Autonomy proposal `{proposal_id}` already has Decision Contract `{contract_id}` with status `{}`; acceptance will not replace promoted or generated execution authority.",
				existing.status().as_str()
			);
		}

		let source_issue_id = contract.source_intent().source_issue_identifier().map(str::to_owned);

		self.upsert_decision_contract(project_id, source_issue_id.as_deref(), contract)
	}

	/// Read one autonomy proposal by stable proposal id.
	#[allow(dead_code)]
	pub(crate) fn autonomy_proposal(
		&self,
		project_id: &str,
		proposal_id: &str,
	) -> Result<Option<AutonomyProposalRecord>> {
		validate_required_autonomy_proposal_field("project_id", project_id)?;
		validate_required_autonomy_proposal_field("proposal_id", proposal_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.autonomy_proposal(project_id, proposal_id)
				.map(|record| record.map(|record| record.as_public()));
		}

		let state = self.lock()?;

		Ok(state
			.autonomy_proposals
			.get(&AutonomyProposalKey::new(project_id, proposal_id))
			.map(AutonomyProposalRuntimeRecord::as_public))
	}

	/// List autonomy proposals tied to one exact Objective Contract version.
	#[allow(dead_code)]
	pub(crate) fn list_autonomy_proposals_for_objective(
		&self,
		project_id: &str,
		objective_id: &str,
		objective_version: u64,
	) -> Result<Vec<AutonomyProposalRecord>> {
		validate_required_autonomy_proposal_field("project_id", project_id)?;
		validate_required_autonomy_proposal_field("objective_id", objective_id)?;
		validate_autonomy_objective_version(objective_version)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.list_autonomy_proposals_for_objective(project_id, objective_id, objective_version)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.autonomy_proposals
			.values()
			.filter(|record| {
				record.project_id == project_id
					&& record.proposal.objective_id() == objective_id
					&& record.proposal.objective_version() == objective_version
			})
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_autonomy_proposal_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List recent autonomy proposals for one project for operator readback.
	#[allow(dead_code)]
	pub(crate) fn recent_autonomy_proposals_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomyProposalRecord>> {
		validate_required_autonomy_proposal_field("project_id", project_id)?;

		if limit == 0 {
			return Ok(Vec::new());
		}

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.recent_autonomy_proposals_for_project(project_id, limit)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.autonomy_proposals
			.values()
			.filter(|record| record.project_id == project_id)
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_recent_autonomy_proposal_runtime_records);
		records.truncate(limit);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// Accept one draft Objective Contract version as immutable runtime authority.
	#[allow(dead_code)]
	pub(crate) fn accept_autonomy_objective_version(
		&self,
		project_id: &str,
		objective_id: &str,
		version: u64,
		acceptance: AutonomyObjectiveAcceptance,
	) -> Result<AutonomyObjectiveRecord> {
		validate_required_autonomy_objective_field("project_id", project_id)?;
		validate_required_autonomy_objective_field("objective_id", objective_id)?;
		validate_autonomy_objective_version(version)?;

		let superseded_by = acceptance.accepted_by().to_owned();
		let superseded_at = acceptance.accepted_at().to_owned();
		let supersession_source = acceptance.acceptance_source().to_owned();
		let now = timestamp_parts();
		let key = AutonomyObjectiveKey::new(project_id, objective_id, version);
		let mut state = self.lock()?;
		let mut record = state.autonomy_objectives.get(&key).cloned().ok_or_else(|| {
			eyre::eyre!("Autonomy objective `{objective_id}` version {version} does not exist.")
		})?;

		if let Some(current_version) = state
			.autonomy_objectives
			.values()
			.filter(|candidate| {
				candidate.project_id == project_id
					&& candidate.objective.id() == objective_id
					&& candidate.state == AutonomyObjectiveState::Accepted
			})
			.map(|candidate| candidate.objective.version())
			.max() && version <= current_version
		{
			eyre::bail!(
				"Autonomy objective `{objective_id}` version {version} must be greater than current accepted version {current_version}."
			);
		}

		record.objective.accept(acceptance)?;

		record.state = record.objective.state();
		record.updated_at = now.text.clone();
		record.updated_at_unix = now.unix;

		let superseded_keys = state
			.autonomy_objectives
			.iter()
			.filter(|(_, candidate)| {
				candidate.project_id == project_id
					&& candidate.objective.id() == objective_id
					&& candidate.state == AutonomyObjectiveState::Accepted
			})
			.map(|(key, _)| key.clone())
			.collect::<Vec<_>>();
		let mut changed_records = Vec::new();

		for superseded_key in superseded_keys {
			let supersession = AutonomyObjectiveSupersession::new(
				objective_id,
				version,
				superseded_by.clone(),
				superseded_at.clone(),
				supersession_source.clone(),
				format!("Accepted objective version {version} superseded this version."),
			)?;
			let mut superseded = state
				.autonomy_objectives
				.get(&superseded_key)
				.cloned()
				.expect("superseded key should exist");

			superseded.objective.supersede(supersession)?;

			superseded.state = superseded.objective.state();
			superseded.updated_at = now.text.clone();
			superseded.updated_at_unix = now.unix;

			state.autonomy_objectives.insert(superseded.key(), superseded.clone());
			changed_records.push(superseded);
		}

		state.autonomy_objectives.insert(key, record.clone());
		changed_records.push(record.clone());

		for changed_record in &changed_records {
			self.upsert_autonomy_objective_locked(changed_record)?;
		}

		Ok(record.as_public())
	}

	/// Reject one draft Objective Contract version with provenance.
	#[allow(dead_code)]
	pub(crate) fn reject_autonomy_objective_version(
		&self,
		project_id: &str,
		objective_id: &str,
		version: u64,
		rejection: AutonomyObjectiveRejection,
	) -> Result<AutonomyObjectiveRecord> {
		self.update_autonomy_objective(project_id, objective_id, version, |objective| {
			objective.reject(rejection)
		})
	}

	/// Supersede one draft or accepted Objective Contract version with provenance.
	#[allow(dead_code)]
	pub(crate) fn supersede_autonomy_objective_version(
		&self,
		project_id: &str,
		objective_id: &str,
		version: u64,
		supersession: AutonomyObjectiveSupersession,
	) -> Result<AutonomyObjectiveRecord> {
		self.update_autonomy_objective(project_id, objective_id, version, |objective| {
			objective.supersede(supersession)
		})
	}

	#[allow(dead_code)]
	fn update_autonomy_objective(
		&self,
		project_id: &str,
		objective_id: &str,
		version: u64,
		update: impl FnOnce(&mut AutonomyObjectiveContract) -> Result<()>,
	) -> Result<AutonomyObjectiveRecord> {
		validate_required_autonomy_objective_field("project_id", project_id)?;
		validate_required_autonomy_objective_field("objective_id", objective_id)?;
		validate_autonomy_objective_version(version)?;

		let now = timestamp_parts();
		let key = AutonomyObjectiveKey::new(project_id, objective_id, version);
		let mut state = self.lock()?;
		let mut record = state.autonomy_objectives.get(&key).cloned().ok_or_else(|| {
			eyre::eyre!("Autonomy objective `{objective_id}` version {version} does not exist.")
		})?;

		update(&mut record.objective)?;

		record.objective.validate()?;

		record.state = record.objective.state();
		record.updated_at = now.text;
		record.updated_at_unix = now.unix;

		state.autonomy_objectives.insert(key, record.clone());
		self.upsert_autonomy_objective_locked(&record)?;

		Ok(record.as_public())
	}

	/// Create or replace one local internal Execution Program payload.
	#[allow(dead_code)]
	pub(crate) fn upsert_execution_program(
		&self,
		project_id: &str,
		program: ExecutionProgram,
	) -> Result<ExecutionProgramRecord> {
		validate_execution_program_record_inputs(project_id, &program)?;

		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let key = ExecutionProgramKey::new(project_id, program.program_id());
		let (created_at, created_at_unix) = state.execution_programs.get(&key).map_or_else(
			|| (now.text.clone(), now.unix),
			|record| (record.created_at.clone(), record.created_at_unix),
		);
		let record = ExecutionProgramRuntimeRecord {
			project_id: project_id.to_owned(),
			source_contract_id: program.source_contract_id().map(str::to_owned),
			program,
			created_at,
			created_at_unix,
			updated_at: now.text,
			updated_at_unix: now.unix,
		};

		state.execution_programs.insert(record.key(), record.clone());

		apply_derived_program_intake_state(&mut state, &record);

		self.upsert_execution_program_locked(&record)?;

		Ok(record.as_public())
	}

	/// Read one local internal Execution Program by project and program id.
	#[allow(dead_code)]
	pub(crate) fn execution_program(
		&self,
		project_id: &str,
		program_id: &str,
	) -> Result<Option<ExecutionProgramRecord>> {
		validate_required_execution_program_field("project_id", project_id)?;
		validate_required_execution_program_field("program_id", program_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.execution_program(project_id, program_id)
				.map(|record| record.map(|record| record.as_public()));
		}

		let state = self.lock()?;

		Ok(state
			.execution_programs
			.get(&ExecutionProgramKey::new(project_id, program_id))
			.map(ExecutionProgramRuntimeRecord::as_public))
	}

	/// List local internal Execution Programs derived from one Decision Contract.
	#[allow(dead_code)]
	pub(crate) fn list_execution_programs_for_contract(
		&self,
		project_id: &str,
		source_contract_id: &str,
	) -> Result<Vec<ExecutionProgramRecord>> {
		validate_required_execution_program_field("project_id", project_id)?;
		validate_required_execution_program_field("source_contract_id", source_contract_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.list_execution_programs_for_contract(project_id, source_contract_id)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.execution_programs
			.values()
			.filter(|record| {
				record.project_id == project_id
					&& record.source_contract_id.as_deref() == Some(source_contract_id)
			})
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_execution_program_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List local internal Execution Programs retained for one project.
	#[allow(dead_code)]
	pub(crate) fn list_execution_programs(
		&self,
		project_id: &str,
	) -> Result<Vec<ExecutionProgramRecord>> {
		validate_required_execution_program_field("project_id", project_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let records = sqlite
				.list_execution_programs(project_id)?
				.into_iter()
				.map(|record| record.as_public())
				.collect();

			return Ok(records);
		}

		let state = self.lock()?;
		let mut records = state
			.execution_programs
			.values()
			.filter(|record| record.project_id == project_id)
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_execution_program_runtime_records);

		Ok(records.into_iter().map(|record| record.as_public()).collect())
	}

	/// List local Program Intake Plan records retained for one project.
	#[allow(dead_code)]
	pub(crate) fn list_program_intake_plans(
		&self,
		project_id: &str,
	) -> Result<Vec<ProgramIntakePlanRecord>> {
		validate_required_execution_program_field("project_id", project_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite.list_program_intake_plans(project_id);
		}

		let state = self.lock()?;
		let mut records = state
			.program_intake_plans
			.values()
			.filter(|record| record.project_id == project_id)
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_program_intake_plan_records);

		Ok(records)
	}

	/// List local issue mappings for one internal Execution Program.
	#[allow(dead_code)]
	pub(crate) fn list_program_issue_mappings(
		&self,
		project_id: &str,
		program_id: &str,
	) -> Result<Vec<ProgramIssueMappingRecord>> {
		validate_required_execution_program_field("project_id", project_id)?;
		validate_required_execution_program_field("program_id", program_id)?;

		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite.list_program_issue_mappings(project_id, program_id);
		}

		let state = self.lock()?;
		let mut records = state
			.program_issue_mappings
			.values()
			.filter(|record| record.project_id == project_id && record.program_id == program_id)
			.cloned()
			.collect::<Vec<_>>();

		records.sort_by(compare_program_issue_mapping_records);

		Ok(records)
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
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;
		let loaded = sqlite.load_state()?;

		state.replace_durable_state(loaded);

		Ok(())
	}

	fn refresh_project_run_metadata_state_locked(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;
		let loaded = sqlite.load_project_run_metadata_for_project(project_id)?;

		state.replace_project_run_metadata_state(loaded);

		Ok(())
	}

	fn refresh_run_activity_summaries_for_runs_locked(
		&self,
		state: &mut StateData,
		run_ids: &[String],
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.load_run_activity_summaries_for_runs(state, run_ids)
	}

	fn refresh_run_attempt_identities_from_worktree_markers_locked(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let updates = state
			.worktrees
			.values()
			.filter(|mapping| mapping.project_id == project_id)
			.filter_map(|mapping| {
				let marker = match read_run_activity_marker_snapshot(&mapping.worktree_path) {
					Ok(Some(marker)) => marker,
					Ok(None) => return None,
					Err(_) => return None,
				};
				let attempt = state.run_attempts.get(marker.run_id())?;

				if attempt.issue_id != mapping.issue_id
					|| attempt.attempt_number != marker.attempt_number()
				{
					return None;
				}

				let thread_id = marker.thread_id().map(str::to_owned);
				let turn_id = marker.turn_id().map(str::to_owned);

				if thread_id.is_none() && turn_id.is_none() {
					return None;
				}

				Some(Ok((marker.run_id().to_owned(), thread_id, turn_id)))
			})
			.collect::<Result<Vec<_>>>()?;

		for (run_id, thread_id, turn_id) in updates {
			let Some(attempt) = state.run_attempts.get_mut(&run_id) else {
				continue;
			};
			let mut changed = false;

			if attempt.thread_id.is_none()
				&& let Some(thread_id) = thread_id
			{
				attempt.thread_id = Some(thread_id);
				changed = true;
			}
			if attempt.turn_id.is_none()
				&& let Some(turn_id) = turn_id
			{
				attempt.turn_id = Some(turn_id);
				changed = true;
			}
			if changed {
				let attempt = attempt.clone();

				self.upsert_run_attempt_locked(&attempt)?;
			}
		}

		Ok(())
	}

	fn refresh_project_loop_evidence_state_locked(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;
		let loaded = sqlite.load_project_loop_evidence_for_project(project_id)?;

		state.replace_project_loop_evidence_state(project_id, loaded);

		Ok(())
	}

	fn refresh_project_registry_state_locked(&self, state: &mut StateData) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;
		let loaded = sqlite.load_project_registry_state()?;

		state.replace_project_registry_state(loaded);

		Ok(())
	}

	fn persist_runtime_state_locked(&self, state: &StateData) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.persist_runtime_state(state)
	}

	fn delete_project_locked(&self, service_id: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_project(service_id)
	}

	fn upsert_project_locked(&self, project: &ProjectRegistration) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_project(project)
	}

	fn delete_connector_backoff_locked(&self, project_id: &str, connector: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_connector_backoff(project_id, connector)
	}

	fn upsert_run_attempt_locked(&self, attempt: &RunAttemptRecord) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_run_attempt(attempt)
	}

	fn upsert_run_control_channel_locked(&self, channel: &RunControlChannelRecord) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_run_control_channel(channel)
	}

	fn upsert_run_activity_summary_locked(&self, summary: &RunActivitySummaryRecord) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_run_activity_summary(summary)
	}

	fn upsert_lease_and_remember_run_project_locked(&self, lease: &IssueLease) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_lease_and_remember_run_project(lease)
	}

	fn upsert_worktree_and_remember_run_project_locked(
		&self,
		mapping: &WorktreeMappingRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_worktree_and_remember_run_project(mapping)
	}

	fn insert_linear_execution_event_if_absent_locked(
		&self,
		record: &LinearExecutionEventRuntimeRecord,
	) -> Result<bool> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(true);
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.insert_linear_execution_event_if_absent(record)
	}

	fn delete_linear_execution_event_locked(&self, idempotency_key: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

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
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.list_linear_execution_events(service_id, issue_id).map(Some)
	}

	fn insert_private_execution_event_locked(
		&self,
		record: &PrivateExecutionEventRuntimeRecord,
	) -> Result<Option<i64>> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(None);
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.insert_private_execution_event(record).map(Some)
	}

	#[allow(dead_code)]
	fn upsert_decision_contract_locked(
		&self,
		record: &DecisionContractRuntimeRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_decision_contract(record)
	}

	#[allow(dead_code)]
	fn upsert_autonomy_objective_locked(
		&self,
		record: &AutonomyObjectiveRuntimeRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_autonomy_objective(record)
	}

	#[allow(dead_code)]
	fn upsert_autonomy_signal_locked(&self, record: &AutonomySignalRuntimeRecord) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_autonomy_signal(record)
	}

	#[allow(dead_code)]
	fn upsert_autonomy_proposal_locked(
		&self,
		record: &AutonomyProposalRuntimeRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_autonomy_proposal(record)
	}

	#[allow(dead_code)]
	fn upsert_execution_program_locked(
		&self,
		record: &ExecutionProgramRuntimeRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_execution_program(record)
	}

	fn delete_lease_locked(&self, issue_id: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

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
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.retarget_issue_identity(previous_issue_id, canonical_issue_id)
	}

	fn delete_worktree_and_review_lifecycle_locked(&self, issue_id: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_worktree_and_review_lifecycle(issue_id)
	}

	fn delete_worktree_mapping_locked(&self, issue_id: &str) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_worktree_mapping(issue_id)
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
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

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
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_review_policy_checkpoints_for_run_attempt(
			project_id,
			issue_id,
			run_id,
			attempt_number,
		)
	}

	fn delete_loop_guardrail_checkpoints_for_issue_locked(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_loop_guardrail_checkpoints_for_issue(project_id, issue_id)
	}

	fn delete_loop_guardrail_checkpoint_locked(
		&self,
		project_id: &str,
		issue_id: &str,
		reason: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let mut sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_loop_guardrail_checkpoint(project_id, issue_id, reason)
	}
}

fn retarget_review_lifecycle_issue(
	records: &mut HashMap<ReviewLifecycleKey, ReviewLifecycleRuntimeRecord>,
	previous_issue_id: &str,
	canonical_issue_id: &str,
) {
	let previous_keys =
		records.keys().filter(|key| key.issue_id == previous_issue_id).cloned().collect::<Vec<_>>();

	for key in previous_keys {
		let Some(mut record) = records.remove(&key) else {
			continue;
		};

		record.issue_id = canonical_issue_id.to_owned();

		records
			.entry(ReviewLifecycleKey::new(&key.project_id, canonical_issue_id, &key.branch_name))
			.or_insert(record);
	}
}

fn retarget_review_policy_issue(
	records: &mut HashMap<ReviewPolicyKey, ReviewPolicyRuntimeRecord>,
	previous_issue_id: &str,
	canonical_issue_id: &str,
) {
	let previous_keys =
		records.keys().filter(|key| key.issue_id == previous_issue_id).cloned().collect::<Vec<_>>();

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

fn retarget_evidence_artifact_issue(
	records: &mut HashMap<EvidenceArtifactKey, EvidenceArtifactRuntimeRecord>,
	previous_issue_id: &str,
	canonical_issue_id: &str,
) {
	let previous_keys =
		records.keys().filter(|key| key.issue_id == previous_issue_id).cloned().collect::<Vec<_>>();

	for key in previous_keys {
		let Some(mut record) = records.remove(&key) else {
			continue;
		};

		record.issue_id = canonical_issue_id.to_owned();

		records
			.entry(EvidenceArtifactKey::new(
				&key.project_id,
				canonical_issue_id,
				&key.artifact_kind,
				&key.key_hash,
			))
			.or_insert(record);
	}
}

fn retarget_loop_guardrail_issue(
	records: &mut HashMap<LoopGuardrailKey, LoopGuardrailRuntimeRecord>,
	previous_issue_id: &str,
	canonical_issue_id: &str,
) {
	let previous_keys =
		records.keys().filter(|key| key.issue_id == previous_issue_id).cloned().collect::<Vec<_>>();

	for key in previous_keys {
		let Some(mut record) = records.remove(&key) else {
			continue;
		};

		record.issue_id = canonical_issue_id.to_owned();

		records
			.entry(LoopGuardrailKey::new(&key.project_id, canonical_issue_id, &key.reason))
			.or_insert(record);
	}
}

fn running_run_attempt_status(status: &str) -> bool {
	matches!(status, "starting" | "running")
}

#[allow(dead_code)]
fn validate_decision_contract_record_inputs(
	project_id: &str,
	source_issue_id: Option<&str>,
	contract: &DecisionContract,
) -> Result<()> {
	validate_required_decision_contract_field("project_id", project_id)?;

	if let Some(source_issue_id) = source_issue_id {
		validate_required_decision_contract_field("source_issue_id", source_issue_id)?;
	}

	contract.validate()
}

#[allow(dead_code)]
fn validate_required_decision_contract_field(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("Decision contract {name} must not be empty.");
	}

	Ok(())
}

#[allow(dead_code)]
fn validate_autonomy_objective_record_inputs(
	project_id: &str,
	objective: &AutonomyObjectiveContract,
) -> Result<()> {
	validate_required_autonomy_objective_field("project_id", project_id)?;

	if objective.project_id() != project_id {
		eyre::bail!(
			"Autonomy objective `{}` belongs to project `{}` but was stored for `{}`.",
			objective.id(),
			objective.project_id(),
			project_id
		);
	}

	objective.validate()
}

#[allow(dead_code)]
fn validate_required_autonomy_objective_field(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("Autonomy objective {name} must not be empty.");
	}

	Ok(())
}

#[allow(dead_code)]
fn validate_autonomy_objective_version(version: u64) -> Result<()> {
	if version == 0 {
		eyre::bail!("Autonomy objective version must be greater than zero.");
	}

	Ok(())
}

#[allow(dead_code)]
fn validate_autonomy_signal_record_inputs(project_id: &str, signal: &AutonomySignal) -> Result<()> {
	validate_required_autonomy_signal_field("project_id", project_id)?;

	if signal.project_id() != project_id {
		eyre::bail!(
			"Autonomy signal `{}` belongs to project `{}` but was stored for `{}`.",
			signal.id(),
			signal.project_id(),
			project_id
		);
	}

	signal.validate()
}

#[allow(dead_code)]
fn validate_required_autonomy_signal_field(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("Autonomy signal {name} must not be empty.");
	}

	Ok(())
}

#[allow(dead_code)]
fn validate_autonomy_proposal_record_inputs(
	project_id: &str,
	proposal: &AutonomyProposal,
) -> Result<()> {
	validate_required_autonomy_proposal_field("project_id", project_id)?;

	if proposal.project_id() != project_id {
		eyre::bail!(
			"Autonomy proposal `{}` belongs to project `{}` but was stored for `{}`.",
			proposal.id(),
			proposal.project_id(),
			project_id
		);
	}

	proposal.validate()
}

#[allow(dead_code)]
fn validate_required_autonomy_proposal_field(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("Autonomy proposal {name} must not be empty.");
	}

	Ok(())
}

#[allow(dead_code)]
fn validate_execution_program_record_inputs(
	project_id: &str,
	program: &ExecutionProgram,
) -> Result<()> {
	validate_required_execution_program_field("project_id", project_id)?;

	program.validate()
}

#[allow(dead_code)]
fn validate_required_execution_program_field(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("Execution program {name} must not be empty.");
	}

	Ok(())
}
