use crate::autonomy_proposal::AutonomyProposalChallengeInput;
use project_run_recovery::{
	ProjectRunListingMode, project_lease_run_ids,
	project_run_recovery_candidate_counts_as_project_run, project_run_recovery_candidates,
	project_run_status_from_recovery_candidate,
};

const TERMINAL_THREAD_ARCHIVE_EVENT_TYPES: [&str; 2] =
	["thread/archive", "thread/archive/discarded"];
const DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE: &str = "protocol/post_archive_event/discarded";
const REVIEW_CHECKPOINT_PROMPT_VERSION: &str = "decodex-review-checkpoint/2";

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
			let sqlite =
				sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

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
			let sqlite =
				sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

			return sqlite.issue_has_retry_budget_attempt_after(issue_id, attempt_number);
		}

		let state = self.lock_without_refresh()?;

		Ok(state.run_attempts.values().any(|attempt| {
			attempt.issue_id == issue_id
				&& attempt.attempt_number > attempt_number
				&& matches!(attempt.status.as_str(), "failed" | "interrupted" | "terminal_guarded")
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

	/// Mark all running run attempts for one issue as succeeded.
	pub fn succeed_running_run_attempts_for_issue(&self, issue_id: &str) -> Result<usize> {
		let now = timestamp_parts();
		let mut state = self.lock()?;
		let mut updated_count = 0;

		for attempt in state
			.run_attempts
			.values_mut()
			.filter(|attempt| attempt.issue_id == issue_id)
			.filter(|attempt| running_run_attempt_status(&attempt.status))
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
		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.run_attempt_for_issue_attempt(issue_id, attempt_number)
				.map(|attempt| attempt.map(|attempt| attempt.as_public()));
		}

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
		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.latest_run_attempt_for_issue(issue_id)
				.map(|attempt| attempt.map(|attempt| attempt.as_public()));
		}

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
		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let attempts = sqlite
				.list_run_attempts_for_issue(issue_id)?
				.into_iter()
				.map(|attempt| attempt.as_public())
				.collect();

			return Ok(attempts);
		}

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

	/// List all locally recorded run attempts for one registered project.
	pub fn list_run_attempts_for_project(&self, project_id: &str) -> Result<Vec<RunAttempt>> {
		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;
			let attempts = sqlite
				.list_run_attempts_for_project(project_id)?
				.into_iter()
				.map(|attempt| attempt.as_public())
				.collect();

			return Ok(attempts);
		}

		let state = self.lock()?;
		let mut attempts = state
			.run_attempts
			.values()
			.filter(|attempt| attempt.project_id.as_deref() == Some(project_id))
			.map(RunAttemptRecord::as_public)
			.collect::<Vec<_>>();

		attempts.sort_by(|left, right| right.run_id().cmp(left.run_id()));

		Ok(attempts)
	}

	/// Return whether one run already has a matching protocol event.
	pub fn run_has_protocol_event(&self, run_id: &str, event_type: &str) -> Result<bool> {
		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite.run_has_protocol_event(run_id, event_type);
		}

		let state = self.lock()?;

		Ok(state
			.events
			.get(run_id)
			.is_some_and(|events| events.iter().any(|event| event.event_type == event_type)))
	}

	/// List recent run attempts for one project, including lease and protocol summary fields.
	pub fn list_recent_runs(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<ProjectRunStatus>> {
		self.list_project_runs(project_id, limit).map(|(_active, recent)| recent)
	}

	/// List active and recent run attempts for one project from one durable snapshot.
	pub(crate) fn list_project_runs(
		&self,
		project_id: &str,
		base_recent_limit: usize,
	) -> Result<(Vec<ProjectRunStatus>, Vec<ProjectRunStatus>)> {
		self.list_project_runs_with_mode(
			project_id,
			base_recent_limit,
			ProjectRunListingMode::AllowMarkerIdentityPersistence,
		)
	}

	/// List active and recent project runs without persisting marker-derived identities.
	pub(crate) fn list_project_runs_read_only(
		&self,
		project_id: &str,
		base_recent_limit: usize,
	) -> Result<(Vec<ProjectRunStatus>, Vec<ProjectRunStatus>)> {
		self.list_project_runs_with_mode(
			project_id,
			base_recent_limit,
			ProjectRunListingMode::ReadOnly,
		)
	}

	fn list_project_runs_with_mode(
		&self,
		project_id: &str,
		base_recent_limit: usize,
		mode: ProjectRunListingMode,
	) -> Result<(Vec<ProjectRunStatus>, Vec<ProjectRunStatus>)> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_metadata_state_locked(&mut state, project_id)?;

		if matches!(mode, ProjectRunListingMode::AllowMarkerIdentityPersistence) {
			self.refresh_run_attempt_identities_from_worktree_markers_locked(
				&mut state, project_id,
			)?;
		}

		self.refresh_project_loop_evidence_state_locked(&mut state, project_id)?;

		let lease_run_ids = project_lease_run_ids(&state, project_id, None);

		self.refresh_run_activity_summaries_for_runs_locked(&mut state, &lease_run_ids)?;

		let recovery_candidates = project_run_recovery_candidates(&state, project_id, None)?
			.into_iter()
			.filter(|candidate| {
				project_run_recovery_candidate_counts_as_project_run(&state, candidate)
			})
			.collect::<Vec<_>>();
		let recovery_run_ids = recovery_candidates
			.iter()
			.map(|candidate| candidate.run_id().to_owned())
			.collect::<Vec<_>>();

		self.refresh_run_activity_summaries_for_runs_locked(&mut state, &recovery_run_ids)?;
		self.refresh_protocol_event_summaries_for_runs_locked(&mut state, &recovery_run_ids)?;

		let mut runs = state
			.run_attempts
			.values()
			.filter_map(|attempt| state.project_run_status(project_id, attempt))
			.collect::<Vec<_>>();

		runs.extend(
			recovery_candidates.iter().filter_map(|candidate| {
				project_run_status_from_recovery_candidate(&state, candidate)
			}),
		);
		runs.sort_by(compare_project_run_status);

		let leased_runs =
			runs.iter().filter(|status| status.run_lease()).cloned().collect::<Vec<_>>();
		let recent_limit = base_recent_limit.saturating_add(leased_runs.len());
		let recent_run_ids =
			runs.iter().take(recent_limit).map(|run| run.run_id().to_owned()).collect::<Vec<_>>();
		let mut summary_run_ids =
			leased_runs.iter().map(|run| run.run_id().to_owned()).collect::<Vec<_>>();

		summary_run_ids.extend(recent_run_ids);
		summary_run_ids.sort();
		summary_run_ids.dedup();
		self.refresh_protocol_event_summaries_for_runs_locked(&mut state, &summary_run_ids)?;
		self.refresh_run_activity_summaries_for_runs_locked(&mut state, &summary_run_ids)?;

		let summary_run_id_set = summary_run_ids.iter().cloned().collect::<HashSet<_>>();
		let mut selected_runs = state
			.run_attempts
			.values()
			.filter(|attempt| summary_run_id_set.contains(&attempt.run_id))
			.filter_map(|attempt| state.project_run_status(project_id, attempt))
			.collect::<Vec<_>>();

		selected_runs.extend(
			recovery_candidates
				.iter()
				.filter(|candidate| summary_run_id_set.contains(candidate.run_id()))
				.filter_map(|candidate| {
					project_run_status_from_recovery_candidate(&state, candidate)
				}),
		);
		selected_runs.sort_by(compare_project_run_status);

		let leased_runs =
			selected_runs.iter().filter(|status| status.run_lease()).cloned().collect::<Vec<_>>();
		let mut recent_runs = selected_runs;

		recent_runs.truncate(recent_limit);

		Ok((leased_runs, recent_runs))
	}

	/// List all locally recorded run attempts for one issue in one project.
	pub(crate) fn list_project_issue_runs(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<Vec<ProjectRunStatus>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_metadata_state_locked(&mut state, project_id)?;
		self.refresh_run_attempt_identities_from_worktree_markers_locked(&mut state, project_id)?;
		self.refresh_project_loop_evidence_state_locked(&mut state, project_id)?;

		let lease_run_ids = project_lease_run_ids(&state, project_id, Some(issue_id));

		self.refresh_run_activity_summaries_for_runs_locked(&mut state, &lease_run_ids)?;

		let recovery_candidates =
			project_run_recovery_candidates(&state, project_id, Some(issue_id))?;
		let recovery_run_ids = recovery_candidates
			.iter()
			.map(|candidate| candidate.run_id().to_owned())
			.collect::<Vec<_>>();
		let run_ids = state
			.run_attempts
			.values()
			.filter(|attempt| attempt.issue_id == issue_id)
			.map(|attempt| attempt.run_id.clone())
			.collect::<Vec<_>>();
		let mut summary_run_ids = run_ids;

		summary_run_ids.extend(recovery_run_ids);
		summary_run_ids.sort();
		summary_run_ids.dedup();
		self.refresh_run_activity_summaries_for_runs_locked(&mut state, &summary_run_ids)?;
		self.refresh_protocol_event_summaries_for_runs_locked(&mut state, &summary_run_ids)?;

		let mut runs = state
			.run_attempts
			.values()
			.filter(|attempt| attempt.issue_id == issue_id)
			.filter_map(|attempt| state.project_run_status(project_id, attempt))
			.collect::<Vec<_>>();

		runs.extend(
			recovery_candidates.iter().filter_map(|candidate| {
				project_run_status_from_recovery_candidate(&state, candidate)
			}),
		);
		runs.sort_by(compare_project_run_status);

		Ok(runs)
	}

	/// List all leased run attempts for one project without applying the recent-run limit.
	pub fn list_leased_runs(&self, project_id: &str) -> Result<Vec<ProjectRunStatus>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_metadata_state_locked(&mut state, project_id)?;

		let mut runs = state
			.run_attempts
			.values()
			.filter_map(|attempt| {
				let status = state.project_run_status(project_id, attempt)?;

				status.run_lease.then_some(status)
			})
			.collect::<Vec<_>>();
		let mut run_ids = runs.iter().map(|run| run.run_id().to_owned()).collect::<Vec<_>>();

		run_ids.sort();
		run_ids.dedup();
		self.refresh_protocol_event_summaries_for_runs_locked(&mut state, &run_ids)?;

		runs = state
			.run_attempts
			.values()
			.filter(|attempt| run_ids.contains(&attempt.run_id))
			.filter_map(|attempt| {
				let status = state.project_run_status(project_id, attempt)?;

				status.run_lease.then_some(status)
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
		payload: &str,
	) -> Result<()> {
		let now = timestamp_parts();
		let mut state = self.lock_without_refresh()?;
		let event = ProtocolEventRecord {
			sequence_number,
			event_type: event_type.to_owned(),
			payload_sha256: protocol_event_payload_sha256(payload),
			created_at: now.text,
			created_at_unix: now.unix,
		};
		let Some(mut event) =
			self.prepare_protocol_event_for_append_locked(&mut state, run_id, event)?
		else {
			return Ok(());
		};

		loop {
			let (insert_index, cached_existing) = {
				let events = state.events.entry(run_id.to_owned()).or_default();

				match events
					.binary_search_by_key(&event.sequence_number, |event| event.sequence_number)
				{
					Ok(index) => (index, Some(events[index].clone())),
					Err(index) => (index, None),
				}
			};

			if let Some(existing) = cached_existing {
				if self.handle_protocol_event_append_conflict_locked(
					&mut state, run_id, &mut event, &existing,
				)? {
					continue;
				}

				return Ok(());
			}

			if !self.append_protocol_event_locked(run_id, &event)? {
				let existing =
					self.protocol_event_locked(run_id, event.sequence_number)?.ok_or_else(|| {
						eyre::eyre!(
							"Protocol event `{run_id}` sequence `{}` already exists in the runtime journal, but the existing row could not be read.",
							event.sequence_number
						)
					})?;

				if self.handle_protocol_event_append_conflict_locked(
					&mut state, run_id, &mut event, &existing,
				)? {
					continue;
				}

				return Ok(());
			}

			self.record_inserted_protocol_event_locked(&mut state, run_id, insert_index, event)?;

			return Ok(());
		}
	}

	fn prepare_protocol_event_for_append_locked(
		&self,
		state: &mut StateData,
		run_id: &str,
		event: ProtocolEventRecord,
	) -> Result<Option<ProtocolEventRecord>> {
		if !self.protocol_event_should_be_discarded_after_archive_locked(state, run_id, &event)? {
			return Ok(Some(event));
		}
		if self.protocol_event_replay_already_recorded_locked(state, run_id, &event)? {
			self.refresh_protocol_event_summaries_for_runs_locked(state, &[run_id.to_owned()])?;

			return Ok(None);
		}

		Ok(Some(discarded_post_archive_protocol_event_with_log(run_id, event)))
	}

	fn handle_protocol_event_append_conflict_locked(
		&self,
		state: &mut StateData,
		run_id: &str,
		event: &mut ProtocolEventRecord,
		existing: &ProtocolEventRecord,
	) -> Result<bool> {
		if protocol_event_conflict_should_be_discarded_after_archive(existing, event) {
			*event = discarded_post_archive_protocol_event_with_log(run_id, event.clone());

			return Ok(true);
		}
		if protocol_event_is_discarded_post_archive_collision(existing, event) {
			event.sequence_number =
				next_discarded_post_archive_sequence_after_collision(event.sequence_number)?;

			return Ok(true);
		}

		ensure_protocol_event_replay_matches(run_id, existing, event)?;

		self.refresh_protocol_event_summaries_for_runs_locked(state, &[run_id.to_owned()])?;

		Ok(false)
	}

	fn record_inserted_protocol_event_locked(
		&self,
		state: &mut StateData,
		run_id: &str,
		insert_index: usize,
		event: ProtocolEventRecord,
	) -> Result<()> {
		let had_cached_summary = state.event_summaries.contains_key(run_id);
		let inserted_event = event.clone();

		state.events.entry(run_id.to_owned()).or_default().insert(insert_index, event);

		if self.sqlite.is_some() && !had_cached_summary {
			self.refresh_protocol_event_summaries_for_runs_locked(state, &[run_id.to_owned()])?;
		} else if self.sqlite.is_some() {
			let summary = state.event_summaries.entry(run_id.to_owned()).or_default();

			if summary.last_sequence_number.is_none_or(|last_sequence_number| {
				inserted_event.sequence_number == last_sequence_number.saturating_add(1)
			}) {
				summary.record_event(&inserted_event);
				let summary = summary.clone();

				self.upsert_protocol_event_summary_locked(run_id, &summary)?;
			} else {
				self.refresh_protocol_event_summaries_for_runs_locked(state, &[run_id.to_owned()])?;
			}
		} else if let Some(events) = state.events.get(run_id) {
			let summary = protocol_event_summary_from_events(events);

			state.event_summaries.insert(run_id.to_owned(), summary);
		}

		Ok(())
	}

	fn protocol_event_should_be_discarded_after_archive_locked(
		&self,
		state: &StateData,
		run_id: &str,
		event: &ProtocolEventRecord,
	) -> Result<bool> {
		if !protocol_event_can_be_discarded_after_archive(event) {
			return Ok(false);
		}

		self.run_has_terminal_thread_archive_event_locked(state, run_id)
	}

	fn run_has_terminal_thread_archive_event_locked(
		&self,
		state: &StateData,
		run_id: &str,
	) -> Result<bool> {
		if state.events.get(run_id).is_some_and(|events| {
			events.iter().any(|event| protocol_event_is_terminal_thread_archive(&event.event_type))
		}) {
			return Ok(true);
		}
		if state.event_summaries.get(run_id).is_some_and(|summary| {
			summary
				.last_event_type
				.as_deref()
				.is_some_and(protocol_event_is_terminal_thread_archive)
		}) {
			return Ok(true);
		}

		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(false);
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		for event_type in TERMINAL_THREAD_ARCHIVE_EVENT_TYPES {
			if sqlite.run_has_protocol_event(run_id, event_type)? {
				return Ok(true);
			}
		}

		Ok(false)
	}

	fn protocol_event_replay_already_recorded_locked(
		&self,
		state: &StateData,
		run_id: &str,
		event: &ProtocolEventRecord,
	) -> Result<bool> {
		if let Some(events) = state.events.get(run_id)
			&& let Ok(index) =
				events.binary_search_by_key(&event.sequence_number, |event| event.sequence_number)
		{
			return Ok(events[index].is_idempotent_replay_of(event));
		}

		let Some(existing) = self.protocol_event_locked(run_id, event.sequence_number)? else {
			return Ok(false);
		};

		Ok(existing.is_idempotent_replay_of(event))
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
		let mut state = self.lock_without_refresh()?;
		let now_unix = OffsetDateTime::now_utc().unix_timestamp();
		let created_at_unix = state
			.worktrees
			.get(issue_id)
			.and_then(|mapping| mapping.created_at_unix)
			.or(Some(now_unix));
		let mapping = WorktreeMappingRecord {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			branch_name: branch_name.to_owned(),
			worktree_path: PathBuf::from(worktree_path),
			provenance_source: WORKTREE_PROVENANCE_RUNTIME_RECORDED.to_owned(),
			created_at_unix,
			updated_at_unix: Some(now_unix),
		};

		state.worktrees.insert(issue_id.to_owned(), mapping.clone());
		state.remember_run_project(project_id, issue_id, None);

		self.upsert_worktree_and_remember_run_project_locked(&mapping)
	}

	/// Create or refresh a worktree mapping reconstructed from retained local state.
	pub(crate) fn upsert_recovered_worktree(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
		worktree_path: &str,
		observed_at_unix: Option<i64>,
	) -> Result<()> {
		let mut state = self.lock_without_refresh()?;
		let existing = state.worktrees.get(issue_id);
		let existing_provenance_source = existing.map(|mapping| mapping.provenance_source.as_str());
		let provenance_source = match existing_provenance_source {
			Some(WORKTREE_PROVENANCE_RUNTIME_RECORDED) => WORKTREE_PROVENANCE_RUNTIME_RECORDED,
			Some(WORKTREE_PROVENANCE_RUNTIME_RECOVERED) => WORKTREE_PROVENANCE_RUNTIME_RECOVERED,
			_ => WORKTREE_PROVENANCE_RUNTIME_RECOVERED,
		}
		.to_owned();
		let existing_created_at_unix = existing.and_then(|mapping| mapping.created_at_unix);
		let existing_updated_at_unix = existing.and_then(|mapping| mapping.updated_at_unix);
		let created_at_unix = existing_created_at_unix.or(observed_at_unix);
		let updated_at_unix = match (existing_updated_at_unix, observed_at_unix) {
			(Some(existing), Some(observed)) => Some(existing.max(observed)),
			(Some(existing), None) => Some(existing),
			(None, observed) => observed,
		};
		let mapping = WorktreeMappingRecord {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			branch_name: branch_name.to_owned(),
			worktree_path: PathBuf::from(worktree_path),
			provenance_source,
			created_at_unix,
			updated_at_unix,
		};

		state.worktrees.insert(issue_id.to_owned(), mapping.clone());
		state.remember_run_project(project_id, issue_id, None);

		self.upsert_worktree_and_remember_run_project_locked(&mapping)
	}

	/// Create or replace the retained review handoff projection for one issue lane.
	pub(crate) fn upsert_review_handoff_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		marker: &ReviewHandoffMarker,
	) -> Result<()> {
		let now = timestamp_parts();
		let key = ReviewLifecycleKey::new(project_id, issue_id, marker.branch_name());
		let mut state = self.lock()?;
		let record = state.review_lifecycle_records.entry(key).or_insert_with(|| {
			ReviewLifecycleRuntimeRecord {
				project_id: project_id.to_owned(),
				issue_id: issue_id.to_owned(),
				branch_name: marker.branch_name().to_owned(),
				run_id: marker.run_id().to_owned(),
				attempt_number: marker.attempt_number(),
				pr_url: marker.pr_url().to_owned(),
				target_base_ref_name: marker.target_base_ref_name().map(str::to_owned),
				pr_head_ref_name: marker.pr_head_ref_name().to_owned(),
				pr_head_oid: marker.pr_head_oid().to_owned(),
				head_sha: marker.pr_head_oid().to_owned(),
				phase: String::from("request_pending"),
				request_comment_database_id: None,
				request_created_at_unix_epoch: None,
				request_description_thumbs_up_count: None,
				request_retry_count: 0,
				external_round_count: 0,
				auto_merge_enabled_at_unix_epoch: None,
				landing_state: String::from("not_started"),
				closeout_state: String::from("not_started"),
				repair_attempt_count: 0,
				evidence_json: String::from("{}"),
				next_action: String::new(),
				updated_at: now.text.clone(),
				updated_at_unix: now.unix,
			}
		});
		let same_handoff_projection = record.run_id == marker.run_id()
			&& record.attempt_number == marker.attempt_number()
			&& record.pr_url == marker.pr_url()
			&& record.target_base_ref_name.as_deref() == marker.target_base_ref_name()
			&& record.pr_head_ref_name == marker.pr_head_ref_name()
			&& record.pr_head_oid == marker.pr_head_oid();

		record.run_id = marker.run_id().to_owned();
		record.attempt_number = marker.attempt_number();
		record.pr_url = marker.pr_url().to_owned();
		record.target_base_ref_name = marker.target_base_ref_name().map(str::to_owned);
		record.pr_head_ref_name = marker.pr_head_ref_name().to_owned();
		record.pr_head_oid = marker.pr_head_oid().to_owned();

		if !same_handoff_projection {
			record.head_sha = marker.pr_head_oid().to_owned();
			record.phase = String::from("request_pending");
			record.request_comment_database_id = None;
			record.request_created_at_unix_epoch = None;
			record.request_description_thumbs_up_count = None;
			record.request_retry_count = 0;
			record.external_round_count = 0;
			record.auto_merge_enabled_at_unix_epoch = None;
			record.landing_state = String::from("not_started");
			record.closeout_state = String::from("not_started");
			record.repair_attempt_count = 0;
			record.evidence_json = String::from("{}");

			record.next_action.clear();
		}

		record.updated_at = now.text;
		record.updated_at_unix = now.unix;

		self.persist_runtime_state_locked(&state)
	}

	/// Read the retained review handoff projection for one issue branch.
	pub(crate) fn review_handoff_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
	) -> Result<Option<ReviewHandoffMarker>> {
		Ok(self.review_lifecycle_record(project_id, issue_id, branch_name)?.map(|record| {
			ReviewHandoffMarker {
				run_id: record.run_id().to_owned(),
				attempt_number: record.attempt_number(),
				branch_name: record.branch_name().to_owned(),
				pr_url: record.pr_url().to_owned(),
				target_base_ref_name: record.target_base_ref_name().map(str::to_owned),
				pr_head_ref_name: record.pr_head_ref_name().to_owned(),
				pr_head_oid: record.pr_head_oid().to_owned(),
			}
		}))
	}

	/// Read the runtime-owned review lifecycle record for one retained issue branch.
	pub(crate) fn review_lifecycle_record(
		&self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
	) -> Result<Option<ReviewLifecycleRecord>> {
		let state = self.lock()?;
		let key = ReviewLifecycleKey::new(project_id, issue_id, branch_name);

		Ok(state.review_lifecycle_records.get(&key).map(ReviewLifecycleRuntimeRecord::as_public))
	}

	/// Return whether any retained review lifecycle row owns this issue.
	pub(crate) fn issue_has_review_lifecycle_record(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<bool> {
		let state = self.lock()?;

		Ok(state
			.review_lifecycle_records
			.values()
			.any(|record| record.project_id == project_id && record.issue_id == issue_id))
	}

	/// Create or replace the retained review orchestration projection for one issue lane.
	pub(crate) fn upsert_review_orchestration_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		marker: &ReviewOrchestrationMarker,
	) -> Result<()> {
		let now = timestamp_parts();
		let key = ReviewLifecycleKey::new(project_id, issue_id, marker.branch_name());
		let mut state = self.lock()?;
		let record = state.review_lifecycle_records.entry(key).or_insert_with(|| {
			ReviewLifecycleRuntimeRecord {
				project_id: project_id.to_owned(),
				issue_id: issue_id.to_owned(),
				branch_name: marker.branch_name().to_owned(),
				run_id: marker.run_id().to_owned(),
				attempt_number: marker.attempt_number(),
				pr_url: marker.pr_url().to_owned(),
				target_base_ref_name: None,
				pr_head_ref_name: marker.branch_name().to_owned(),
				pr_head_oid: marker.head_sha().to_owned(),
				head_sha: marker.head_sha().to_owned(),
				phase: marker.phase().to_owned(),
				request_comment_database_id: None,
				request_created_at_unix_epoch: None,
				request_description_thumbs_up_count: None,
				request_retry_count: 0,
				external_round_count: 0,
				auto_merge_enabled_at_unix_epoch: None,
				landing_state: String::from("not_started"),
				closeout_state: String::from("not_started"),
				repair_attempt_count: 0,
				evidence_json: String::from("{}"),
				next_action: String::new(),
				updated_at: now.text.clone(),
				updated_at_unix: now.unix,
			}
		});

		record.run_id = marker.run_id().to_owned();
		record.attempt_number = marker.attempt_number();
		record.pr_url = marker.pr_url().to_owned();
		record.head_sha = marker.head_sha().to_owned();
		record.phase = marker.phase().to_owned();
		record.request_comment_database_id = marker.request_comment_database_id();
		record.request_created_at_unix_epoch = marker.request_created_at_unix_epoch();
		record.request_description_thumbs_up_count = marker.request_description_thumbs_up_count();
		record.request_retry_count = marker.request_retry_count();
		record.external_round_count = marker.external_round_count();
		record.auto_merge_enabled_at_unix_epoch = marker.auto_merge_enabled_at_unix_epoch();
		record.updated_at = now.text;
		record.updated_at_unix = now.unix;

		self.persist_runtime_state_locked(&state)
	}

	/// Read retained review orchestration for the current handoff identity.
	pub(crate) fn review_orchestration_marker(
		&self,
		project_id: &str,
		issue_id: &str,
		review_handoff: &ReviewHandoffMarker,
	) -> Result<Option<ReviewOrchestrationMarker>> {
		let Some(record) =
			self.review_lifecycle_record(project_id, issue_id, review_handoff.branch_name())?
		else {
			return Ok(None);
		};

		if record.run_id() != review_handoff.run_id()
			|| record.attempt_number() != review_handoff.attempt_number()
			|| record.branch_name() != review_handoff.branch_name()
			|| record.pr_url() != review_handoff.pr_url()
		{
			return Ok(None);
		}

		Ok(Some(ReviewOrchestrationMarker::new(
			record.run_id().to_owned(),
			record.attempt_number(),
			record.branch_name().to_owned(),
			record.pr_url().to_owned(),
			record.head_sha().to_owned(),
			record.phase().to_owned(),
			record.request_comment_database_id(),
			record.request_created_at_unix_epoch(),
			record.request_description_thumbs_up_count(),
			record.request_retry_count(),
			record.external_round_count(),
			record.auto_merge_enabled_at_unix_epoch(),
		)))
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
			details_json: input.details_json.to_owned(),
			updated_at: now.text.clone(),
			updated_at_unix: now.unix,
		};
		let mut state = self.lock()?;

		state.review_policy_checkpoints.insert(key, record.clone());

		let artifact = evidence_artifact_from_review_checkpoint_input(input, &record, &now)?;
		let artifact_key = EvidenceArtifactKey::new(
			&artifact.project_id,
			&artifact.issue_id,
			&artifact.artifact_kind,
			&artifact.key_hash,
		);

		state.evidence_artifacts.insert(artifact_key, artifact);
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

	/// Return whether any bounded-review checkpoint row owns this issue.
	pub(crate) fn issue_has_review_policy_checkpoint(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<bool> {
		let state = self.lock()?;

		Ok(state
			.review_policy_checkpoints
			.values()
			.any(|record| record.project_id == project_id && record.issue_id == issue_id))
	}

	/// Read the latest review checkpoint by its canonical reusable evidence key.
	pub(crate) fn review_checkpoint_artifact(
		&self,
		lookup: ReviewCheckpointArtifactLookup<'_>,
	) -> Result<Option<ReviewPolicyCheckpoint>> {
		let state = self.lock()?;
		let key_json = review_checkpoint_evidence_key_json(
			lookup.phase,
			lookup.review_level,
			lookup.head_sha,
		)?;
		let key_hash = evidence_artifact_key_hash("issue_review_checkpoint", &key_json);
		let key = EvidenceArtifactKey::new(
			lookup.project_id,
			lookup.issue_id,
			"issue_review_checkpoint",
			&key_hash,
		);

		state
			.evidence_artifacts
			.get(&key)
			.map(EvidenceArtifactRuntimeRecord::as_review_policy_checkpoint)
			.transpose()
	}

	/// Check whether review policy has any non-clean artifact that could fence mutation.
	pub(crate) fn has_nonclean_review_checkpoint_artifact(
		&self,
		project_id: &str,
		issue_id: &str,
		phase: &str,
	) -> Result<bool> {
		let state = self.lock()?;

		Ok(state.evidence_artifacts.values().any(|record| {
			record.project_id == project_id
				&& record.issue_id == issue_id
				&& record.artifact_kind == "issue_review_checkpoint"
				&& record.phase == phase
				&& record.status != "clean"
		}))
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

	/// Record one loop-guardrail observation and return its consecutive count.
	pub(crate) fn observe_loop_guardrail_checkpoint(
		&self,
		input: LoopGuardrailCheckpointInput<'_>,
	) -> Result<LoopGuardrailCheckpoint> {
		let now = timestamp_parts();
		let key = LoopGuardrailKey::new(input.project_id, input.issue_id, input.reason);
		let mut state = self.lock()?;
		let previous = state.loop_guardrail_checkpoints.get(&key);
		let consecutive_count = previous.map_or(1, |record| {
			if record.fingerprint == input.fingerprint {
				record.consecutive_count.saturating_add(1)
			} else {
				1
			}
		});
		let record = LoopGuardrailRuntimeRecord {
			project_id: input.project_id.to_owned(),
			issue_id: input.issue_id.to_owned(),
			reason: input.reason.to_owned(),
			fingerprint: input.fingerprint.to_owned(),
			run_id: input.run_id.to_owned(),
			attempt_number: input.attempt_number,
			consecutive_count,
			details_json: input.details_json.to_owned(),
			updated_at: now.text,
			updated_at_unix: now.unix,
		};

		state.loop_guardrail_checkpoints.insert(key, record.clone());
		self.persist_runtime_state_locked(&state)?;

		Ok(record.as_public())
	}

	/// Read one loop-guardrail checkpoint by project, issue, and reason.
	#[cfg(test)]
	pub(crate) fn loop_guardrail_checkpoint(
		&self,
		project_id: &str,
		issue_id: &str,
		reason: &str,
	) -> Result<Option<LoopGuardrailCheckpoint>> {
		let state = self.lock()?;
		let key = LoopGuardrailKey::new(project_id, issue_id, reason);

		Ok(state.loop_guardrail_checkpoints.get(&key).map(LoopGuardrailRuntimeRecord::as_public))
	}

	/// Clear loop-guardrail checkpoints for one issue.
	pub(crate) fn clear_loop_guardrail_checkpoints_for_issue(
		&self,
		project_id: &str,
		issue_id: &str,
	) -> Result<()> {
		let mut state = self.lock()?;

		state
			.loop_guardrail_checkpoints
			.retain(|key, _record| key.project_id != project_id || key.issue_id != issue_id);

		self.delete_loop_guardrail_checkpoints_for_issue_locked(project_id, issue_id)
	}

	/// Clear one loop-guardrail checkpoint reason for one issue.
	pub(crate) fn clear_loop_guardrail_checkpoint(
		&self,
		project_id: &str,
		issue_id: &str,
		reason: &str,
	) -> Result<()> {
		let key = LoopGuardrailKey::new(project_id, issue_id, reason);
		let mut state = self.lock()?;

		state.loop_guardrail_checkpoints.remove(&key);

		self.delete_loop_guardrail_checkpoint_locked(project_id, issue_id, reason)
	}

	/// Remove the exact review lifecycle record created for one handoff identity.
	pub(crate) fn clear_review_lifecycle_for_handoff(
		&self,
		project_id: &str,
		issue_id: &str,
		handoff_marker: &ReviewHandoffMarker,
		orchestration_marker: &ReviewOrchestrationMarker,
	) -> Result<()> {
		let lifecycle_key =
			ReviewLifecycleKey::new(project_id, issue_id, handoff_marker.branch_name());
		let mut state = self.lock()?;

		if state
			.review_lifecycle_records
			.get(&lifecycle_key)
			.is_some_and(|record| record.matches_handoff_identity(handoff_marker))
		{
			state.review_lifecycle_records.remove(&lifecycle_key);
		}

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
		if let Some(sqlite) = &self.sqlite {
			let sqlite = sqlite.lock().map_err(|_| eyre::eyre!("State store lock poisoned."))?;

			return sqlite
				.worktree_for_issue(issue_id)
				.map(|mapping| mapping.map(|mapping| mapping.as_public()));
		}

		let state = self.lock()?;

		Ok(state.worktrees.get(issue_id).map(WorktreeMappingRecord::as_public))
	}

	/// List all known worktree mappings.
	pub fn list_worktrees(&self, project_id: &str) -> Result<Vec<WorktreeMapping>> {
		let mut state = self.lock_without_refresh()?;

		self.refresh_project_run_metadata_state_locked(&mut state, project_id)?;

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
		state.review_lifecycle_records.retain(|key, _record| key.issue_id != issue_id);
		state.review_policy_checkpoints.retain(|key, _record| key.issue_id != issue_id);
		self.persist_runtime_state_locked(&state)?;

		self.delete_worktree_and_review_lifecycle_locked(issue_id)
	}

	/// Remove only the worktree mapping for one issue.
	pub(crate) fn clear_worktree_mapping(&self, issue_id: &str) -> Result<()> {
		let mut state = self.lock()?;

		state.worktrees.remove(issue_id);
		self.persist_runtime_state_locked(&state)?;

		self.delete_worktree_mapping_locked(issue_id)
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

	fn refresh_protocol_event_summaries_for_runs_locked(
		&self,
		state: &mut StateData,
		run_ids: &[String],
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.load_protocol_event_summaries_for_runs(state, run_ids)
	}

	fn upsert_protocol_event_summary_locked(
		&self,
		run_id: &str,
		summary: &ProtocolEventSummaryRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_protocol_event_summary(run_id, summary)
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

	fn append_protocol_event_locked(
		&self,
		run_id: &str,
		event: &ProtocolEventRecord,
	) -> Result<bool> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(true);
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.append_protocol_event(run_id, event)
	}

	fn protocol_event_locked(
		&self,
		run_id: &str,
		sequence_number: i64,
	) -> Result<Option<ProtocolEventRecord>> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(None);
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.protocol_event(run_id, sequence_number)
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


fn evidence_artifact_from_review_checkpoint_input(
	input: ReviewPolicyCheckpointInput<'_>,
	record: &ReviewPolicyRuntimeRecord,
	now: &TimestampParts,
) -> Result<EvidenceArtifactRuntimeRecord> {
	let key_json =
		review_checkpoint_evidence_key_json(input.phase, input.review_level, input.head_sha)?;
	let payload_json = serde_json::json!({
		"schema": "decodex.review_checkpoint_artifact/1",
		"phase": input.phase,
		"review_level": input.review_level,
		"status": input.status,
		"head_sha": input.head_sha,
		"nonclean_rounds": input.nonclean_rounds,
		"details_json": input.details_json,
		"source": {
			"run_id": input.run_id,
			"attempt_number": input.attempt_number
		}
	});
	let key_hash = evidence_artifact_key_hash("issue_review_checkpoint", &key_json);

	Ok(EvidenceArtifactRuntimeRecord {
		project_id: record.project_id.clone(),
		issue_id: record.issue_id.clone(),
		artifact_kind: String::from("issue_review_checkpoint"),
		key_hash,
		phase: record.phase.clone(),
		status: record.status.clone(),
		head_sha: Some(record.head_sha.clone()),
		key_json,
		payload_json: payload_json.to_string(),
		source_run_id: record.run_id.clone(),
		source_attempt_number: record.attempt_number,
		updated_at: now.text.clone(),
		updated_at_unix: now.unix,
	})
}

fn review_checkpoint_evidence_key_json(
	phase: &str,
	review_level: &str,
	head_sha: &str,
) -> Result<String> {
	#[derive(Serialize)]
	struct ReviewCheckpointEvidenceKey<'a> {
		schema: &'static str,
		artifact_kind: &'static str,
		phase: &'a str,
		head_sha: &'a str,
		review_level: &'a str,
		review_prompt_version: &'static str,
	}

	serde_json::to_string(&ReviewCheckpointEvidenceKey {
		schema: "decodex.evidence_key/1",
		artifact_kind: "issue_review_checkpoint",
		phase,
		head_sha,
		review_level,
		review_prompt_version: REVIEW_CHECKPOINT_PROMPT_VERSION,
	})
	.map_err(|error| eyre::eyre!("failed to serialize review checkpoint evidence key: {error}"))
}

fn evidence_artifact_key_hash(artifact_kind: &str, key_json: &str) -> String {
	let payload = format!("{artifact_kind}\n{key_json}");
	let digest = Sha256::digest(payload.as_bytes());
	let mut hash = String::with_capacity(64);

	for byte in digest {
		hash.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		hash.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	hash
}

fn protocol_event_payload_sha256(payload: &str) -> String {
	let digest = Sha256::digest(payload.as_bytes());
	let mut hash = String::with_capacity(64);

	for byte in digest {
		hash.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		hash.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	hash
}

fn protocol_event_is_terminal_thread_archive(event_type: &str) -> bool {
	TERMINAL_THREAD_ARCHIVE_EVENT_TYPES.contains(&event_type)
}

fn protocol_event_can_be_discarded_after_archive(event: &ProtocolEventRecord) -> bool {
	!protocol_event_is_terminal_thread_archive(&event.event_type)
		&& event.event_type != DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE
}

fn protocol_event_conflict_should_be_discarded_after_archive(
	existing: &ProtocolEventRecord,
	candidate: &ProtocolEventRecord,
) -> bool {
	protocol_event_is_terminal_thread_archive(&existing.event_type)
		&& protocol_event_can_be_discarded_after_archive(candidate)
}

fn protocol_event_is_discarded_post_archive_collision(
	existing: &ProtocolEventRecord,
	candidate: &ProtocolEventRecord,
) -> bool {
	existing.event_type == DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE
		&& candidate.event_type == DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE
		&& !existing.is_idempotent_replay_of(candidate)
}

fn discarded_post_archive_protocol_event(mut event: ProtocolEventRecord) -> ProtocolEventRecord {
	if event.event_type == DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE {
		return event;
	}

	event.sequence_number = discarded_post_archive_protocol_sequence(&event);
	event.event_type = DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE.to_owned();

	event
}

fn discarded_post_archive_protocol_event_with_log(
	run_id: &str,
	event: ProtocolEventRecord,
) -> ProtocolEventRecord {
	let original_sequence_number = event.sequence_number;
	let original_event_type = event.event_type.clone();
	let discarded = discarded_post_archive_protocol_event(event);

	tracing::info!(
		run_id,
		original_sequence_number,
		original_event_type,
		discarded_sequence_number = discarded.sequence_number,
		discarded_event_type = discarded.event_type.as_str(),
		"Discarded late app-server protocol event after terminal thread archive barrier; child protocol activity is isolated from parent journal and closeout state."
	);

	discarded
}

fn discarded_post_archive_protocol_sequence(event: &ProtocolEventRecord) -> i64 {
	let payload =
		format!("{}\n{}\n{}", event.sequence_number, event.event_type, event.payload_sha256);
	let digest = Sha256::digest(payload.as_bytes());
	let mut bytes = [0_u8; 8];

	bytes.copy_from_slice(&digest[..8]);

	let slot = i64::from_be_bytes(bytes) & i64::MAX;

	if slot == i64::MAX { i64::MIN } else { -1 - slot }
}

fn next_discarded_post_archive_sequence_after_collision(sequence_number: i64) -> Result<i64> {
	if sequence_number == i64::MIN {
		eyre::bail!("Post-archive discarded protocol event sequence space is exhausted.");
	}

	Ok(sequence_number - 1)
}

fn ensure_protocol_event_replay_matches(
	run_id: &str,
	existing: &ProtocolEventRecord,
	candidate: &ProtocolEventRecord,
) -> Result<()> {
	if existing.is_idempotent_replay_of(candidate) {
		return Ok(());
	}

	eyre::bail!(
		"Protocol event `{run_id}` sequence `{}` conflicts with an existing runtime journal event: \
		 existing event_type=`{}` payload_sha256=`{}`, candidate event_type=`{}` payload_sha256=`{}`.",
		candidate.sequence_number,
		existing.event_type,
		existing.payload_sha256,
		candidate.event_type,
		candidate.payload_sha256,
	);
}
