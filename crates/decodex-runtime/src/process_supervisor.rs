//! Sole runtime writer and positive-only reconciler for durable ProcessGeneration authority.
//!
//! This service has no account selector, routing input, RuntimeSession constructor, provider
//! request, or production dispatch gate. Restored processes can be observed but are never
//! adopted, reacquired, proxied, or signaled.

use std::{
	collections::{BTreeMap, BTreeSet, btree_map::Entry},
	fmt::{Display, Formatter},
	future::Future,
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, Ordering},
	},
	time::{Duration, Instant},
};

use decodex_core::{
	AccountId, ProcessAccountQuarantine, ProcessAuthorityLossReason, ProcessBootIdentity,
	ProcessDeathEvidence, ProcessDeathEvidenceId, ProcessDeathEvidenceKind,
	ProcessExecutionAuthorization, ProcessGeneration, ProcessGenerationAccountBinding,
	ProcessGenerationId, ProcessGenerationIntent, ProcessGenerationState, ProcessIdentity,
};
use decodex_database::{
	FreshConversationProcessGeneration, PrepareProcessGenerationOutcome, ProcessGenerationMutation,
	ProcessGenerationMutationOutcome, SqliteStore,
};
use sha2::{Digest as _, Sha256};

use crate::{
	account_launch::{AttestedAppServerLaunch, AttestedProcessChild},
	process_platform::{self, ExactProcessObservation, KernelExitWitness, ProcessPlatformError},
};

const RECONCILIATION_PAGE_SIZE: u16 = 256;
const RECONCILIATION_INTERVAL: Duration = Duration::from_secs(5);
const FAILED_SPAWN_CLEANUP: Duration = Duration::from_secs(1);
const MAX_TERMINATION_WAIT: Duration = Duration::from_secs(30);

/// Authority-bound operator/runtime port for diagnostics, reconciliation, and termination.
#[derive(Clone)]
pub struct ProcessGenerationControl {
	inner: Arc<ProcessSupervisor>,
}

/// Sole in-process owner of every durable ProcessGeneration mutation capability.
struct ProcessSupervisor {
	store: SqliteStore,
	boot_id: ProcessBootIdentity,
	owned: OwnedGenerationRegistry<OwnedGeneration>,
	observers: Mutex<BTreeMap<String, KernelExitWitness>>,
	pending_non_creation: Mutex<BTreeSet<ProcessGenerationId>>,
	supervised: Mutex<BTreeSet<String>>,
}

struct OwnedGenerationSlot<T> {
	admitted: AtomicBool,
	value: Mutex<Option<T>>,
}

/// Two-level ownership table: membership is global, but child access is generation-local.
///
/// Removing a slot closes admission before waiting for an operation already in flight.
struct OwnedGenerationRegistry<T> {
	entries: Mutex<BTreeMap<String, Arc<OwnedGenerationSlot<T>>>>,
}
impl<T> OwnedGenerationRegistry<T> {
	fn new() -> Self {
		Self { entries: Mutex::new(BTreeMap::new()) }
	}

	fn contains(&self, key: &str) -> Result<bool, ProcessSupervisorError> {
		Ok(self
			.entries
			.lock()
			.map_err(|_| ProcessSupervisorError::AuthorityConflict)?
			.contains_key(key))
	}

	fn insert(&self, key: String, value: T) -> Result<bool, ProcessSupervisorError> {
		let mut entries =
			self.entries.lock().map_err(|_| ProcessSupervisorError::AuthorityConflict)?;
		match entries.entry(key) {
			Entry::Occupied(_) => Ok(false),
			Entry::Vacant(entry) => {
				entry.insert(Arc::new(OwnedGenerationSlot {
					admitted: AtomicBool::new(true),
					value: Mutex::new(Some(value)),
				}));
				Ok(true)
			},
		}
	}

	fn take(&self, key: &str) -> Result<Option<T>, ProcessSupervisorError> {
		let slot = {
			let mut entries =
				self.entries.lock().map_err(|_| ProcessSupervisorError::AuthorityConflict)?;
			let slot = entries.remove(key);
			if let Some(slot) = slot.as_ref() {
				slot.admitted.store(false, Ordering::Release);
			}
			slot
		};
		let Some(slot) = slot else {
			return Ok(None);
		};
		let mut value = slot.value.lock().map_err(|_| ProcessSupervisorError::AuthorityConflict)?;
		Ok(value.take())
	}

	fn with_current<R>(
		&self,
		key: &str,
		operation: impl FnOnce(&mut T) -> R,
	) -> Result<Option<R>, ProcessSupervisorError> {
		let slot = self
			.entries
			.lock()
			.map_err(|_| ProcessSupervisorError::AuthorityConflict)?
			.get(key)
			.cloned();
		let Some(slot) = slot else {
			return Ok(None);
		};

		let mut value = slot.value.lock().map_err(|_| ProcessSupervisorError::AuthorityConflict)?;
		if !slot.admitted.load(Ordering::Acquire) {
			return Ok(None);
		}
		let Some(value) = value.as_mut() else {
			return Ok(None);
		};
		Ok(Some(operation(value)))
	}
}

struct OwnedGeneration {
	child: AttestedProcessChild,
	process_group_id: u32,
	identity: Option<ProcessIdentity>,
	revision: i64,
	leader_exited: bool,
}

struct SupervisionReservation {
	inner: Arc<ProcessSupervisor>,
	key: String,
	retained: bool,
}
impl SupervisionReservation {
	fn retain(&mut self) {
		self.retained = true;
	}
}
impl Drop for SupervisionReservation {
	fn drop(&mut self) {
		if !self.retained
			&& let Ok(mut supervised) = self.inner.supervised.lock()
		{
			supervised.remove(&self.key);
		}
	}
}

/// Exact bounded diagnostic for one durable generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessGenerationDiagnostic {
	/// Complete durable projection. It never contains the external epoch authorization digest.
	pub generation: ProcessGeneration,
	/// Immutable account/store/provider/callback launch binding persisted with V27 generations.
	pub account_binding: Option<ProcessGenerationAccountBinding>,
	/// Current positive or explicitly inconclusive host observation.
	pub observation: ProcessGenerationObservation,
}
impl ProcessGenerationDiagnostic {
	/// Derive the affected-account quarantine. Dead generations return `None`.
	pub fn quarantine(&self) -> Option<ProcessAccountQuarantine> {
		self.generation.state.quarantines_account().then(|| ProcessAccountQuarantine {
			account_id: self.generation.account_id.clone(),
			generation_id: self.generation.generation_id.clone(),
			state: self.generation.state,
			has_process_identity: self.generation.process_identity.is_some(),
		})
	}
}

/// Kernel source that positively observed the exact restored process leader exit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessGenerationExitWitnessKind {
	/// Linux pidfd reported exit for the exact persisted process identity.
	LinuxPidfd,
	/// macOS kqueue reported `EVFILT_PROC/NOTE_EXIT` for the exact persisted process identity.
	MacosKqueueNoteExit,
}

/// Closed diagnostic observation. Negative observations never imply death.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessGenerationObservation {
	/// Positive death evidence is already durable.
	Dead,
	/// Current boot differs, which is positive proof that the prior boot ended.
	PriorBootEnded,
	/// The original supervisor still owns the unreaped child handle.
	Owned,
	/// A spawn or exact termination is active under the original supervisor.
	SupervisionInFlight,
	/// Spawn positively failed and durable evidence is pending retry.
	SpawnNonCreationPending,
	/// A read-only kernel exit witness is attached to the exact restored identity.
	PositiveExitObserverAttached,
	/// The exact witness observed exit; group quiescence controls durable death evidence.
	PositiveExitObserved {
		/// Exact kernel source that observed the process leader exit.
		witness_kind: ProcessGenerationExitWitnessKind,
		/// Whether the exact process group is now quiescent.
		process_group_quiescent: bool,
	},
	/// Same-boot exact identity is present, but no observer is currently attached.
	SameBootExactIdentityPresent,
	/// The PID could not be inspected. This is explicitly not death evidence.
	SameBootNotObserved,
	/// The PID names different exact facts. This is explicitly not death evidence.
	SameBootIdentityMismatch {
		/// Complete current identity facts for operator comparison.
		observed: ProcessIdentity,
	},
	/// No exact process identity was persisted before authority loss.
	SameBootUnbound,
	/// The supported-OS adapter could not obtain a safe observation.
	ObservationUnavailable,
}

/// Result of one positive-only reconciliation request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessGenerationReconciliation {
	/// Positive death evidence was already durable.
	AlreadyDead,
	/// Positive generation-bound evidence committed now or was read back exactly.
	PositiveDeathRecorded,
	/// No positive proof exists. Only this generation's account remains quarantined.
	Quarantined {
		/// Exact durable nonterminal state.
		state: ProcessGenerationState,
		/// Diagnostic reason that does not grant replacement.
		observation: ProcessGenerationObservation,
	},
	/// The exact generation does not exist.
	GenerationMissing,
}

/// Result of exact termination. Restored children are never reacquired for this operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessGenerationTermination {
	/// Positive owned-child exit and group-quiescence evidence committed.
	PositiveDeathRecorded,
	/// Positive death was already durable.
	AlreadyDead,
	/// The expected revision did not match current owned authority.
	StaleGeneration,
	/// The original live child handle is absent. The service refuses takeover or PID signaling.
	NotOwned,
	/// The bounded signal/wait did not prove death; the account remains quarantined.
	DeathUnproved,
}

/// Daemon-local readiness for the diagnostic and reconciliation service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessGenerationReadiness {
	/// Startup projection and the first positive reconciliation pass completed.
	Ready,
	/// Durable product authority was unavailable or inconsistent.
	ProductStateUnavailable,
	/// The supported-OS exact identity adapter was unavailable.
	PlatformUnavailable,
}

/// Closed supervisor failure without provider, credential, or database detail.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessSupervisorError {
	/// Durable ProcessGeneration authority was unavailable or inconsistent.
	ProductState,
	/// The supported-OS adapter could not establish exact identity or evidence.
	Platform,
	/// The request contradicted durable or live authority.
	AuthorityConflict,
	/// A required generation or derived evidence identity could not be represented.
	Identity,
	/// Process creation failed after a durable fence; positive non-creation was recorded.
	SpawnFailed,
	/// Process creation occurred, but exact identity binding did not complete.
	IdentityBindingFailed,
	/// Required private lifetime channels were not created.
	ControlChannelUnavailable,
	/// A requested bounded termination duration was invalid.
	InvalidTerminationWait,
}
impl std::error::Error for ProcessSupervisorError {}
impl Display for ProcessSupervisorError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "{self:?}")
	}
}

/// Fenced launch receipt for a newly spawned child.
///
/// This receipt grants no process I/O or protocol authority. `ProcessSupervisor` privately
/// retains the child's channels and exposes only its closed typed operation gateway.
#[derive(Clone)]
pub(crate) struct FencedProcess {
	generation_id: ProcessGenerationId,
	identity: ProcessIdentity,
	revision: i64,
}
impl FencedProcess {
	#[cfg(test)]
	pub(crate) fn for_test(generation_id: ProcessGenerationId, revision: i64) -> Self {
		let boot_id = ProcessBootIdentity::new("runtime-resume-test").expect("test boot identity");
		let identity = ProcessIdentity::new(
			boot_id,
			42_001,
			decodex_core::ProcessStartIdentity::new("runtime-resume-start")
				.expect("test process start identity"),
			42_001,
			42_001,
		)
		.expect("test process identity");
		Self { generation_id, identity, revision }
	}

	pub(crate) fn generation_id(&self) -> &ProcessGenerationId {
		&self.generation_id
	}

	#[allow(dead_code, reason = "reserved for the separately enabled live gateway")]
	pub(crate) fn identity(&self) -> &ProcessIdentity {
		&self.identity
	}

	pub(crate) fn revision(&self) -> i64 {
		self.revision
	}
}

impl ProcessGenerationControl {
	/// Restore fail-closed state and perform one positive reconciliation pass.
	///
	/// The server lifecycle separately owns continued background reconciliation. One uncertain
	/// account does not change product-state availability.
	pub(crate) async fn start(store: SqliteStore) -> Result<Self, ProcessSupervisorError> {
		let boot_id = process_platform::current_boot_identity()
			.map_err(|_| ProcessSupervisorError::Platform)?;
		store
			.project_process_generations_after_supervisor_loss()
			.await
			.map_err(|_| ProcessSupervisorError::ProductState)?;
		let control = Self {
			inner: Arc::new(ProcessSupervisor {
				store,
				boot_id,
				owned: OwnedGenerationRegistry::new(),
				observers: Mutex::new(BTreeMap::new()),
				pending_non_creation: Mutex::new(BTreeSet::new()),
				supervised: Mutex::new(BTreeSet::new()),
			}),
		};
		control.reconcile_all().await?;

		Ok(control)
	}

	/// Build the periodic reconciler for direct ownership by the server lifecycle.
	pub(crate) fn reconciliation_task(
		&self,
		mut stop: tokio::sync::watch::Receiver<bool>,
	) -> impl Future<Output = ()> + Send + 'static {
		let weak = Arc::downgrade(&self.inner);

		async move {
			loop {
				tokio::select! {
					biased;

					changed = stop.changed() => {
						let stopping = changed.is_err() || *stop.borrow_and_update();
						if stopping {
							break;
						}
						continue;
					},
					_ = tokio::time::sleep(RECONCILIATION_INTERVAL) => {},
				}

				if *stop.borrow_and_update() {
					break;
				}
				let Some(inner) = weak.upgrade() else {
					break;
				};
				let control = Self { inner };
				let _ = control.reconcile_all().await;
			}
		}
	}

	/// Read exact bounded diagnostics without treating absence, reuse, or timeout as proof.
	pub async fn diagnostics(
		&self,
		account_id: Option<&AccountId>,
		include_dead: bool,
		limit: u16,
	) -> Result<Vec<ProcessGenerationDiagnostic>, ProcessSupervisorError> {
		let generations = self
			.inner
			.store
			.read_bound_process_generations(account_id, include_dead, limit)
			.await
			.map_err(|_| ProcessSupervisorError::ProductState)?;
		Ok(generations
			.into_iter()
			.map(|bound| {
				let generation = bound.generation;
				let observation = self.observe_for_diagnostic(&generation);
				ProcessGenerationDiagnostic {
					generation,
					account_binding: bound.account_binding,
					observation,
				}
			})
			.collect())
	}

	/// Read one exact generation diagnostic.
	pub async fn diagnostic_exact(
		&self,
		generation_id: &ProcessGenerationId,
	) -> Result<Option<ProcessGenerationDiagnostic>, ProcessSupervisorError> {
		Ok(self.find_bound_generation(generation_id).await?.map(|bound| {
			let generation = bound.generation;
			let observation = self.observe_for_diagnostic(&generation);
			ProcessGenerationDiagnostic {
				generation,
				account_binding: bound.account_binding,
				observation,
			}
		}))
	}

	/// Reconcile one exact generation using positive evidence only.
	pub async fn reconcile_exact(
		&self,
		generation_id: &ProcessGenerationId,
	) -> Result<ProcessGenerationReconciliation, ProcessSupervisorError> {
		if let Some(outcome) = self.reconcile_pending_non_creation_exact(generation_id).await? {
			return Ok(outcome);
		}
		let Some(generation) = self.find_generation(generation_id).await? else {
			return Ok(ProcessGenerationReconciliation::GenerationMissing);
		};
		self.reconcile_projection(generation).await
	}

	/// Terminate only a process whose original unreaped `Child` remains owned by this supervisor.
	///
	/// Restored same-boot processes return `NotOwned`; this method never adopts or signals them.
	#[allow(clippy::too_many_lines)] // Keep one exact fenced termination sequence together.
	pub async fn terminate_exact(
		&self,
		generation_id: &ProcessGenerationId,
		expected_revision: i64,
		wait: Duration,
	) -> Result<ProcessGenerationTermination, ProcessSupervisorError> {
		if wait.is_zero() || wait > MAX_TERMINATION_WAIT {
			return Err(ProcessSupervisorError::InvalidTerminationWait);
		}
		let Some(generation) = self.find_generation(generation_id).await? else {
			return Ok(ProcessGenerationTermination::NotOwned);
		};
		if generation.state == ProcessGenerationState::Dead {
			return Ok(ProcessGenerationTermination::AlreadyDead);
		}
		if generation.revision != expected_revision {
			return Ok(ProcessGenerationTermination::StaleGeneration);
		}

		let key = generation_id.as_str().to_owned();
		let Some(mut owned_process) = self.inner.owned.take(&key)? else {
			return Ok(ProcessGenerationTermination::NotOwned);
		};
		let mut supervision = match self.supervision_activity(generation_id) {
			Ok(supervision) => supervision,
			Err(error) => {
				self.replace_owned(key, owned_process)?;
				return Err(error);
			},
		};
		if owned_process.identity.as_ref() != generation.process_identity.as_ref() {
			self.restore_owned(key, owned_process, &mut supervision)?;
			return Ok(ProcessGenerationTermination::StaleGeneration);
		}
		let Some(identity) = owned_process.identity.clone() else {
			self.restore_owned(key, owned_process, &mut supervision)?;
			return Ok(ProcessGenerationTermination::NotOwned);
		};
		owned_process.revision = generation.revision;
		if let Err(error) = refresh_owned_exit(&mut owned_process) {
			self.restore_owned(key, owned_process, &mut supervision)?;
			return Err(error);
		}

		let stopping = self
			.inner
			.store
			.mark_process_generation_stopping(generation_id, expected_revision)
			.await
			.map_err(|_| ProcessSupervisorError::ProductState);
		let stopping = match stopping.and_then(accepted_mutation) {
			Ok(stopping) => stopping,
			Err(error) => {
				self.restore_owned(key, owned_process, &mut supervision)?;
				return Err(error);
			},
		};
		owned_process.revision = stopping.revision;

		if !owned_process.leader_exited {
			if !owned_process.child.may_signal_process_group() {
				self.restore_owned(key, owned_process, &mut supervision)?;
				return Err(ProcessSupervisorError::Platform);
			}
			if process_platform::signal_owned_process_group(&identity, libc::SIGTERM).is_err() {
				if let Err(error) = refresh_owned_exit(&mut owned_process) {
					self.restore_owned(key, owned_process, &mut supervision)?;
					return Err(error);
				}
				if !owned_process.leader_exited {
					self.restore_owned(key, owned_process, &mut supervision)?;
					return Err(ProcessSupervisorError::Platform);
				}
			}
		}

		let deadline = Instant::now() + wait;
		let hard_signal_at = Instant::now() + wait / 2;
		let mut hard_signal_sent = false;
		while Instant::now() < deadline {
			if refresh_owned_exit(&mut owned_process).is_err() {
				self.restore_owned(key, owned_process, &mut supervision)?;
				return Err(ProcessSupervisorError::Platform);
			}
			let group_quiescent = if owned_process.leader_exited {
				match process_platform::process_group_id_is_quiescent(
					owned_process.process_group_id,
				) {
					Ok(value) => value,
					Err(_) => {
						self.restore_owned(key, owned_process, &mut supervision)?;
						return Err(ProcessSupervisorError::Platform);
					},
				}
			} else {
				false
			};
			if group_quiescent {
				let generation_for_death = ProcessGeneration {
					state: stopping.state,
					authority_loss_reason: None,
					revision: stopping.revision,
					updated_at_micros: stopping.recorded_at_micros,
					..generation.clone()
				};
				if let Err(error) = self
					.record_positive_death(
						&generation_for_death,
						ProcessDeathEvidenceKind::ExactTerminationExit,
						Some(identity.clone()),
					)
					.await
				{
					self.restore_owned(key, owned_process, &mut supervision)?;
					return Err(error);
				}
				return Ok(ProcessGenerationTermination::PositiveDeathRecorded);
			}
			if !hard_signal_sent && !owned_process.leader_exited && Instant::now() >= hard_signal_at
			{
				if process_platform::signal_owned_process_group(&identity, libc::SIGKILL).is_err() {
					if let Err(error) = refresh_owned_exit(&mut owned_process) {
						self.restore_owned(key, owned_process, &mut supervision)?;
						return Err(error);
					}
					if !owned_process.leader_exited {
						self.restore_owned(key, owned_process, &mut supervision)?;
						return Err(ProcessSupervisorError::Platform);
					}
				}
				hard_signal_sent = true;
			}
			tokio::time::sleep(Duration::from_millis(10)).await;
		}

		let unknown = self
			.inner
			.store
			.mark_process_generation_death_unknown(
				generation_id,
				owned_process.revision,
				ProcessAuthorityLossReason::TerminationUnproved,
			)
			.await
			.map_err(|_| ProcessSupervisorError::ProductState)
			.and_then(accepted_mutation);
		let unknown = match unknown {
			Ok(unknown) => unknown,
			Err(error) => {
				self.restore_owned(key, owned_process, &mut supervision)?;
				return Err(error);
			},
		};
		owned_process.revision = unknown.revision;
		self.restore_owned(key, owned_process, &mut supervision)?;
		Ok(ProcessGenerationTermination::DeathUnproved)
	}

	/// Spawn only after the durable ProcessGeneration insert re-locks Conversation Turn authority.
	pub(crate) async fn spawn_fenced_conversation(
		&self,
		admission: FreshConversationProcessGeneration,
		execution_authorization: ProcessExecutionAuthorization,
		launch: AttestedAppServerLaunch,
	) -> Result<FencedProcess, ProcessSupervisorError> {
		let generation_id = admission.generation_id().clone();
		self.spawn_fenced_inner(generation_id, execution_authorization, launch, admission).await
	}

	async fn spawn_fenced_inner(
		&self,
		generation_id: ProcessGenerationId,
		execution_authorization: ProcessExecutionAuthorization,
		launch: AttestedAppServerLaunch,
		admission: FreshConversationProcessGeneration,
	) -> Result<FencedProcess, ProcessSupervisorError> {
		let intent = launch.derive_intent(
			generation_id,
			self.inner.boot_id.clone(),
			execution_authorization,
		);
		let account_binding = launch.account_binding().clone();
		let mut supervision = self.reserve_supervision(&intent.generation_id)?;
		let preparation = self
			.inner
			.store
			.prepare_conversation_bound_process_generation(&intent, &account_binding, admission)
			.await
			.map_err(|_| ProcessSupervisorError::ProductState)?;
		let fence = match preparation {
			PrepareProcessGenerationOutcome::Fresh(fence) => fence,
			PrepareProcessGenerationOutcome::Replayed(_)
			| PrepareProcessGenerationOutcome::Rejected { .. } => {
				return Err(ProcessSupervisorError::AuthorityConflict);
			},
		};

		let child = match launch.spawn() {
			Ok(child) => child,
			Err(_) => {
				let generation =
					generation_from_intent(&intent, fence.revision(), fence.fenced_at_micros());
				if let Err(error) = self
					.record_positive_death(
						&generation,
						ProcessDeathEvidenceKind::SpawnNotCreated,
						None,
					)
					.await
				{
					self.remember_pending_non_creation(&generation.generation_id)?;
					supervision.retain();
					return Err(error);
				}
				return Err(ProcessSupervisorError::SpawnFailed);
			},
		};

		let process_id = child.process_id();
		let identity =
			match process_platform::inspect_process_identity(process_id, &self.inner.boot_id) {
				Ok(Some(identity)) => identity,
				Ok(None) | Err(_) => {
					supervision.retain();
					self.quarantine_failed_identity(&intent, fence.revision(), child, process_id)
						.await?;
					return Err(ProcessSupervisorError::IdentityBindingFailed);
				},
			};
		let bound = self
			.inner
			.store
			.bind_process_generation_identity(fence.generation_id(), fence.revision(), &identity)
			.await
			.map_err(|_| ProcessSupervisorError::ProductState)
			.and_then(accepted_mutation);
		let bound = match bound {
			Ok(bound) => bound,
			Err(error) => {
				supervision.retain();
				self.quarantine_failed_bound_identity(&intent, fence.revision(), child, identity)
					.await?;
				return Err(error);
			},
		};

		if !child.has_private_lifetime_channels() {
			supervision.retain();
			self.quarantine_failed_bound_identity(&intent, bound.revision, child, identity).await?;
			return Err(ProcessSupervisorError::ControlChannelUnavailable);
		}
		let key = intent.generation_id.as_str().to_owned();
		self.replace_owned(
			key,
			OwnedGeneration {
				child,
				process_group_id: identity.process_group_id,
				identity: Some(identity.clone()),
				revision: bound.revision,
				leader_exited: false,
			},
		)?;
		supervision.retain();
		Ok(FencedProcess {
			generation_id: intent.generation_id.clone(),
			identity,
			revision: bound.revision,
		})
	}

	/// Persist application readiness for one still-owned fenced child.
	pub(crate) async fn mark_spawned_ready(
		&self,
		process: &mut FencedProcess,
	) -> Result<(), ProcessSupervisorError> {
		let mutation = self
			.inner
			.store
			.mark_process_generation_ready(&process.generation_id, process.revision)
			.await
			.map_err(|_| ProcessSupervisorError::ProductState)
			.and_then(accepted_mutation)?;
		process.revision = mutation.revision;
		self.inner
			.owned
			.with_current(process.generation_id.as_str(), |current| {
				if current.identity.as_ref() != Some(&process.identity) {
					return Err(ProcessSupervisorError::AuthorityConflict);
				}
				current.revision = mutation.revision;
				Ok(())
			})?
			.ok_or(ProcessSupervisorError::AuthorityConflict)?
	}

	/// Execute one typed adapter operation against the exact still-owned child.
	///
	/// The closure receives no process handles and runs while generation ownership is locked. The
	/// caller must use the adapter's closed account or Reset Card methods only.
	pub(crate) fn with_fenced_child<T, E>(
		&self,
		process: &FencedProcess,
		operation: impl FnOnce(&mut AttestedProcessChild) -> Result<T, E>,
	) -> Result<Result<T, E>, ProcessSupervisorError> {
		self.inner
			.owned
			.with_current(process.generation_id.as_str(), |current| {
				if current.revision != process.revision
					|| current.identity.as_ref() != Some(&process.identity)
					|| current.leader_exited
				{
					return Err(ProcessSupervisorError::AuthorityConflict);
				}
				Ok(operation(&mut current.child))
			})?
			.ok_or(ProcessSupervisorError::AuthorityConflict)?
	}

	async fn reconcile_all(&self) -> Result<(), ProcessSupervisorError> {
		let pending = self
			.inner
			.pending_non_creation
			.lock()
			.map_err(|_| ProcessSupervisorError::AuthorityConflict)?
			.iter()
			.cloned()
			.collect::<Vec<_>>();
		for generation_id in pending {
			let _ = self.reconcile_pending_non_creation_exact(&generation_id).await;
		}

		let local_generation_ids = self.local_generation_ids()?;
		for generation_id in local_generation_ids {
			let Ok(Some(generation)) = self.find_generation(&generation_id).await else {
				continue;
			};
			let _ = self.reconcile_projection(generation).await;
		}

		let mut after = None;
		loop {
			let page = self
				.inner
				.store
				.read_process_generation_page(None, false, after.as_ref(), RECONCILIATION_PAGE_SIZE)
				.await
				.map_err(|_| ProcessSupervisorError::ProductState)?;
			if page.is_empty() {
				break;
			}
			let page_len = page.len();
			for generation in page {
				after = Some(generation.generation_id.clone());
				let _ = self.reconcile_projection(generation).await;
			}
			if page_len < usize::from(RECONCILIATION_PAGE_SIZE) {
				break;
			}
		}
		Ok(())
	}

	#[allow(clippy::too_many_lines)] // Keep one exact projection reconciliation sequence together.
	async fn reconcile_projection(
		&self,
		mut generation: ProcessGeneration,
	) -> Result<ProcessGenerationReconciliation, ProcessSupervisorError> {
		if generation.state == ProcessGenerationState::Dead {
			self.clear_local_generation(&generation.generation_id)?;
			return Ok(ProcessGenerationReconciliation::AlreadyDead);
		}
		if generation.state != ProcessGenerationState::DeathUnknown
			&& !self.supervises(&generation.generation_id)?
		{
			let mutation = self
				.inner
				.store
				.mark_process_generation_death_unknown(
					&generation.generation_id,
					generation.revision,
					ProcessAuthorityLossReason::SupervisorRestarted,
				)
				.await
				.map_err(|_| ProcessSupervisorError::ProductState)
				.and_then(accepted_mutation)?;
			generation.state = mutation.state;
			generation.revision = mutation.revision;
			generation.authority_loss_reason =
				Some(ProcessAuthorityLossReason::SupervisorRestarted);
		}

		if process_platform::boot_identity_mismatch_proves_prior_boot(
			&generation.intended_boot_id,
			&self.inner.boot_id,
		) {
			self.record_positive_death(&generation, ProcessDeathEvidenceKind::PriorBootEnded, None)
				.await?;
			return Ok(ProcessGenerationReconciliation::PositiveDeathRecorded);
		}

		if let Some(identity) = self.owned_positive_exit(&generation)? {
			self.record_positive_death(
				&generation,
				ProcessDeathEvidenceKind::OwnedChildExit,
				identity,
			)
			.await?;
			self.remove_owned(&generation.generation_id)?;
			return Ok(ProcessGenerationReconciliation::PositiveDeathRecorded);
		}
		if self.owns(&generation.generation_id)? {
			return Ok(ProcessGenerationReconciliation::Quarantined {
				state: generation.state,
				observation: ProcessGenerationObservation::Owned,
			});
		}
		if self.supervises(&generation.generation_id)? {
			return Ok(ProcessGenerationReconciliation::Quarantined {
				state: generation.state,
				observation: ProcessGenerationObservation::SupervisionInFlight,
			});
		}

		let Some(identity) = generation.process_identity.as_ref() else {
			return Ok(ProcessGenerationReconciliation::Quarantined {
				state: generation.state,
				observation: ProcessGenerationObservation::SameBootUnbound,
			});
		};
		let key = generation.generation_id.as_str().to_owned();
		if !self
			.inner
			.observers
			.lock()
			.map_err(|_| ProcessSupervisorError::AuthorityConflict)?
			.contains_key(&key)
		{
			match process_platform::attach_exit_witness(identity)
				.map_err(|_| ProcessSupervisorError::Platform)?
			{
				ExactProcessObservation::Attached(witness) => {
					self.inner
						.observers
						.lock()
						.map_err(|_| ProcessSupervisorError::AuthorityConflict)?
						.insert(key.clone(), witness);
				},
				ExactProcessObservation::NotObserved => {
					return Ok(ProcessGenerationReconciliation::Quarantined {
						state: generation.state,
						observation: ProcessGenerationObservation::SameBootNotObserved,
					});
				},
				ExactProcessObservation::IdentityMismatch { observed } => {
					return Ok(ProcessGenerationReconciliation::Quarantined {
						state: generation.state,
						observation: ProcessGenerationObservation::SameBootIdentityMismatch {
							observed,
						},
					});
				},
			}
		}

		let positive = {
			let observers = self
				.inner
				.observers
				.lock()
				.map_err(|_| ProcessSupervisorError::AuthorityConflict)?;
			let witness = observers.get(&key).ok_or(ProcessSupervisorError::AuthorityConflict)?;
			if witness.identity() != identity {
				return Err(ProcessSupervisorError::AuthorityConflict);
			}
			witness.try_positive_exit().map_err(|_| ProcessSupervisorError::Platform)?
		};
		if let Some(kind) = positive {
			let process_group_quiescent = process_platform::process_group_is_quiescent(identity)
				.map_err(|_| ProcessSupervisorError::Platform)?;
			if process_group_quiescent {
				self.record_positive_death(&generation, kind, Some(identity.clone())).await?;
				self.remove_observer(&generation.generation_id)?;
				return Ok(ProcessGenerationReconciliation::PositiveDeathRecorded);
			}
			return Ok(ProcessGenerationReconciliation::Quarantined {
				state: generation.state,
				observation: ProcessGenerationObservation::PositiveExitObserved {
					witness_kind: exit_witness_kind(kind)?,
					process_group_quiescent,
				},
			});
		}
		Ok(ProcessGenerationReconciliation::Quarantined {
			state: generation.state,
			observation: ProcessGenerationObservation::PositiveExitObserverAttached,
		})
	}

	fn owned_positive_exit(
		&self,
		generation: &ProcessGeneration,
	) -> Result<Option<Option<ProcessIdentity>>, ProcessSupervisorError> {
		let key = generation.generation_id.as_str();
		self.inner
			.owned
			.with_current(key, |process| {
				if let Some(durable_identity) = generation.process_identity.as_ref()
					&& process.identity.as_ref() != Some(durable_identity)
				{
					return Err(ProcessSupervisorError::AuthorityConflict);
				}
				process.revision = generation.revision;
				refresh_owned_exit(process)?;
				if !process.leader_exited
					|| !process_platform::process_group_id_is_quiescent(process.process_group_id)
						.map_err(|_| ProcessSupervisorError::Platform)?
				{
					return Ok(None);
				}
				// A lost bind response can leave the original `Child` owned while the durable
				// generation remains unbound. The owned wait still proves that generation's child
				// exited, but the evidence must not claim identity facts that did not commit.
				Ok(Some(generation.process_identity.clone()))
			})?
			.transpose()
			.map(Option::flatten)
	}

	async fn record_positive_death(
		&self,
		generation: &ProcessGeneration,
		kind: ProcessDeathEvidenceKind,
		identity: Option<ProcessIdentity>,
	) -> Result<(), ProcessSupervisorError> {
		let evidence = ProcessDeathEvidence::new(
			derived_evidence_id(&generation.generation_id, kind)?,
			generation.generation_id.clone(),
			kind,
			self.inner.boot_id.clone(),
			identity,
			witness_digest(generation, kind, &self.inner.boot_id),
		)
		.map_err(|_| ProcessSupervisorError::Identity)?;
		self.inner
			.store
			.record_process_generation_death(generation.revision, &evidence)
			.await
			.map_err(|_| ProcessSupervisorError::ProductState)
			.and_then(accepted_mutation)?;
		Ok(())
	}

	async fn find_generation(
		&self,
		generation_id: &ProcessGenerationId,
	) -> Result<Option<ProcessGeneration>, ProcessSupervisorError> {
		let mut after = None;
		loop {
			let page = self
				.inner
				.store
				.read_process_generation_page(None, true, after.as_ref(), RECONCILIATION_PAGE_SIZE)
				.await
				.map_err(|_| ProcessSupervisorError::ProductState)?;
			if page.is_empty() {
				return Ok(None);
			}
			for generation in page {
				if generation.generation_id == *generation_id {
					return Ok(Some(generation));
				}
				if generation.generation_id > *generation_id {
					return Ok(None);
				}
				after = Some(generation.generation_id);
			}
		}
	}

	async fn find_bound_generation(
		&self,
		generation_id: &ProcessGenerationId,
	) -> Result<Option<decodex_core::BoundProcessGeneration>, ProcessSupervisorError> {
		let mut after = None;
		loop {
			let page = self
				.inner
				.store
				.read_bound_process_generation_page(
					None,
					true,
					after.as_ref(),
					RECONCILIATION_PAGE_SIZE,
				)
				.await
				.map_err(|_| ProcessSupervisorError::ProductState)?;
			if page.is_empty() {
				return Ok(None);
			}
			for bound in page {
				if bound.generation.generation_id == *generation_id {
					return Ok(Some(bound));
				}
				if bound.generation.generation_id > *generation_id {
					return Ok(None);
				}
				after = Some(bound.generation.generation_id);
			}
		}
	}

	fn observe_for_diagnostic(
		&self,
		generation: &ProcessGeneration,
	) -> ProcessGenerationObservation {
		if generation.state == ProcessGenerationState::Dead {
			return ProcessGenerationObservation::Dead;
		}
		if process_platform::boot_identity_mismatch_proves_prior_boot(
			&generation.intended_boot_id,
			&self.inner.boot_id,
		) {
			return ProcessGenerationObservation::PriorBootEnded;
		}
		if self.owns(&generation.generation_id).unwrap_or(false) {
			return ProcessGenerationObservation::Owned;
		}
		if self.has_pending_non_creation(&generation.generation_id).unwrap_or(false) {
			return ProcessGenerationObservation::SpawnNonCreationPending;
		}
		if self.supervises(&generation.generation_id).unwrap_or(false) {
			return ProcessGenerationObservation::SupervisionInFlight;
		}
		let observers = match self.inner.observers.lock() {
			Ok(observers) => observers,
			Err(_) => return ProcessGenerationObservation::ObservationUnavailable,
		};
		if let Some(witness) = observers.get(generation.generation_id.as_str()) {
			let Some(kind) = witness.positive_exit_kind() else {
				return ProcessGenerationObservation::PositiveExitObserverAttached;
			};
			let Some(witness_kind) = exit_witness_kind(kind).ok() else {
				return ProcessGenerationObservation::ObservationUnavailable;
			};
			return match generation
				.process_identity
				.as_ref()
				.map(process_platform::process_group_is_quiescent)
			{
				Some(Ok(process_group_quiescent)) =>
					ProcessGenerationObservation::PositiveExitObserved {
						witness_kind,
						process_group_quiescent,
					},
				Some(Err(_)) | None => ProcessGenerationObservation::ObservationUnavailable,
			};
		}
		drop(observers);
		let Some(identity) = generation.process_identity.as_ref() else {
			return ProcessGenerationObservation::SameBootUnbound;
		};
		match process_platform::inspect_process_identity(identity.process_id, &self.inner.boot_id) {
			Ok(Some(observed)) if observed == *identity =>
				ProcessGenerationObservation::SameBootExactIdentityPresent,
			Ok(Some(observed)) =>
				ProcessGenerationObservation::SameBootIdentityMismatch { observed },
			Ok(None) => ProcessGenerationObservation::SameBootNotObserved,
			Err(_) => ProcessGenerationObservation::ObservationUnavailable,
		}
	}

	fn owns(&self, generation_id: &ProcessGenerationId) -> Result<bool, ProcessSupervisorError> {
		self.inner.owned.contains(generation_id.as_str())
	}

	fn supervises(
		&self,
		generation_id: &ProcessGenerationId,
	) -> Result<bool, ProcessSupervisorError> {
		Ok(self
			.inner
			.supervised
			.lock()
			.map_err(|_| ProcessSupervisorError::AuthorityConflict)?
			.contains(generation_id.as_str()))
	}

	fn local_generation_ids(&self) -> Result<Vec<ProcessGenerationId>, ProcessSupervisorError> {
		let mut keys = self
			.inner
			.supervised
			.lock()
			.map_err(|_| ProcessSupervisorError::AuthorityConflict)?
			.iter()
			.cloned()
			.collect::<BTreeSet<_>>();
		keys.extend(
			self.inner
				.observers
				.lock()
				.map_err(|_| ProcessSupervisorError::AuthorityConflict)?
				.keys()
				.cloned(),
		);
		keys.into_iter()
			.map(|key| ProcessGenerationId::new(key).map_err(|_| ProcessSupervisorError::Identity))
			.collect()
	}

	fn reserve_supervision(
		&self,
		generation_id: &ProcessGenerationId,
	) -> Result<SupervisionReservation, ProcessSupervisorError> {
		let key = generation_id.as_str().to_owned();
		if !self
			.inner
			.supervised
			.lock()
			.map_err(|_| ProcessSupervisorError::AuthorityConflict)?
			.insert(key.clone())
		{
			return Err(ProcessSupervisorError::AuthorityConflict);
		}
		Ok(SupervisionReservation { inner: Arc::clone(&self.inner), key, retained: false })
	}

	async fn reconcile_pending_non_creation_exact(
		&self,
		generation_id: &ProcessGenerationId,
	) -> Result<Option<ProcessGenerationReconciliation>, ProcessSupervisorError> {
		if !self.has_pending_non_creation(generation_id)? {
			return Ok(None);
		}
		let Some(generation) = self.find_generation(generation_id).await? else {
			return Ok(Some(ProcessGenerationReconciliation::GenerationMissing));
		};
		if generation.state == ProcessGenerationState::Dead {
			self.forget_pending_non_creation(generation_id)?;
			self.remove_supervision(generation_id)?;
			return Ok(Some(ProcessGenerationReconciliation::AlreadyDead));
		}
		self.record_positive_death(&generation, ProcessDeathEvidenceKind::SpawnNotCreated, None)
			.await?;
		self.forget_pending_non_creation(generation_id)?;
		self.remove_supervision(generation_id)?;
		Ok(Some(ProcessGenerationReconciliation::PositiveDeathRecorded))
	}

	fn remember_pending_non_creation(
		&self,
		generation_id: &ProcessGenerationId,
	) -> Result<(), ProcessSupervisorError> {
		self.inner
			.pending_non_creation
			.lock()
			.map_err(|_| ProcessSupervisorError::AuthorityConflict)?
			.insert(generation_id.clone());
		Ok(())
	}

	fn has_pending_non_creation(
		&self,
		generation_id: &ProcessGenerationId,
	) -> Result<bool, ProcessSupervisorError> {
		Ok(self
			.inner
			.pending_non_creation
			.lock()
			.map_err(|_| ProcessSupervisorError::AuthorityConflict)?
			.contains(generation_id))
	}

	fn forget_pending_non_creation(
		&self,
		generation_id: &ProcessGenerationId,
	) -> Result<(), ProcessSupervisorError> {
		self.inner
			.pending_non_creation
			.lock()
			.map_err(|_| ProcessSupervisorError::AuthorityConflict)?
			.remove(generation_id);
		Ok(())
	}

	fn supervision_activity(
		&self,
		generation_id: &ProcessGenerationId,
	) -> Result<SupervisionReservation, ProcessSupervisorError> {
		if !self.supervises(generation_id)? {
			return Err(ProcessSupervisorError::AuthorityConflict);
		}
		Ok(SupervisionReservation {
			inner: Arc::clone(&self.inner),
			key: generation_id.as_str().to_owned(),
			retained: false,
		})
	}

	fn restore_owned(
		&self,
		key: String,
		process: OwnedGeneration,
		supervision: &mut SupervisionReservation,
	) -> Result<(), ProcessSupervisorError> {
		self.replace_owned(key, process)?;
		supervision.retain();
		Ok(())
	}

	fn replace_owned(
		&self,
		key: String,
		process: OwnedGeneration,
	) -> Result<(), ProcessSupervisorError> {
		if !self
			.inner
			.supervised
			.lock()
			.map_err(|_| ProcessSupervisorError::AuthorityConflict)?
			.contains(&key)
		{
			return Err(ProcessSupervisorError::AuthorityConflict);
		}
		let inserted = self.inner.owned.insert(key, process)?;
		if !inserted {
			return Err(ProcessSupervisorError::AuthorityConflict);
		}
		Ok(())
	}

	fn remove_observer(
		&self,
		generation_id: &ProcessGenerationId,
	) -> Result<(), ProcessSupervisorError> {
		self.inner
			.observers
			.lock()
			.map_err(|_| ProcessSupervisorError::AuthorityConflict)?
			.remove(generation_id.as_str());
		Ok(())
	}

	fn remove_owned(
		&self,
		generation_id: &ProcessGenerationId,
	) -> Result<(), ProcessSupervisorError> {
		let _ = self.inner.owned.take(generation_id.as_str())?;
		self.remove_supervision(generation_id)?;
		Ok(())
	}

	fn remove_supervision(
		&self,
		generation_id: &ProcessGenerationId,
	) -> Result<(), ProcessSupervisorError> {
		self.inner
			.supervised
			.lock()
			.map_err(|_| ProcessSupervisorError::AuthorityConflict)?
			.remove(generation_id.as_str());
		Ok(())
	}

	fn clear_local_generation(
		&self,
		generation_id: &ProcessGenerationId,
	) -> Result<(), ProcessSupervisorError> {
		self.remove_observer(generation_id)?;
		self.remove_owned(generation_id)?;
		self.forget_pending_non_creation(generation_id)
	}

	async fn quarantine_failed_identity(
		&self,
		intent: &ProcessGenerationIntent,
		revision: i64,
		mut child: AttestedProcessChild,
		process_group_id: u32,
	) -> Result<(), ProcessSupervisorError> {
		child.close_private_lifetime_channels();
		let _ = process_platform::signal_owned_process_group_id(process_group_id, libc::SIGKILL);
		let mutation = self
			.inner
			.store
			.mark_process_generation_death_unknown(
				&intent.generation_id,
				revision,
				ProcessAuthorityLossReason::IdentityPersistenceFailed,
			)
			.await
			.map_err(|_| ProcessSupervisorError::ProductState)
			.and_then(accepted_mutation);
		let mut owned = OwnedGeneration {
			child,
			process_group_id,
			identity: None,
			revision,
			leader_exited: false,
		};
		let _ = wait_owned_briefly(&mut owned).await;
		match mutation {
			Ok(mutation) => {
				owned.revision = mutation.revision;
				self.replace_owned(intent.generation_id.as_str().to_owned(), owned)
			},
			Err(error) => {
				self.replace_owned(intent.generation_id.as_str().to_owned(), owned)?;
				Err(error)
			},
		}
	}

	async fn quarantine_failed_bound_identity(
		&self,
		intent: &ProcessGenerationIntent,
		revision: i64,
		mut child: AttestedProcessChild,
		identity: ProcessIdentity,
	) -> Result<(), ProcessSupervisorError> {
		child.close_private_lifetime_channels();
		let _ = process_platform::signal_owned_process_group(&identity, libc::SIGKILL);
		let current = match self.find_generation(&intent.generation_id).await {
			Ok(current) => current,
			Err(error) => {
				let owned = OwnedGeneration {
					child,
					process_group_id: identity.process_group_id,
					identity: Some(identity),
					revision,
					leader_exited: false,
				};
				self.replace_owned(intent.generation_id.as_str().to_owned(), owned)?;
				return Err(error);
			},
		};
		let revision = current.as_ref().map_or(revision, |generation| generation.revision);
		let mutation = self
			.inner
			.store
			.mark_process_generation_death_unknown(
				&intent.generation_id,
				revision,
				ProcessAuthorityLossReason::IdentityPersistenceFailed,
			)
			.await
			.map_err(|_| ProcessSupervisorError::ProductState)
			.and_then(accepted_mutation);
		let mut owned = OwnedGeneration {
			child,
			process_group_id: identity.process_group_id,
			identity: Some(identity),
			revision,
			leader_exited: false,
		};
		let _ = wait_owned_briefly(&mut owned).await;
		match mutation {
			Ok(mutation) => {
				owned.revision = mutation.revision;
				self.replace_owned(intent.generation_id.as_str().to_owned(), owned)
			},
			Err(error) => {
				self.replace_owned(intent.generation_id.as_str().to_owned(), owned)?;
				Err(error)
			},
		}
	}
}

fn accepted_mutation(
	outcome: ProcessGenerationMutationOutcome,
) -> Result<ProcessGenerationMutation, ProcessSupervisorError> {
	match outcome {
		ProcessGenerationMutationOutcome::Applied(mutation)
		| ProcessGenerationMutationOutcome::Replayed(mutation) => Ok(mutation),
		ProcessGenerationMutationOutcome::Rejected { .. } =>
			Err(ProcessSupervisorError::AuthorityConflict),
	}
}

fn exit_witness_kind(
	kind: ProcessDeathEvidenceKind,
) -> Result<ProcessGenerationExitWitnessKind, ProcessSupervisorError> {
	match kind {
		ProcessDeathEvidenceKind::LinuxPidfdExit =>
			Ok(ProcessGenerationExitWitnessKind::LinuxPidfd),
		ProcessDeathEvidenceKind::MacosKqueueExitAndGroupQuiescence =>
			Ok(ProcessGenerationExitWitnessKind::MacosKqueueNoteExit),
		_ => Err(ProcessSupervisorError::AuthorityConflict),
	}
}

fn refresh_owned_exit(process: &mut OwnedGeneration) -> Result<(), ProcessSupervisorError> {
	if !process.leader_exited {
		process.leader_exited =
			process.child.try_wait().map_err(|_| ProcessSupervisorError::Platform)?.is_some();
	}
	Ok(())
}

async fn wait_owned_briefly(process: &mut OwnedGeneration) -> Result<(), ProcessSupervisorError> {
	let deadline = Instant::now() + FAILED_SPAWN_CLEANUP;
	while Instant::now() < deadline {
		refresh_owned_exit(process)?;
		if process.leader_exited {
			break;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	Ok(())
}

fn generation_from_intent(
	intent: &ProcessGenerationIntent,
	revision: i64,
	fenced_at_micros: i64,
) -> ProcessGeneration {
	ProcessGeneration {
		generation_id: intent.generation_id.clone(),
		account_id: intent.account_id.clone(),
		execution_epoch_id: intent.execution_authorization.epoch_id.clone(),
		runner_identity: intent.runner_identity.clone(),
		intended_boot_id: intent.intended_boot_id.clone(),
		control_kind: intent.control_kind,
		isolation_kind: intent.isolation_kind,
		process_identity: None,
		state: ProcessGenerationState::Starting,
		authority_loss_reason: None,
		death_evidence_id: None,
		revision,
		created_at_micros: fenced_at_micros,
		updated_at_micros: fenced_at_micros,
	}
}

fn derived_evidence_id(
	generation_id: &ProcessGenerationId,
	kind: ProcessDeathEvidenceKind,
) -> Result<ProcessDeathEvidenceId, ProcessSupervisorError> {
	let digest = Sha256::digest(format!("xy-1400|{}|{}", generation_id.as_str(), kind.as_sql()));
	let mut bytes = [0_u8; 16];
	bytes.copy_from_slice(&digest[..16]);
	bytes[6] = (bytes[6] & 0x0f) | 0x40;
	bytes[8] = (bytes[8] & 0x3f) | 0x80;
	ProcessDeathEvidenceId::new(format!(
		"{}-{}-{}-{}-{}",
		hex(&bytes[0..4]),
		hex(&bytes[4..6]),
		hex(&bytes[6..8]),
		hex(&bytes[8..10]),
		hex(&bytes[10..16]),
	))
	.map_err(|_| ProcessSupervisorError::Identity)
}

fn witness_digest(
	generation: &ProcessGeneration,
	kind: ProcessDeathEvidenceKind,
	observed_boot_id: &ProcessBootIdentity,
) -> String {
	let identity = generation.process_identity.as_ref().map_or_else(
		|| "unbound".to_owned(),
		|identity| {
			format!(
				"{}|{}|{}|{}|{}",
				identity.boot_id.as_str(),
				identity.process_id,
				identity.process_start_id.as_str(),
				identity.process_group_id,
				identity.session_id
			)
		},
	);
	hex(&Sha256::digest(format!(
		"xy-1400-witness|{}|{}|{}|{}|{}",
		generation.generation_id.as_str(),
		kind.as_sql(),
		generation.revision,
		observed_boot_id.as_str(),
		identity
	)))
}

fn hex(bytes: &[u8]) -> String {
	const HEX: &[u8; 16] = b"0123456789abcdef";
	let mut output = String::with_capacity(bytes.len() * 2);
	for &byte in bytes {
		output.push(char::from(HEX[usize::from(byte >> 4)]));
		output.push(char::from(HEX[usize::from(byte & 0x0f)]));
	}
	output
}

impl From<ProcessPlatformError> for ProcessSupervisorError {
	fn from(_: ProcessPlatformError) -> Self {
		Self::Platform
	}
}

#[cfg(test)]
mod tests {
	use std::{
		collections::BTreeSet,
		sync::{Arc, Condvar, Mutex, TryLockError, mpsc},
		thread,
		time::{Duration, Instant},
	};

	use super::OwnedGenerationRegistry;

	const CONCURRENCY_TIMEOUT: Duration = Duration::from_secs(2);
	const EXCLUSION_WINDOW: Duration = Duration::from_millis(200);

	#[test]
	fn distinct_generation_operations_do_not_head_of_line_block() {
		let registry = Arc::new(OwnedGenerationRegistry::new());
		assert!(registry.insert("first".to_owned(), 0_u8).unwrap());
		assert!(registry.insert("second".to_owned(), 0_u8).unwrap());

		let release = Arc::new((Mutex::new(false), Condvar::new()));
		let (entered_sender, entered_receiver) = mpsc::channel();
		let mut workers = Vec::new();
		for key in ["first", "second"] {
			let registry = Arc::clone(&registry);
			let release = Arc::clone(&release);
			let entered_sender = entered_sender.clone();
			workers.push(thread::spawn(move || {
				let result = registry
					.with_current(key, |value| {
						*value += 1;
						entered_sender.send(key).unwrap();
						let (released, condition) = &*release;
						let mut released = released.lock().unwrap();
						while !*released {
							released = condition.wait(released).unwrap();
						}
					})
					.unwrap();
				assert!(result.is_some());
			}));
		}
		drop(entered_sender);

		let first = entered_receiver.recv_timeout(CONCURRENCY_TIMEOUT);
		let second = entered_receiver.recv_timeout(CONCURRENCY_TIMEOUT);
		{
			let (released, condition) = &*release;
			*released.lock().unwrap() = true;
			condition.notify_all();
		}
		for worker in workers {
			worker.join().unwrap();
		}

		let entered = [first.unwrap(), second.unwrap()].into_iter().collect::<BTreeSet<_>>();
		assert_eq!(entered, BTreeSet::from(["first", "second"]));
	}

	#[test]
	fn same_generation_operations_remain_serialized() {
		let registry = Arc::new(OwnedGenerationRegistry::new());
		assert!(registry.insert("generation".to_owned(), 0_u8).unwrap());

		let release = Arc::new((Mutex::new(false), Condvar::new()));
		let (first_entered_sender, first_entered_receiver) = mpsc::sync_channel(1);
		let first_registry = Arc::clone(&registry);
		let first_release = Arc::clone(&release);
		let first = thread::spawn(move || {
			let result = first_registry
				.with_current("generation", |value| {
					*value += 1;
					first_entered_sender.send(()).unwrap();
					let (released, condition) = &*first_release;
					let mut released = released.lock().unwrap();
					while !*released {
						released = condition.wait(released).unwrap();
					}
				})
				.unwrap();
			assert!(result.is_some());
		});
		first_entered_receiver.recv_timeout(CONCURRENCY_TIMEOUT).unwrap();

		let slot = registry.entries.lock().unwrap().get("generation").cloned().unwrap();
		assert!(matches!(slot.value.try_lock(), Err(TryLockError::WouldBlock)));

		{
			let (released, condition) = &*release;
			*released.lock().unwrap() = true;
			condition.notify_all();
		}
		first.join().unwrap();

		let second = registry
			.with_current("generation", |value| {
				*value += 1;
				*value
			})
			.unwrap();
		assert_eq!(second, Some(2));
	}

	#[test]
	fn removal_excludes_new_operations_and_waits_for_the_active_generation() {
		let registry = Arc::new(OwnedGenerationRegistry::new());
		assert!(registry.insert("generation".to_owned(), 0_u8).unwrap());

		let release = Arc::new((Mutex::new(false), Condvar::new()));
		let (active_sender, active_receiver) = mpsc::sync_channel(1);
		let active_registry = Arc::clone(&registry);
		let active_release = Arc::clone(&release);
		let active = thread::spawn(move || {
			let result = active_registry
				.with_current("generation", |value| {
					*value += 1;
					active_sender.send(()).unwrap();
					let (released, condition) = &*active_release;
					let mut released = released.lock().unwrap();
					while !*released {
						released = condition.wait(released).unwrap();
					}
				})
				.unwrap();
			assert!(result.is_some());
		});
		active_receiver.recv_timeout(CONCURRENCY_TIMEOUT).unwrap();

		let (take_started_sender, take_started_receiver) = mpsc::sync_channel(1);
		let (take_finished_sender, take_finished_receiver) = mpsc::sync_channel(1);
		let take_registry = Arc::clone(&registry);
		let taker = thread::spawn(move || {
			take_started_sender.send(()).unwrap();
			let value = take_registry.take("generation").unwrap();
			take_finished_sender.send(value).unwrap();
		});
		take_started_receiver.recv_timeout(CONCURRENCY_TIMEOUT).unwrap();
		let deadline = Instant::now() + CONCURRENCY_TIMEOUT;
		while registry.contains("generation").unwrap() {
			assert!(Instant::now() < deadline, "generation removal did not become visible");
			thread::yield_now();
		}

		let new_operation = registry.with_current("generation", |_| ());
		let finished_while_active = take_finished_receiver.recv_timeout(EXCLUSION_WINDOW);
		assert!(new_operation.unwrap().is_none());
		assert!(matches!(finished_while_active, Err(mpsc::RecvTimeoutError::Timeout)));

		{
			let (released, condition) = &*release;
			*released.lock().unwrap() = true;
			condition.notify_all();
		}
		assert_eq!(take_finished_receiver.recv_timeout(CONCURRENCY_TIMEOUT).unwrap(), Some(1));
		active.join().unwrap();
		taker.join().unwrap();
	}
}
