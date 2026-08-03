//! Daemon-owned manual reset-card service.
//!
//! PostgreSQL owns account admission and the durable effect fence. The process adapter owns
//! credential projection and exact provider calls. Public callers can select only one configured
//! vNext account UUID and one public grant/expiry descriptor.

use std::{
	fmt::{Debug, Formatter},
	future::Future,
	sync::{
		Arc,
		atomic::{AtomicBool, AtomicUsize, Ordering},
	},
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use decodex_codex::{
	AccountRateLimitObservation, ExactResetCreditId, ResetCardIdempotencyKey, ResetCardInventory,
	ResetCardResolutionError,
};
use decodex_core::{
	AccountId, AccountLifecycleReadiness, AccountOperationId, AccountQuotaDisposition,
	AccountQuotaObservationError, AccountQuotaWindow, AccountQuotaWindowObservation, AccountRecord,
	AccountState, ProcessExecutionAuthorization, ProcessGenerationAccountBinding,
	ProcessGenerationId, ResetCardConsumeOutcome, ResetCardDescriptor, ServerIdentity,
};
use decodex_postgres::{
	CommandIdentity, OutboxReconciliation, PostgresStore, ReconciliationOutcome, ResetCardClaim,
	ResetCardFailureCode, ResetCardOperationStatus, ResetCardPreparation, StoreError,
};
use serde_json::json;
use tokio::{
	sync::{Mutex, Notify, oneshot, watch},
	task, time,
};

use crate::{
	account_service::{AccountLifecycleError, AccountProcessCredential, AccountService},
	host_credentials::StoredCredential,
	process_supervisor::{
		FencedProcess, ProcessGenerationControl, ProcessGenerationTermination,
		ProcessSupervisorError,
	},
};

use super::{
	CapacityExhausted, RunnerCapacity,
	process::{
		AccountBinding, AccountIdentity, AccountRefreshCallback, AttestedAppServerLaunch,
		AttestedAppServerProfile, AttestedProcessChild, ChatgptRefreshProjection,
		CredentialProjection, CredentialVault, CredentialVaultError, ResetCardConsumeReadback,
		ResetCardProcessError,
	},
};

const PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
// The longest consume path reserves eight sequential process deadlines, including conservative
// startup headroom plus initialize, credential projection, initial account read, consume, one
// incomplete-detail inventory retry, and account re-attestation. Process-group and stdout-pump
// shutdown add 1.25 seconds, rounded up to two seconds for this lease proof.
const MAX_BLOCKING_PROCESS_DEADLINE: Duration =
	Duration::from_secs(PROCESS_TIMEOUT.as_secs() * 8 + 2);
// A query receives a typed row-scoped refusal before the protocol client's whole-request
// deadline. The daemon-owned operation continues its already-bounded cleanup.
const INVENTORY_RESPONSE_TIMEOUT: Duration = Duration::from_secs(25);
const CLAIM_LEASE: Duration = Duration::from_secs(360);
const CLAIM_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
// A detached blocking task must begin early enough that all bounded provider work finishes before
// the lease written by the initial renewal can expire. One additional second makes the inequality
// strict. The gate is anchored before that renewal request, so database response latency cannot
// move the local start deadline past the database lease deadline.
const MAX_CLAIM_WORK_START_DELAY: Duration = Duration::from_secs(
	CLAIM_LEASE.as_secs()
		- MAX_BLOCKING_PROCESS_DEADLINE.as_secs()
		- CLAIM_HEARTBEAT_INTERVAL.as_secs()
		- 1,
);
const _: () = assert!(
	MAX_CLAIM_WORK_START_DELAY.as_secs()
		+ MAX_BLOCKING_PROCESS_DEADLINE.as_secs()
		+ CLAIM_HEARTBEAT_INTERVAL.as_secs()
		< CLAIM_LEASE.as_secs()
);
const WORKER_IDLE_POLL: Duration = Duration::from_secs(5);
const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const RECONCILIATION_DELAY: Duration = Duration::from_secs(1);

#[derive(Clone, Copy)]
enum InitialCredentialProjection {
	Stored,
	CallbackProbe,
}

/// Public reset-card observation plus the exact account revision observed around process work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResetCardInventoryView {
	pub account_id: AccountId,
	pub account_revision: i64,
	pub reported_available_count: Option<u64>,
	pub details_complete: bool,
	pub cards: Vec<ResetCardDescriptor>,
	pub five_hour_quota: AccountQuotaWindowObservation,
	pub seven_day_quota: AccountQuotaWindowObservation,
}

/// Row-scoped failed provider observation with facts persisted by the Account Service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResetCardObservationFailure {
	pub account_id: AccountId,
	pub account_revision: i64,
	pub five_hour_quota: AccountQuotaWindowObservation,
	pub seven_day_quota: AccountQuotaWindowObservation,
	pub error: ResetCardServiceError,
}

/// One bounded account/rateLimits/read result after quota persistence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResetCardInventoryObservation {
	Available(ResetCardInventoryView),
	ObservationFailed(ResetCardObservationFailure),
}

/// Whether the daemon-owned Account Service and host store are composed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResetCardVaultStatus {
	NotConfigured,
	Ready,
	Unavailable,
}

/// Closed service failures safe to map to the public protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResetCardServiceError {
	InvalidRequest,
	AccountNotFound,
	AccountStateRejected,
	AccountChanged,
	ExpectedRevisionMismatch { actual: i64 },
	VaultUnavailable,
	SchemaUnsupported,
	ProviderUnavailable,
	InventoryIncomplete,
	InventoryChanged,
	RequestTimedOut,
	ResourceExhausted,
	ProductStateUnavailable,
	IdempotencyConflict,
	AcceptanceUnknown,
}

/// Fail-closed startup error without environment values or database text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResetCardStartupError {
	CapacityUnavailable,
	AccountIdentityConflict,
	AccountEnrollmentUnavailable,
	AccountEnrollmentConflict,
	WorkerIdentityUnavailable,
}

#[derive(Clone)]
pub(crate) struct ResetCardRuntime {
	inner: Arc<ResetCardRuntimeInner>,
}

struct ResetCardRuntimeInner {
	store: PostgresStore,
	accounts: Arc<AccountService>,
	capacity: Arc<RunnerCapacity>,
	process_generations: ProcessGenerationControl,
	execution_authorization: ProcessExecutionAuthorization,
	launch_profile: AttestedAppServerProfile,
	worker_id: String,
	worker_lock: Mutex<()>,
	worker_wakeup: Arc<Notify>,
	provider_work: Arc<ProviderWorkLifecycle>,
}

/// Owner-held cancellation and absolute start fence for blocking provider work.
///
/// Dropping the gate closes every permit. A blocking task that was queued after its async owner
/// disappeared therefore cannot start provider work later under an expired or reclaimed claim.
struct ClaimWorkGate {
	state: Arc<ClaimWorkState>,
}

#[derive(Clone)]
struct ClaimWorkPermit {
	state: Arc<ClaimWorkState>,
}

struct ClaimWorkState {
	open: AtomicBool,
	start_deadline: Instant,
}

#[derive(Clone)]
struct ResetCardReconciliationLaunch {
	outbox_id: i64,
	worker_id: String,
	claim_token: String,
}

type InventoryResult = Result<ResetCardInventoryObservation, ResetCardServiceError>;
impl ResetCardReconciliationLaunch {
	fn from_claim(inner: &ResetCardRuntimeInner, claim: &ResetCardClaim) -> Self {
		Self {
			outbox_id: claim.id,
			worker_id: inner.worker_id.clone(),
			claim_token: claim.claim_token().to_owned(),
		}
	}
}

/// Daemon-lifecycle fence and exact accounting for blocking provider work.
///
/// Registration happens before `spawn_blocking`. Closing the lifecycle prevents a queued closure
/// from starting provider work, while the retained permit keeps already-started or queued work in
/// the service's settlement count even if its async caller is cancelled.
struct ProviderWorkLifecycle {
	open: AtomicBool,
	active: AtomicUsize,
	settled: Notify,
}

struct ProviderWorkPermit {
	lifecycle: Arc<ProviderWorkLifecycle>,
}

impl ClaimWorkGate {
	fn from_renewal_start(renewal_started_at: Instant) -> Self {
		Self::with_deadline(renewal_started_at + MAX_CLAIM_WORK_START_DELAY)
	}

	fn with_deadline(start_deadline: Instant) -> Self {
		Self { state: Arc::new(ClaimWorkState { open: AtomicBool::new(true), start_deadline }) }
	}

	fn permit(&self) -> ClaimWorkPermit {
		ClaimWorkPermit { state: Arc::clone(&self.state) }
	}

	fn close(&self) {
		self.state.open.store(false, Ordering::Release);
	}
}

impl Drop for ClaimWorkGate {
	fn drop(&mut self) {
		self.close();
	}
}

impl ClaimWorkPermit {
	fn permits_start(&self) -> bool {
		self.state.open.load(Ordering::Acquire) && Instant::now() < self.state.start_deadline
	}
}

impl ProviderWorkLifecycle {
	fn new() -> Self {
		Self { open: AtomicBool::new(true), active: AtomicUsize::new(0), settled: Notify::new() }
	}

	fn register(self: &Arc<Self>) -> Option<ProviderWorkPermit> {
		if !self.open.load(Ordering::Acquire) {
			return None;
		}
		if self
			.active
			.fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| active.checked_add(1))
			.is_err()
		{
			return None;
		}

		let permit = ProviderWorkPermit { lifecycle: Arc::clone(self) };
		if !self.open.load(Ordering::Acquire) {
			drop(permit);

			return None;
		}

		Some(permit)
	}

	fn close(&self) {
		self.open.store(false, Ordering::Release);
		if self.active.load(Ordering::Acquire) == 0 {
			self.settled.notify_waiters();
		}
	}

	async fn wait_for_settlement(&self) {
		loop {
			let settled = self.settled.notified();

			tokio::pin!(settled);
			settled.as_mut().enable();
			if self.active.load(Ordering::Acquire) == 0 {
				return;
			}

			settled.await;
		}
	}
}

impl ProviderWorkPermit {
	fn permits_start(&self) -> bool {
		self.lifecycle.open.load(Ordering::Acquire)
	}
}

impl Drop for ProviderWorkPermit {
	fn drop(&mut self) {
		let previous = self.lifecycle.active.fetch_sub(1, Ordering::AcqRel);

		debug_assert!(previous > 0, "provider work lifecycle accounting underflow");
		if previous == 1 {
			self.lifecycle.settled.notify_waiters();
		}
	}
}

impl ResetCardRuntime {
	/// Compose Reset Card with the singleton Account Service and its exact host-store authority.
	pub(crate) fn start(
		store: PostgresStore,
		accounts: Arc<AccountService>,
		process_generations: ProcessGenerationControl,
		execution_authorization: ProcessExecutionAuthorization,
		launch_profile: AttestedAppServerProfile,
	) -> Result<Self, ResetCardStartupError> {
		let capacity =
			RunnerCapacity::daemon().map_err(|_| ResetCardStartupError::CapacityUnavailable)?;
		let worker_id = ServerIdentity::generate()
			.map_err(|_| ResetCardStartupError::WorkerIdentityUnavailable)?
			.as_str()
			.to_owned();
		let runtime = Self {
			inner: Arc::new(ResetCardRuntimeInner {
				store,
				accounts,
				capacity,
				process_generations,
				execution_authorization,
				launch_profile,
				worker_id,
				worker_lock: Mutex::new(()),
				worker_wakeup: Arc::new(Notify::new()),
				provider_work: Arc::new(ProviderWorkLifecycle::new()),
			}),
		};

		runtime.inner.worker_wakeup.notify_one();

		Ok(runtime)
	}

	/// Run the one lifecycle-owned worker until shutdown and all provider work settle.
	pub(crate) async fn daemon_service(self, mut stop: watch::Receiver<bool>) {
		let worker_stop = stop.clone();
		let worker_inner = Arc::clone(&self.inner);
		let worker = run_worker(worker_inner, worker_stop);

		tokio::pin!(worker);

		tokio::select! {
			biased;

			() = shutdown_requested(&mut stop) => {
				self.inner.provider_work.close();
				worker.await;
			},
			() = &mut worker => {
				self.inner.provider_work.close();
			},
		}

		self.inner.provider_work.wait_for_settlement().await;
	}

	/// Close provider admission synchronously when the server enters its stopping phase.
	pub(crate) fn begin_shutdown(&self) {
		self.inner.provider_work.close();
	}

	/// Wait until every registered blocking provider closure exits.
	pub(crate) async fn wait_for_shutdown(&self) {
		self.inner.provider_work.wait_for_settlement().await;
	}

	pub(crate) fn vault_status(&self) -> ResetCardVaultStatus {
		ResetCardVaultStatus::Ready
	}

	/// Run one bounded exact-image login, refresh callback, CAS, and provider-readback proof.
	pub(crate) async fn prove_callback_capability(
		&self,
		account: &AccountRecord,
	) -> Result<(), ResetCardServiceError> {
		let credential = self
			.inner
			.accounts
			.process_credential(&account.account_id, account.revision)
			.await
			.map_err(map_account_service_error)?;
		let initial = credential.binding.credential.clone();
		let provider_permit = Arc::new(
			self.inner
				.provider_work
				.register()
				.ok_or(ResetCardServiceError::ProductStateUnavailable)?,
		);
		let mut process = prepare_fenced_reset_process(
			Arc::clone(&self.inner),
			account.account_id.clone(),
			credential,
			Arc::clone(&provider_permit),
			None,
			None,
			InitialCredentialProjection::CallbackProbe,
		)
		.await?;
		let control = self.inner.process_generations.clone();
		let process_for_proof = process.clone();
		let permit_for_proof = Arc::clone(&provider_permit);
		let result = task::spawn_blocking(move || {
			require_provider_work_start(&permit_for_proof, None)?;
			control
				.with_fenced_child(&process_for_proof, |child| child.prove_refresh_callback())
				.map_err(map_process_supervisor_error)?
				.map_err(map_process_error)
		})
		.await
		.map_err(|_| ResetCardServiceError::ResourceExhausted)?;
		finish_fenced_reset_process(&self.inner, &mut process, result).await?;
		self.inner
			.accounts
			.verify_callback_successor(&account.account_id, &initial)
			.await
			.map_err(map_account_service_error)
	}

	/// Read one fresh, strict provider inventory under a PostgreSQL revision fence.
	pub(crate) async fn inventory(
		&self,
		account_id: &AccountId,
	) -> Result<ResetCardInventoryObservation, ResetCardServiceError> {
		let provider_permit = Arc::new(
			self.inner
				.provider_work
				.register()
				.ok_or(ResetCardServiceError::ProductStateUnavailable)?,
		);
		let inner = Arc::clone(&self.inner);
		let account_id = account_id.clone();
		let mut owner =
			task::spawn(
				async move { Self::inventory_once(inner, account_id, provider_permit).await },
			);
		await_inventory_owner(&mut owner, INVENTORY_RESPONSE_TIMEOUT).await
	}

	async fn inventory_once(
		inner: Arc<ResetCardRuntimeInner>,
		account_id: AccountId,
		provider_permit: Arc<ProviderWorkPermit>,
	) -> InventoryResult {
		let credential = inner
			.accounts
			.process_credential_for_observation(&account_id, MAX_BLOCKING_PROCESS_DEADLINE)
			.await
			.map_err(map_account_service_error)?;
		let account_revision = credential.binding.account_revision;
		let process_binding = credential.binding.clone();
		let observed_at_unix_micros =
			current_unix_micros().ok_or(ResetCardServiceError::ProductStateUnavailable)?;
		let inventory = run_inventory(
			Arc::clone(&inner),
			account_id.clone(),
			credential,
			None,
			None,
			Some(provider_permit),
		)
		.await;

		match inventory {
			Ok(inventory) => {
				let [five_hour_quota, seven_day_quota] = persist_quota_observations(
					inner.accounts.as_ref(),
					&account_id,
					*inventory.quota_windows(),
					observed_at_unix_micros,
				)
				.await?;
				let current = inner
					.accounts
					.process_credential_for_existing_work(&account_id, &process_binding)
					.await
					.map_err(map_account_service_error)?;
				if current.binding.credential != process_binding.credential
					|| current.binding.refresh_callback_profile_sha256
						!= process_binding.refresh_callback_profile_sha256
				{
					return Err(ResetCardServiceError::AccountChanged);
				}

				Ok(ResetCardInventoryObservation::Available(ResetCardInventoryView {
					account_id: account_id.clone(),
					account_revision: current.binding.account_revision,
					reported_available_count: inventory.reported_available_count(),
					details_complete: inventory.details_complete(),
					cards: if inventory.details_complete() {
						inventory.available_cards().iter().map(|card| card.descriptor()).collect()
					} else {
						Vec::new()
					},
					five_hour_quota,
					seven_day_quota,
				}))
			},
			Err(error) => {
				let quota_error = quota_error_for_service(error);
				let [five_hour_quota, seven_day_quota] = persist_quota_errors(
					inner.accounts.as_ref(),
					&account_id,
					quota_error,
					observed_at_unix_micros,
				)
				.await?;

				Ok(ResetCardInventoryObservation::ObservationFailed(ResetCardObservationFailure {
					account_id: account_id.clone(),
					account_revision,
					five_hour_quota,
					seven_day_quota,
					error,
				}))
			},
		}
	}

	/// Atomically prepare one exact selection and wake the daemon worker. This method never waits
	/// for an app-server process or external effect.
	pub(crate) async fn prepare(
		&self,
		idempotency_key: &str,
		account_id: &AccountId,
		expected_revision: i64,
		descriptor: ResetCardDescriptor,
	) -> Result<ResetCardPreparation, ResetCardServiceError> {
		let request = format!(
			"decodex/reset-card-operation/1\n{}\n{}\n{}\n{}",
			account_id.as_str(),
			expected_revision,
			descriptor.granted_at().unix_seconds(),
			descriptor.expires_at().unix_seconds(),
		);
		let command = CommandIdentity::new(idempotency_key, request.as_bytes())
			.map_err(map_prepare_store_error)?;
		if let Some(preparation) =
			self.replay_preparation(&command, account_id, expected_revision, descriptor).await?
		{
			self.inner.worker_wakeup.notify_one();

			return Ok(preparation);
		}
		let account = match self.admitted_account(account_id).await {
			Ok(account) => account,
			Err(error) => {
				return self
					.replay_preparation_or(
						&command,
						account_id,
						expected_revision,
						descriptor,
						error,
					)
					.await;
			},
		};
		if account.revision != expected_revision {
			return self
				.replay_preparation_or(
					&command,
					account_id,
					expected_revision,
					descriptor,
					ResetCardServiceError::ExpectedRevisionMismatch { actual: account.revision },
				)
				.await;
		}
		let process_credential =
			match self.inner.accounts.process_credential(account_id, expected_revision).await {
				Ok(credential) => credential,
				Err(error) => {
					return self
						.replay_preparation_or(
							&command,
							account_id,
							expected_revision,
							descriptor,
							map_account_service_error(error),
						)
						.await;
				},
			};
		let preparation = self
			.inner
			.store
			.prepare_reset_card_operation(
				&command,
				account_id,
				expected_revision,
				&process_credential.binding,
				descriptor,
			)
			.await
			.map_err(map_prepare_store_error)?;

		self.inner.worker_wakeup.notify_one();

		Ok(preparation)
	}

	async fn replay_preparation(
		&self,
		command: &CommandIdentity,
		account_id: &AccountId,
		expected_revision: i64,
		descriptor: ResetCardDescriptor,
	) -> Result<Option<ResetCardPreparation>, ResetCardServiceError> {
		let callback_profile = self.inner.accounts.reset_card_callback_profile();
		self.inner
			.store
			.replay_reset_card_preparation(
				command,
				account_id,
				expected_revision,
				descriptor,
				callback_profile.as_deref(),
			)
			.await
			.map_err(map_prepare_store_error)
	}

	async fn replay_preparation_or(
		&self,
		command: &CommandIdentity,
		account_id: &AccountId,
		expected_revision: i64,
		descriptor: ResetCardDescriptor,
		error: ResetCardServiceError,
	) -> Result<ResetCardPreparation, ResetCardServiceError> {
		match self.replay_preparation(command, account_id, expected_revision, descriptor).await? {
			Some(preparation) => {
				self.inner.worker_wakeup.notify_one();

				Ok(preparation)
			},
			None => Err(error),
		}
	}

	pub(crate) async fn operation_status(
		&self,
		idempotency_key: &str,
	) -> Result<ResetCardOperationStatus, ResetCardServiceError> {
		self.inner.store.reset_card_operation_status(idempotency_key).await.map_err(|error| {
			match error {
				StoreError::InvalidInput(_) => ResetCardServiceError::InvalidRequest,
				StoreError::Incompatible(_) => ResetCardServiceError::ProductStateUnavailable,
				_ => ResetCardServiceError::ProductStateUnavailable,
			}
		})
	}

	async fn admitted_account(
		&self,
		account_id: &AccountId,
	) -> Result<AccountRecord, ResetCardServiceError> {
		let account = self
			.inner
			.accounts
			.inspect(account_id)
			.await
			.map_err(map_account_service_error)?
			.account;
		if !reset_card_account_admitted(&account) {
			return Err(ResetCardServiceError::AccountStateRejected);
		}

		Ok(account)
	}
}

async fn await_inventory_owner(
	owner: &mut task::JoinHandle<InventoryResult>,
	response_timeout: Duration,
) -> InventoryResult {
	match time::timeout(response_timeout, owner).await {
		Ok(Ok(result)) => result,
		Ok(Err(_)) => Err(ResetCardServiceError::ResourceExhausted),
		Err(_) => Err(ResetCardServiceError::RequestTimedOut),
	}
}

async fn run_worker(inner: Arc<ResetCardRuntimeInner>, mut stop: watch::Receiver<bool>) {
	loop {
		if *stop.borrow() {
			return;
		}

		drain_worker(Arc::clone(&inner), &stop).await;
		if *stop.borrow() {
			return;
		}

		tokio::select! {
			biased;

			() = shutdown_requested(&mut stop) => return,
			() = inner.worker_wakeup.notified() => {},
			() = time::sleep(WORKER_IDLE_POLL) => {},
		}
	}
}

async fn drain_worker(inner: Arc<ResetCardRuntimeInner>, stop: &watch::Receiver<bool>) {
	let Ok(_worker) = inner.worker_lock.try_lock() else {
		return;
	};

	loop {
		if *stop.borrow() {
			return;
		}

		let claim =
			match inner.store.claim_reset_card_operation(&inner.worker_id, CLAIM_LEASE).await {
				Ok(Some(claim)) => claim,
				Ok(None) | Err(_) => return,
			};

		process_claim(Arc::clone(&inner), claim).await;
	}
}

async fn shutdown_requested(receiver: &mut watch::Receiver<bool>) {
	loop {
		if *receiver.borrow_and_update() {
			return;
		}
		if receiver.changed().await.is_err() {
			return;
		}
	}
}

async fn process_claim(inner: Arc<ResetCardRuntimeInner>, claim: ResetCardClaim) {
	if !claim.requires_reconciliation && !claim_binding_admitted(&inner, &claim).await {
		fail_before_effect(&inner, &claim, ResetCardFailureCode::AccountChanged).await;

		return;
	}
	let idempotency_key = match provider_idempotency_key(claim.provider_idempotency_key()) {
		Ok(key) => key,
		Err(failure) => {
			fail_before_effect(&inner, &claim, failure).await;

			return;
		},
	};

	let Some(exact_credit_id) = resolve_claim_credit_id(&inner, &claim).await else {
		return;
	};

	if let Some(recorded_outcome) = claim.recorded_outcome {
		reconcile_recorded_claim(&inner, &claim, &exact_credit_id, recorded_outcome).await;
	} else {
		process_new_claim(&inner, &claim, exact_credit_id, idempotency_key).await;
	}
}

async fn resolve_claim_credit_id(
	inner: &Arc<ResetCardRuntimeInner>,
	claim: &ResetCardClaim,
) -> Option<ExactResetCreditId> {
	match claim.exact_credit_id() {
		Some(value) => ExactResetCreditId::new(value.to_owned()).ok(),
		None if claim.requires_reconciliation => None,
		None => {
			let credential = match claim_process_credential(inner, claim).await {
				Ok(credential) => credential,
				Err(error) => {
					fail_before_effect(inner, claim, failure_code(error)).await;
					return None;
				},
			};
			let inventory = match run_with_claim_heartbeat(inner, claim, |permit| {
				run_inventory(
					Arc::clone(inner),
					claim.account_id.clone(),
					credential,
					None,
					Some(permit),
					None,
				)
			})
			.await
			{
				Ok(inventory) => inventory,
				Err(error) => {
					fail_before_effect(inner, claim, failure_code(error)).await;

					return None;
				},
			};
			let exact = match inventory.resolve_exact_credit_id(claim.descriptor) {
				Ok(exact) => exact,
				Err(
					ResetCardResolutionError::Incomplete
					| ResetCardResolutionError::NotFound
					| ResetCardResolutionError::Ambiguous,
				) => {
					fail_before_effect(inner, claim, ResetCardFailureCode::InventoryChanged).await;

					return None;
				},
			};
			if !claim_binding_admitted(inner, claim).await {
				fail_before_effect(inner, claim, ResetCardFailureCode::AccountChanged).await;

				return None;
			}
			if inner
				.store
				.bind_reset_card_credit(claim, &inner.worker_id, exact.as_str())
				.await
				.is_err()
			{
				return None;
			}

			Some(exact)
		},
	}
}

async fn process_new_claim(
	inner: &Arc<ResetCardRuntimeInner>,
	claim: &ResetCardClaim,
	exact_credit_id: ExactResetCreditId,
	idempotency_key: ResetCardIdempotencyKey,
) {
	let credential = match claim_process_credential(inner, claim).await {
		Ok(credential) => credential,
		Err(error) => {
			fail_before_effect(inner, claim, failure_code(error)).await;
			return;
		},
	};
	if !claim.requires_reconciliation {
		match inner.store.begin_reset_card_effect(claim, &inner.worker_id).await {
			Ok(()) => {},
			Err(StoreError::ResetCardSelectionConflict) => {
				fail_before_effect(inner, claim, ResetCardFailureCode::InventoryChanged).await;

				return;
			},
			Err(StoreError::RevisionConflict { .. } | StoreError::InvalidInput(_)) => {
				fail_before_effect(inner, claim, ResetCardFailureCode::AccountChanged).await;

				return;
			},
			Err(_) => return,
		}
	}
	let reconciliation_credit_id = exact_credit_id.clone();
	let launch_authority = ResetCardReconciliationLaunch::from_claim(inner, claim);
	let readback = match run_with_claim_heartbeat(inner, claim, |permit| {
		run_consume(
			Arc::clone(inner),
			claim.account_id.clone(),
			credential,
			exact_credit_id,
			idempotency_key,
			launch_authority,
			permit,
		)
	})
	.await
	{
		Ok(readback) => readback,
		Err(_) => return,
	};
	let outcome = readback.outcome;
	let receipt = json!({"outcome": outcome_text(outcome)});
	if inner
		.store
		.record_outbox_receipt(claim.id, &inner.worker_id, claim.claim_token(), &receipt)
		.await
		.is_err()
	{
		return;
	}
	complete_reconciliation(inner, claim, &reconciliation_credit_id, outcome, &readback.inventory)
		.await;
}

async fn reconcile_recorded_claim(
	inner: &Arc<ResetCardRuntimeInner>,
	claim: &ResetCardClaim,
	exact_credit_id: &ExactResetCreditId,
	recorded_outcome: ResetCardConsumeOutcome,
) {
	let credential = match claim_process_credential(inner, claim).await {
		Ok(credential) => credential,
		Err(_) => return,
	};
	let inventory = match run_with_claim_heartbeat(inner, claim, |permit| {
		run_inventory(
			Arc::clone(inner),
			claim.account_id.clone(),
			credential,
			Some(ResetCardReconciliationLaunch::from_claim(inner, claim)),
			Some(permit),
			None,
		)
	})
	.await
	{
		Ok(inventory) => inventory,
		Err(_) => return,
	};
	complete_reconciliation(inner, claim, exact_credit_id, recorded_outcome, &inventory).await;
}

async fn run_with_claim_heartbeat<T, MakeWork, Work>(
	inner: &Arc<ResetCardRuntimeInner>,
	claim: &ResetCardClaim,
	make_work: MakeWork,
) -> Result<T, ResetCardServiceError>
where
	MakeWork: FnOnce(ClaimWorkPermit) -> Work,
	Work: Future<Output = Result<T, ResetCardServiceError>>,
{
	let renewal_started_at = Instant::now();
	inner
		.store
		.renew_reset_card_claim(claim.id, &inner.worker_id, claim.claim_token(), CLAIM_LEASE)
		.await
		.map_err(|_| ResetCardServiceError::ProductStateUnavailable)?;
	let gate = ClaimWorkGate::from_renewal_start(renewal_started_at);

	let (stop_sender, stop_receiver) = oneshot::channel();
	let heartbeat_store = inner.store.clone();
	let heartbeat_claim_id = claim.id;
	let heartbeat_claim_token = Arc::<str>::from(claim.claim_token());
	let heartbeat_worker = inner.worker_id.clone();
	let heartbeat = maintain_claim_heartbeat(
		move || {
			let store = heartbeat_store.clone();
			let claim_token = Arc::clone(&heartbeat_claim_token);
			let worker = heartbeat_worker.clone();

			async move {
				store
					.renew_reset_card_claim(heartbeat_claim_id, &worker, &claim_token, CLAIM_LEASE)
					.await
			}
		},
		stop_receiver,
		CLAIM_HEARTBEAT_INTERVAL,
	);

	finish_guarded_work(make_work(gate.permit()), heartbeat, stop_sender, &gate).await
}

async fn maintain_claim_heartbeat<Renew, RenewFuture>(
	mut renew: Renew,
	mut stop: oneshot::Receiver<()>,
	interval: Duration,
) -> Result<(), StoreError>
where
	Renew: FnMut() -> RenewFuture,
	RenewFuture: Future<Output = Result<(), StoreError>>,
{
	loop {
		tokio::select! {
			_ = &mut stop => return Ok(()),
			() = time::sleep(interval) => renew().await?,
		}
	}
}

async fn finish_guarded_work<T, Work, Heartbeat>(
	work: Work,
	heartbeat: Heartbeat,
	stop_sender: oneshot::Sender<()>,
	gate: &ClaimWorkGate,
) -> Result<T, ResetCardServiceError>
where
	Work: Future<Output = Result<T, ResetCardServiceError>>,
	Heartbeat: Future<Output = Result<(), StoreError>>,
{
	tokio::pin!(work);
	tokio::pin!(heartbeat);

	tokio::select! {
		result = &mut work => {
			gate.close();
			let _ = stop_sender.send(());

			match heartbeat.await {
				Ok(()) => result,
				Err(_) => Err(ResetCardServiceError::ProductStateUnavailable),
			}
		},
		result = &mut heartbeat => {
			gate.close();
			drop(stop_sender);

			match result {
				Ok(()) | Err(_) => Err(ResetCardServiceError::ProductStateUnavailable),
			}
		},
	}
}

async fn complete_reconciliation(
	inner: &ResetCardRuntimeInner,
	claim: &ResetCardClaim,
	exact_credit_id: &ExactResetCreditId,
	outcome: ResetCardConsumeOutcome,
	inventory: &ResetCardInventory,
) {
	if !inventory.details_complete() {
		return;
	}
	let selected_exact_credit_available = inventory.contains_exact_credit_id(exact_credit_id);
	let selected_descriptor_expired = current_unix_seconds()
		.is_some_and(|now| now >= claim.descriptor.expires_at().unix_seconds());
	if !readback_confirms_outcome(
		outcome,
		selected_exact_credit_available,
		selected_descriptor_expired,
	) {
		return;
	}
	let readback = json!({
		"schema": "decodex/reset-card-readback/1",
		"account_id": claim.account_id.as_str(),
		"account_revision": claim.account_revision,
		"outcome": outcome_text(outcome),
		"available_count": inventory.available_count(),
		"selected_exact_credit_available": selected_exact_credit_available,
		"selected_descriptor_expired": selected_descriptor_expired,
	});
	let _ = inner
		.store
		.reconcile_outbox(
			claim.id,
			&inner.worker_id,
			claim.claim_token(),
			&OutboxReconciliation { readback, outcome: ReconciliationOutcome::EffectPresent },
			RECONCILIATION_DELAY,
			RETENTION,
		)
		.await;
}

const fn readback_confirms_outcome(
	outcome: ResetCardConsumeOutcome,
	selected_exact_credit_available: bool,
	selected_descriptor_expired: bool,
) -> bool {
	match outcome {
		ResetCardConsumeOutcome::NothingToReset =>
			selected_exact_credit_available || selected_descriptor_expired,
		ResetCardConsumeOutcome::Reset
		| ResetCardConsumeOutcome::NoCredit
		| ResetCardConsumeOutcome::AlreadyRedeemed => !selected_exact_credit_available,
	}
}

fn current_unix_seconds() -> Option<i64> {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.ok()
		.and_then(|duration| i64::try_from(duration.as_secs()).ok())
}

async fn run_inventory(
	inner: Arc<ResetCardRuntimeInner>,
	account_id: AccountId,
	credential: AccountProcessCredential,
	reconciliation: Option<ResetCardReconciliationLaunch>,
	claim_permit: Option<ClaimWorkPermit>,
	provider_permit: Option<Arc<ProviderWorkPermit>>,
) -> Result<ResetCardInventory, ResetCardServiceError> {
	let provider_permit = match provider_permit {
		Some(permit) => permit,
		None => Arc::new(
			inner.provider_work.register().ok_or(ResetCardServiceError::ProductStateUnavailable)?,
		),
	};
	let mut process = prepare_fenced_reset_process(
		Arc::clone(&inner),
		account_id,
		credential,
		Arc::clone(&provider_permit),
		reconciliation,
		claim_permit.clone(),
		InitialCredentialProjection::Stored,
	)
	.await?;
	let control = inner.process_generations.clone();
	let process_for_work = process.clone();
	let permit_for_work = Arc::clone(&provider_permit);
	let result = task::spawn_blocking(move || {
		require_provider_work_start(&permit_for_work, claim_permit.as_ref())?;
		control
			.with_fenced_child(&process_for_work, |child| child.read_reset_card_inventory())
			.map_err(map_process_supervisor_error)?
			.map_err(map_process_error)
	})
	.await
	.map_err(|_| ResetCardServiceError::ResourceExhausted)?;
	finish_fenced_reset_process(&inner, &mut process, result).await
}

async fn run_consume(
	inner: Arc<ResetCardRuntimeInner>,
	account_id: AccountId,
	credential: AccountProcessCredential,
	exact_credit_id: ExactResetCreditId,
	idempotency_key: ResetCardIdempotencyKey,
	reconciliation: ResetCardReconciliationLaunch,
	claim_permit: ClaimWorkPermit,
) -> Result<ResetCardConsumeReadback, ResetCardServiceError> {
	let provider_permit = Arc::new(
		inner.provider_work.register().ok_or(ResetCardServiceError::ProductStateUnavailable)?,
	);
	let mut process = prepare_fenced_reset_process(
		Arc::clone(&inner),
		account_id,
		credential,
		Arc::clone(&provider_permit),
		Some(reconciliation),
		Some(claim_permit.clone()),
		InitialCredentialProjection::Stored,
	)
	.await?;
	let control = inner.process_generations.clone();
	let process_for_work = process.clone();
	let permit_for_work = Arc::clone(&provider_permit);
	let result = task::spawn_blocking(move || {
		require_provider_work_start(&permit_for_work, Some(&claim_permit))?;
		control
			.with_fenced_child(&process_for_work, |child| {
				child.consume_reset_card(exact_credit_id, idempotency_key)
			})
			.map_err(map_process_supervisor_error)?
			.map_err(map_process_error)
	})
	.await
	.map_err(|_| ResetCardServiceError::ResourceExhausted)?;
	finish_fenced_reset_process(&inner, &mut process, result).await
}

async fn prepare_fenced_reset_process(
	inner: Arc<ResetCardRuntimeInner>,
	account_id: AccountId,
	credential: AccountProcessCredential,
	provider_permit: Arc<ProviderWorkPermit>,
	reconciliation: Option<ResetCardReconciliationLaunch>,
	claim_permit: Option<ClaimWorkPermit>,
	initial_projection: InitialCredentialProjection,
) -> Result<FencedProcess, ResetCardServiceError> {
	let generated =
		AccountOperationId::generate().map_err(|_| ResetCardServiceError::ResourceExhausted)?;
	let generation_id = ProcessGenerationId::new(generated.as_str().to_owned())
		.map_err(|_| ResetCardServiceError::ResourceExhausted)?;
	let refresh_callback: Arc<dyn AccountRefreshCallback> =
		Arc::new(AccountServiceRefreshCallback::new(
			Arc::clone(&inner.accounts),
			tokio::runtime::Handle::current(),
			generation_id.clone(),
		));
	let launch_profile = inner.launch_profile.clone();
	let capacity = Arc::clone(&inner.capacity);
	let permit_for_attestation = Arc::clone(&provider_permit);
	let claim_for_attestation = claim_permit.clone();
	let generation_for_launch = generation_id;
	let (generation_id, launch, vault, launch_guard) = task::spawn_blocking(move || {
		require_provider_work_start(&permit_for_attestation, claim_for_attestation.as_ref())?;
		let account_revision = credential.binding.account_revision;
		let binding = AccountBinding::shared_home_bound(
			account_id.clone(),
			credential.binding,
			refresh_callback,
		)
		.map_err(|_| ResetCardServiceError::ProviderUnavailable)?;
		let vault =
			StoredCredentialVault::new(account_id.clone(), credential.stored, initial_projection);
		let launch_guard = credential.launch_guard;
		let capacity_permit = capacity
			.reserve(account_id, account_revision)
			.map_err(|_: CapacityExhausted| ResetCardServiceError::ResourceExhausted)?;
		let launch = AttestedAppServerLaunch::bind(
			launch_profile,
			binding,
			PROCESS_TIMEOUT,
			capacity_permit,
		)
		.map_err(|_| ResetCardServiceError::ProviderUnavailable)?;
		Ok::<_, ResetCardServiceError>((generation_for_launch, launch, vault, launch_guard))
	})
	.await
	.map_err(|_| ResetCardServiceError::ResourceExhausted)??;
	require_provider_work_start(&provider_permit, claim_permit.as_ref())?;
	let mut process = match reconciliation {
		Some(reconciliation) =>
			inner
				.process_generations
				.spawn_fenced_reset_reconciliation(
					generation_id,
					inner.execution_authorization.clone(),
					launch,
					reconciliation.outbox_id,
					&reconciliation.worker_id,
					&reconciliation.claim_token,
				)
				.await,
		None =>
			inner
				.process_generations
				.spawn_fenced(generation_id, inner.execution_authorization.clone(), launch)
				.await,
	}
	.map_err(map_process_supervisor_error)?;
	drop(launch_guard);
	let control = inner.process_generations.clone();
	let process_for_init = process.clone();
	let permit_for_init = Arc::clone(&provider_permit);
	let initialized = task::spawn_blocking(move || {
		require_provider_work_start(&permit_for_init, claim_permit.as_ref())?;
		control
			.with_fenced_child(&process_for_init, |child| vault.initialize(child))
			.map_err(map_process_supervisor_error)?
			.map_err(map_process_error)
	})
	.await
	.map_err(|_| ResetCardServiceError::ResourceExhausted)?;
	if let Err(error) = initialized {
		let _ = terminate_fenced_reset_process(&inner, &process).await;
		return Err(error);
	}
	if let Err(error) = inner.process_generations.mark_spawned_ready(&mut process).await {
		let _ = terminate_fenced_reset_process(&inner, &process).await;
		return Err(map_process_supervisor_error(error));
	}
	Ok(process)
}

async fn finish_fenced_reset_process<T>(
	inner: &ResetCardRuntimeInner,
	process: &mut FencedProcess,
	result: Result<T, ResetCardServiceError>,
) -> Result<T, ResetCardServiceError> {
	terminate_fenced_reset_process(inner, process).await?;
	result
}

async fn terminate_fenced_reset_process(
	inner: &ResetCardRuntimeInner,
	process: &FencedProcess,
) -> Result<(), ResetCardServiceError> {
	match inner
		.process_generations
		.terminate_exact(process.generation_id(), process.revision(), Duration::from_secs(2))
		.await
		.map_err(map_process_supervisor_error)?
	{
		ProcessGenerationTermination::PositiveDeathRecorded
		| ProcessGenerationTermination::AlreadyDead => Ok(()),
		ProcessGenerationTermination::StaleGeneration
		| ProcessGenerationTermination::NotOwned
		| ProcessGenerationTermination::DeathUnproved => Err(ResetCardServiceError::ProviderUnavailable),
	}
}

fn require_provider_work_start(
	provider_permit: &ProviderWorkPermit,
	claim_permit: Option<&ClaimWorkPermit>,
) -> Result<(), ResetCardServiceError> {
	if provider_permit.permits_start() {
		require_claim_work_start(claim_permit)
	} else {
		Err(ResetCardServiceError::ProductStateUnavailable)
	}
}

fn require_claim_work_start(
	claim_permit: Option<&ClaimWorkPermit>,
) -> Result<(), ResetCardServiceError> {
	if claim_permit.is_none_or(ClaimWorkPermit::permits_start) {
		Ok(())
	} else {
		Err(ResetCardServiceError::ProductStateUnavailable)
	}
}

async fn claim_process_credential(
	inner: &ResetCardRuntimeInner,
	claim: &ResetCardClaim,
) -> Result<AccountProcessCredential, ResetCardServiceError> {
	if claim.requires_reconciliation {
		let credential = inner
			.accounts
			.process_credential_for_reconciliation(&claim.account_id, &claim.process_binding)
			.await
			.map_err(map_account_service_error)?;
		if credential.binding.credential.provider != claim.process_binding.credential.provider
			|| credential.binding.refresh_callback_profile_sha256
				!= claim.process_binding.refresh_callback_profile_sha256
		{
			return Err(ResetCardServiceError::AccountChanged);
		}
		return Ok(credential);
	}
	let credential = inner
		.accounts
		.process_credential_for_existing_work(&claim.account_id, &claim.process_binding)
		.await
		.map_err(map_account_service_error)?;
	if credential.binding.credential != claim.process_binding.credential
		|| credential.binding.refresh_callback_profile_sha256
			!= claim.process_binding.refresh_callback_profile_sha256
	{
		return Err(ResetCardServiceError::AccountChanged);
	}

	Ok(credential)
}

async fn claim_binding_admitted(inner: &ResetCardRuntimeInner, claim: &ResetCardClaim) -> bool {
	claim_process_credential(inner, claim).await.is_ok()
}

async fn persist_quota_observations(
	accounts: &AccountService,
	account_id: &AccountId,
	observations: [AccountRateLimitObservation; 2],
	observed_at_unix_micros: i64,
) -> Result<[AccountQuotaWindowObservation; 2], ResetCardServiceError> {
	let mut five_hour = None;
	let mut seven_day = None;
	for observation in observations {
		let persisted =
			persist_quota_observation(accounts, account_id, observation, observed_at_unix_micros)
				.await?;
		match observation.duration_minutes() {
			AccountQuotaWindow::FIVE_HOURS_MINUTES => five_hour = Some(persisted),
			AccountQuotaWindow::SEVEN_DAYS_MINUTES => seven_day = Some(persisted),
			_ => return Err(ResetCardServiceError::InventoryIncomplete),
		}
	}

	match (five_hour, seven_day) {
		(Some(five_hour), Some(seven_day)) => Ok([five_hour, seven_day]),
		_ => Err(ResetCardServiceError::InventoryIncomplete),
	}
}

async fn persist_quota_observation(
	accounts: &AccountService,
	account_id: &AccountId,
	observation: AccountRateLimitObservation,
	observed_at_unix_micros: i64,
) -> Result<AccountQuotaWindowObservation, ResetCardServiceError> {
	let disposition = match observation.result() {
		Ok(fact) => {
			accounts
				.observe_quota(account_id, fact, observed_at_unix_micros)
				.await
				.map_err(map_account_service_error)?;
			AccountQuotaDisposition::Current(fact)
		},
		Err(error) => {
			accounts
				.observe_quota_error(
					account_id,
					observation.duration_minutes(),
					error,
					observed_at_unix_micros,
				)
				.await
				.map_err(map_account_service_error)?;
			AccountQuotaDisposition::Error(error)
		},
	};

	Ok(AccountQuotaWindowObservation {
		duration_minutes: observation.duration_minutes(),
		observed_at_unix_micros: Some(observed_at_unix_micros),
		disposition,
	})
}

async fn persist_quota_errors(
	accounts: &AccountService,
	account_id: &AccountId,
	error: AccountQuotaObservationError,
	observed_at_unix_micros: i64,
) -> Result<[AccountQuotaWindowObservation; 2], ResetCardServiceError> {
	let mut observations = Vec::with_capacity(2);
	for duration_minutes in
		[AccountQuotaWindow::FIVE_HOURS_MINUTES, AccountQuotaWindow::SEVEN_DAYS_MINUTES]
	{
		accounts
			.observe_quota_error(account_id, duration_minutes, error, observed_at_unix_micros)
			.await
			.map_err(map_account_service_error)?;
		observations.push(AccountQuotaWindowObservation {
			duration_minutes,
			observed_at_unix_micros: Some(observed_at_unix_micros),
			disposition: AccountQuotaDisposition::Error(error),
		});
	}

	observations.try_into().map_err(|_| ResetCardServiceError::InventoryIncomplete)
}

fn current_unix_micros() -> Option<i64> {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.ok()
		.and_then(|duration| i64::try_from(duration.as_micros()).ok())
}

const fn quota_error_for_service(error: ResetCardServiceError) -> AccountQuotaObservationError {
	match error {
		ResetCardServiceError::AccountChanged
		| ResetCardServiceError::ExpectedRevisionMismatch { .. }
		| ResetCardServiceError::AccountNotFound => AccountQuotaObservationError::AccountMismatch,
		ResetCardServiceError::SchemaUnsupported
		| ResetCardServiceError::InventoryIncomplete
		| ResetCardServiceError::InventoryChanged => AccountQuotaObservationError::ProtocolUnavailable,
		_ => AccountQuotaObservationError::ProviderUnavailable,
	}
}

fn map_account_service_error(error: AccountLifecycleError) -> ResetCardServiceError {
	match error {
		AccountLifecycleError::AccountMissing => ResetCardServiceError::AccountNotFound,
		AccountLifecycleError::CredentialAbsent
		| AccountLifecycleError::CredentialStore(_)
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::CredentialAbsent)
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::StoreUnavailable)
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::StoreMismatch) =>
			ResetCardServiceError::VaultUnavailable,
		AccountLifecycleError::ProviderMismatch
		| AccountLifecycleError::NotReady(AccountLifecycleReadiness::ProviderMismatch) =>
			ResetCardServiceError::AccountChanged,
		AccountLifecycleError::StaleAccount => ResetCardServiceError::AccountChanged,
		AccountLifecycleError::AccountDisabled
		| AccountLifecycleError::NotReady(_)
		| AccountLifecycleError::OperationRejected(_) => ResetCardServiceError::AccountStateRejected,
		AccountLifecycleError::Refresh(_) => ResetCardServiceError::ProviderUnavailable,
		AccountLifecycleError::CredentialImport => ResetCardServiceError::InvalidRequest,
		AccountLifecycleError::InvalidOperation => ResetCardServiceError::InvalidRequest,
		AccountLifecycleError::Persistence(_) | AccountLifecycleError::CoordinatorUnavailable =>
			ResetCardServiceError::ProductStateUnavailable,
	}
}

pub(crate) const fn reset_card_account_admitted(account: &AccountRecord) -> bool {
	account.enabled
		&& !account.tombstoned
		&& account.unsettled_operation.is_none()
		&& matches!(account.lifecycle_readiness, AccountLifecycleReadiness::Ready)
		&& matches!(account.observed_state, AccountState::Available | AccountState::Depleted)
}

async fn fail_before_effect(
	inner: &ResetCardRuntimeInner,
	claim: &ResetCardClaim,
	failure: ResetCardFailureCode,
) {
	if !claim.requires_reconciliation {
		let _ = inner.store.fail_reset_card_before_effect(claim, &inner.worker_id, failure).await;
	}
}

fn provider_idempotency_key(value: &str) -> Result<ResetCardIdempotencyKey, ResetCardFailureCode> {
	ResetCardIdempotencyKey::new(value.to_owned())
		.map_err(|_| ResetCardFailureCode::ProviderUnavailable)
}

fn failure_code(error: ResetCardServiceError) -> ResetCardFailureCode {
	match error {
		ResetCardServiceError::AccountChanged
		| ResetCardServiceError::ExpectedRevisionMismatch { .. }
		| ResetCardServiceError::AccountNotFound
		| ResetCardServiceError::AccountStateRejected => ResetCardFailureCode::AccountChanged,
		ResetCardServiceError::VaultUnavailable => ResetCardFailureCode::VaultUnavailable,
		ResetCardServiceError::SchemaUnsupported => ResetCardFailureCode::SchemaUnsupported,
		ResetCardServiceError::InventoryIncomplete => ResetCardFailureCode::InventoryIncomplete,
		ResetCardServiceError::InventoryChanged => ResetCardFailureCode::InventoryChanged,
		ResetCardServiceError::ResourceExhausted => ResetCardFailureCode::ResourceExhausted,
		_ => ResetCardFailureCode::ProviderUnavailable,
	}
}

fn map_process_error(error: ResetCardProcessError) -> ResetCardServiceError {
	match error {
		ResetCardProcessError::SchemaInvalid | ResetCardProcessError::SchemaUnsupported(_) =>
			ResetCardServiceError::SchemaUnsupported,
		ResetCardProcessError::CredentialVault(_) => ResetCardServiceError::VaultUnavailable,
		ResetCardProcessError::AccountBindingChanged => ResetCardServiceError::AccountChanged,
		ResetCardProcessError::InvalidProviderResponse =>
			ResetCardServiceError::InventoryIncomplete,
		ResetCardProcessError::MethodUnavailable(_)
		| ResetCardProcessError::ProcessUnavailable
		| ResetCardProcessError::ShutdownFailed => ResetCardServiceError::ProviderUnavailable,
	}
}

fn map_process_supervisor_error(error: ProcessSupervisorError) -> ResetCardServiceError {
	match error {
		ProcessSupervisorError::ProductState | ProcessSupervisorError::AuthorityConflict =>
			ResetCardServiceError::ProductStateUnavailable,
		ProcessSupervisorError::Platform
		| ProcessSupervisorError::Identity
		| ProcessSupervisorError::SpawnFailed
		| ProcessSupervisorError::IdentityBindingFailed
		| ProcessSupervisorError::ControlChannelUnavailable
		| ProcessSupervisorError::InvalidTerminationWait => ResetCardServiceError::ProviderUnavailable,
	}
}

fn map_prepare_store_error(error: StoreError) -> ResetCardServiceError {
	match error {
		StoreError::IdempotencyConflict => ResetCardServiceError::IdempotencyConflict,
		StoreError::ResetCardCommitOutcomeUnknown => ResetCardServiceError::AcceptanceUnknown,
		StoreError::OwnershipLost(_) => ResetCardServiceError::AcceptanceUnknown,
		StoreError::RevisionConflict { actual: Some(actual), .. } =>
			ResetCardServiceError::ExpectedRevisionMismatch { actual },
		StoreError::RevisionConflict { .. } => ResetCardServiceError::AccountChanged,
		StoreError::InvalidInput("account state rejects manual reset-card use") =>
			ResetCardServiceError::AccountStateRejected,
		StoreError::InvalidInput(_) => ResetCardServiceError::InvalidRequest,
		StoreError::CapacityExhausted(_) => ResetCardServiceError::ResourceExhausted,
		_ => ResetCardServiceError::ProductStateUnavailable,
	}
}

const fn outcome_text(outcome: ResetCardConsumeOutcome) -> &'static str {
	match outcome {
		ResetCardConsumeOutcome::Reset => "reset",
		ResetCardConsumeOutcome::NothingToReset => "nothing_to_reset",
		ResetCardConsumeOutcome::NoCredit => "no_credit",
		ResetCardConsumeOutcome::AlreadyRedeemed => "already_redeemed",
	}
}

struct AccountServiceRefreshCallback {
	accounts: Arc<AccountService>,
	runtime: tokio::runtime::Handle,
	generation_id: ProcessGenerationId,
}
impl AccountServiceRefreshCallback {
	fn new(
		accounts: Arc<AccountService>,
		runtime: tokio::runtime::Handle,
		generation_id: ProcessGenerationId,
	) -> Self {
		Self { accounts, runtime, generation_id }
	}
}
impl AccountRefreshCallback for AccountServiceRefreshCallback {
	fn refresh(
		&self,
		account_id: &AccountId,
		initial_binding: &ProcessGenerationAccountBinding,
		reason: &str,
		previous_provider_account_id: Option<&str>,
	) -> Result<ChatgptRefreshProjection, CredentialVaultError> {
		if reason != "unauthorized" {
			return Err(CredentialVaultError::ProjectionRejected);
		}
		let operation_id =
			AccountOperationId::generate().map_err(|_| CredentialVaultError::Unavailable)?;
		let projection = self
			.runtime
			.block_on(self.accounts.refresh(
				operation_id,
				account_id,
				None,
				Some((&self.generation_id, initial_binding)),
				previous_provider_account_id,
			))
			.map_err(|_| CredentialVaultError::Unavailable)?;
		ChatgptRefreshProjection::new(
			projection.access_token().to_owned(),
			projection.provider_account_id().to_owned(),
			projection.plan_type().map(str::to_owned),
		)
	}
}
impl Debug for AccountServiceRefreshCallback {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("AccountServiceRefreshCallback")
	}
}

struct StoredCredentialVault {
	account_id: AccountId,
	stored: StoredCredential,
	initial_projection: InitialCredentialProjection,
}
impl StoredCredentialVault {
	fn new(
		account_id: AccountId,
		stored: StoredCredential,
		initial_projection: InitialCredentialProjection,
	) -> Self {
		Self { account_id, stored, initial_projection }
	}

	fn initialize(&self, child: &mut AttestedProcessChild) -> Result<(), ResetCardProcessError> {
		match self.initial_projection {
			InitialCredentialProjection::Stored => child.initialize_reset_card(self),
			InitialCredentialProjection::CallbackProbe => child.initialize_callback_probe(self),
		}
	}
}
impl CredentialVault for StoredCredentialVault {
	fn project(
		&self,
		account_id: &AccountId,
		projection: &mut CredentialProjection<'_>,
	) -> Result<AccountIdentity, CredentialVaultError> {
		if account_id != &self.account_id {
			return Err(CredentialVaultError::Unavailable);
		}
		let binding = self.stored.binding();
		let bundle = self.stored.bundle();
		match self.initial_projection {
			InitialCredentialProjection::Stored => projection.authenticate_chatgpt(
				bundle.access_token(),
				binding.provider.account_id(),
				bundle.plan_type(),
			)?,
			InitialCredentialProjection::CallbackProbe =>
				projection.authenticate_callback_probe(binding.provider.account_id())?,
		}

		Ok(AccountIdentity::from_observation("chatgpt", Some(bundle.provider_email()), true))
	}
}
impl Debug for StoredCredentialVault {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("StoredCredentialVault")
			.field("account_id", &self.account_id)
			.field("stored", &"[REDACTED]")
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use std::{
		future::{pending, ready},
		sync::{
			Arc,
			atomic::{AtomicBool, Ordering},
			mpsc,
		},
		time::Duration,
	};

	use decodex_postgres::StoreError;
	use tokio::{
		runtime::Builder,
		sync::{Notify, oneshot},
		task, time,
	};

	use super::{
		CLAIM_HEARTBEAT_INTERVAL, CLAIM_LEASE, ClaimWorkGate, MAX_BLOCKING_PROCESS_DEADLINE,
		MAX_CLAIM_WORK_START_DELAY, ProviderWorkLifecycle, ResetCardConsumeOutcome,
		ResetCardFailureCode, ResetCardServiceError, await_inventory_owner, finish_guarded_work,
		maintain_claim_heartbeat, map_prepare_store_error, provider_idempotency_key,
		readback_confirms_outcome, require_claim_work_start,
	};

	#[test]
	fn queued_and_detached_process_deadlines_are_strictly_inside_the_initial_lease() {
		assert_eq!(MAX_BLOCKING_PROCESS_DEADLINE, Duration::from_secs(242));
		assert_eq!(MAX_CLAIM_WORK_START_DELAY, Duration::from_secs(87));
		assert!(
			MAX_CLAIM_WORK_START_DELAY + MAX_BLOCKING_PROCESS_DEADLINE + CLAIM_HEARTBEAT_INTERVAL
				< CLAIM_LEASE
		);
	}

	#[tokio::test]
	async fn heartbeat_renews_and_stops_when_the_owner_finishes() {
		let renewed = Arc::new(Notify::new());
		let observed = Arc::clone(&renewed);
		let (stop_sender, stop_receiver) = oneshot::channel();
		let heartbeat = tokio::spawn(maintain_claim_heartbeat(
			move || {
				observed.notify_one();
				ready(Ok(()))
			},
			stop_receiver,
			Duration::from_millis(1),
		));

		time::timeout(Duration::from_secs(1), renewed.notified())
			.await
			.expect("heartbeat must renew before the lease interval can elapse");
		stop_sender.send(()).expect("heartbeat must still own its stop receiver");

		assert!(heartbeat.await.expect("heartbeat task must not panic").is_ok());
	}

	#[tokio::test]
	async fn dropped_owner_stops_heartbeats_for_expiry_recovery() {
		let renewed = Arc::new(Notify::new());
		let observed = Arc::clone(&renewed);
		let (stop_sender, stop_receiver) = oneshot::channel();
		let heartbeat = tokio::spawn(maintain_claim_heartbeat(
			move || {
				observed.notify_one();
				ready(Ok(()))
			},
			stop_receiver,
			Duration::from_millis(1),
		));

		time::timeout(Duration::from_secs(1), renewed.notified())
			.await
			.expect("heartbeat must have started");
		drop(stop_sender);

		assert!(
			time::timeout(Duration::from_secs(1), heartbeat)
				.await
				.expect("orphan heartbeat must stop")
				.expect("heartbeat task must not panic")
				.is_ok()
		);
	}

	#[tokio::test]
	async fn renewal_loss_fails_closed_before_guarded_work_can_finish() {
		let (stop_sender, stop_receiver) = oneshot::channel();
		let gate =
			ClaimWorkGate::with_deadline(std::time::Instant::now() + Duration::from_secs(60));
		let permit = gate.permit();
		let heartbeat = async move {
			let _stop_receiver = stop_receiver;

			Err(StoreError::OwnershipLost("reset-card claim"))
		};
		let result = finish_guarded_work(
			pending::<Result<(), ResetCardServiceError>>(),
			heartbeat,
			stop_sender,
			&gate,
		)
		.await;

		assert_eq!(result, Err(ResetCardServiceError::ProductStateUnavailable));
		assert!(!permit.permits_start());
	}

	#[tokio::test]
	async fn guarded_completion_stops_the_heartbeat_before_returning() {
		let (stop_sender, stop_receiver) = oneshot::channel();
		let heartbeat =
			maintain_claim_heartbeat(|| ready(Ok(())), stop_receiver, Duration::from_secs(60));
		let gate =
			ClaimWorkGate::with_deadline(std::time::Instant::now() + Duration::from_secs(60));
		let permit = gate.permit();
		let result = finish_guarded_work(
			ready(Ok::<_, ResetCardServiceError>(7)),
			heartbeat,
			stop_sender,
			&gate,
		)
		.await;

		assert_eq!(result, Ok(7));
		assert!(!permit.permits_start());
	}

	#[tokio::test]
	async fn queued_blocking_work_cannot_start_after_its_absolute_deadline() {
		let gate =
			ClaimWorkGate::with_deadline(std::time::Instant::now() + Duration::from_millis(1));
		let permit = gate.permit();

		time::sleep(Duration::from_millis(5)).await;
		let permitted = task::spawn_blocking(move || permit.permits_start())
			.await
			.expect("blocking start check must not panic");

		assert!(!permitted);
	}

	#[test]
	fn saturated_blocking_queue_cannot_start_after_heartbeat_loss() {
		let runtime = Builder::new_multi_thread()
			.worker_threads(1)
			.max_blocking_threads(1)
			.enable_all()
			.build()
			.expect("the test runtime must build");

		runtime.block_on(async {
			let (blocker_started_sender, blocker_started_receiver) = oneshot::channel();
			let (release_blocker_sender, release_blocker_receiver) = mpsc::channel();
			let blocker = task::spawn_blocking(move || {
				let _ = blocker_started_sender.send(());
				release_blocker_receiver.recv().expect("the saturated worker must be released");
			});
			blocker_started_receiver.await.expect("the blocking worker must start");

			let gate =
				ClaimWorkGate::with_deadline(std::time::Instant::now() + Duration::from_secs(60));
			let permit = gate.permit();
			let (start_observed_sender, start_observed_receiver) = oneshot::channel();
			let queued = task::spawn_blocking(move || {
				let _ = start_observed_sender.send(require_claim_work_start(Some(&permit)).is_ok());
			});
			let (stop_sender, stop_receiver) = oneshot::channel();
			let heartbeat = async move {
				let _stop_receiver = stop_receiver;

				Err(StoreError::OwnershipLost("reset-card claim"))
			};
			let result = finish_guarded_work(
				async move {
					queued.await.map_err(|_| ResetCardServiceError::ProductStateUnavailable)?;

					Ok(())
				},
				heartbeat,
				stop_sender,
				&gate,
			)
			.await;

			assert_eq!(result, Err(ResetCardServiceError::ProductStateUnavailable));
			release_blocker_sender.send(()).expect("the blocking worker must still exist");
			assert!(
				!start_observed_receiver
					.await
					.expect("the detached queued work must perform its start check")
			);
			blocker.await.expect("the saturated blocking worker must not panic");
		});
	}

	#[tokio::test]
	async fn closing_provider_lifecycle_rejects_new_work_and_waits_for_registered_work() {
		let lifecycle = Arc::new(ProviderWorkLifecycle::new());
		let permit = lifecycle.register().expect("open lifecycle must register provider work");

		assert!(permit.permits_start());
		lifecycle.close();
		assert!(!permit.permits_start());
		assert!(lifecycle.register().is_none());
		assert!(
			time::timeout(Duration::from_millis(10), lifecycle.wait_for_settlement())
				.await
				.is_err(),
			"registered provider work must hold lifecycle settlement",
		);

		drop(permit);
		time::timeout(Duration::from_secs(1), lifecycle.wait_for_settlement())
			.await
			.expect("provider lifecycle must settle after its last registered work exits");
	}

	#[tokio::test]
	async fn inventory_deadline_returns_typed_error_without_aborting_owned_cleanup() {
		let cleanup_finished = Arc::new(AtomicBool::new(false));
		let observed_cleanup = Arc::clone(&cleanup_finished);
		let mut owner = task::spawn(async move {
			time::sleep(Duration::from_millis(20)).await;
			observed_cleanup.store(true, Ordering::Release);

			Err(ResetCardServiceError::ProviderUnavailable)
		});

		assert_eq!(
			await_inventory_owner(&mut owner, Duration::from_millis(1)).await,
			Err(ResetCardServiceError::RequestTimedOut),
		);
		drop(owner);
		time::timeout(Duration::from_secs(1), async {
			while !cleanup_finished.load(Ordering::Acquire) {
				task::yield_now().await;
			}
		})
		.await
		.expect("daemon-owned cleanup must finish after the response deadline");
	}

	#[test]
	fn dropping_the_async_owner_closes_detached_blocking_work_permits() {
		let gate =
			ClaimWorkGate::with_deadline(std::time::Instant::now() + Duration::from_secs(60));
		let permit = gate.permit();

		assert!(permit.permits_start());
		drop(gate);
		assert!(!permit.permits_start());
	}

	#[test]
	fn invalid_provider_key_maps_to_a_terminal_before_effect_failure() {
		assert!(provider_idempotency_key("valid-retry-key").is_ok());
		assert!(matches!(
			provider_idempotency_key("invalid\nretry-key"),
			Err(ResetCardFailureCode::ProviderUnavailable)
		));
	}

	#[test]
	fn reconciliation_requires_outcome_specific_exact_credit_membership() {
		for outcome in [
			ResetCardConsumeOutcome::Reset,
			ResetCardConsumeOutcome::NoCredit,
			ResetCardConsumeOutcome::AlreadyRedeemed,
		] {
			for descriptor_expired in [false, true] {
				assert!(readback_confirms_outcome(outcome, false, descriptor_expired));
				assert!(!readback_confirms_outcome(outcome, true, descriptor_expired));
			}
		}
		assert!(readback_confirms_outcome(ResetCardConsumeOutcome::NothingToReset, true, false,));
		assert!(!readback_confirms_outcome(ResetCardConsumeOutcome::NothingToReset, false, false,));
		assert!(readback_confirms_outcome(ResetCardConsumeOutcome::NothingToReset, false, true,));
	}

	#[test]
	fn active_same_key_reservation_is_acceptance_unknown() {
		assert_eq!(
			map_prepare_store_error(StoreError::OwnershipLost("command receipt claim is active",)),
			ResetCardServiceError::AcceptanceUnknown,
		);
	}
}
