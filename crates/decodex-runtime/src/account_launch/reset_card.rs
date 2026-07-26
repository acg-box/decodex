//! Daemon-owned manual reset-card service.
//!
//! PostgreSQL owns account admission and the durable effect fence. The process adapter owns
//! credential projection and exact provider calls. Public callers can select only one configured
//! vNext account UUID and one public grant/expiry descriptor.

use std::{
	collections::BTreeMap,
	env,
	fmt::{Debug, Formatter},
	future::Future,
	path::PathBuf,
	sync::{
		Arc,
		atomic::{AtomicBool, AtomicUsize, Ordering},
	},
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use decodex_codex::{
	ExactResetCreditId, ResetCardIdempotencyKey, ResetCardInventory, ResetCardResolutionError,
};
use decodex_core::{
	AccountId, AccountState, RESET_CARD_PROVIDER_BINDING_METADATA_FIELD, ResetCardAccountConfig,
	ResetCardConsumeOutcome, ResetCardDescriptor, ServerHostConfig, ServerIdentity,
	admit_manual_reset_card_use,
};
use decodex_postgres::{
	AccountMetadata, AccountMutation, CommandIdentity, OutboxReconciliation, PostgresStore,
	ReconciliationOutcome, ResetCardClaim, ResetCardFailureCode, ResetCardOperationStatus,
	ResetCardPreparation, StoreError,
};
use serde_json::json;
use sha2::{Digest as _, Sha256};
use tokio::{
	sync::{Mutex, Notify, oneshot, watch},
	task, time,
};
use zeroize::Zeroizing;

use super::{
	CapacityExhausted, RunnerCapacity,
	process::{
		AccountBinding, AccountIdentity, AppServerCommand, CredentialProjection, CredentialVault,
		CredentialVaultError, ResetCardConsumeReadback, ResetCardProcessError,
		ResetCardProcessRunner,
	},
};

const PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
// The longest consume path has seven sequential process deadlines: combined executable/schema
// preflight; initialize, credential projection, and initial account read; then consume, inventory
// readback, and account re-attestation. Process-group and stdout-pump shutdown add at most 1.25
// seconds, rounded up to two seconds for this lease proof.
const MAX_BLOCKING_PROCESS_DEADLINE: Duration =
	Duration::from_secs(PROCESS_TIMEOUT.as_secs() * 7 + 2);
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
const MAX_ACCESS_TOKEN_BYTES: usize = 64 * 1_024;
const MAX_PROVIDER_ACCOUNT_ID_BYTES: usize = 1_024;
const MAX_EXPECTED_EMAIL_BYTES: usize = 320;
const ENROLLMENT_MARKER: &str = "decodex_reset_card_v1";
const PROVIDER_BINDING_FINGERPRINT_PROTOCOL: &[u8] = b"decodex/reset-card-provider-binding/1\0";

/// Non-secret account projection returned to the application boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResetCardAccountView {
	pub account_id: AccountId,
	pub display_label: String,
	pub state: AccountState,
	pub revision: i64,
}

/// Complete public inventory plus the exact account revision observed around process work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResetCardInventoryView {
	pub account_id: AccountId,
	pub account_revision: i64,
	pub cards: Vec<ResetCardDescriptor>,
}

/// Whether the configured daemon host vault is usable for all enrolled accounts.
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
	accounts: BTreeMap<AccountId, ConfiguredAccount>,
	vault: Arc<EnvironmentCredentialVault>,
	capacity: Arc<RunnerCapacity>,
	working_directory: PathBuf,
	worker_id: String,
	worker_lock: Mutex<()>,
	worker_wakeup: Arc<Notify>,
	provider_work: Arc<ProviderWorkLifecycle>,
}

#[derive(Clone)]
struct ConfiguredAccount {
	display_label: String,
}

/// Owner-held cancellation and absolute start fence for blocking provider work.
///
/// Dropping the gate closes every permit. A blocking task that was queued after its async owner
/// disappeared therefore cannot start provider work later under an expired or reclaimed claim.
struct ClaimWorkGate {
	state: Arc<ClaimWorkState>,
}

struct ClaimWorkPermit {
	state: Arc<ClaimWorkState>,
}

struct ClaimWorkState {
	open: AtomicBool,
	start_deadline: Instant,
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
	/// Enroll configured vNext UUIDs and load process-scoped environment vault entries.
	///
	/// The server lifecycle starts and directly owns the durable worker after it takes ownership of
	/// this runtime. Missing credential values make only reset-card calls unavailable.
	pub(crate) async fn start(
		store: PostgresStore,
		host: &ServerHostConfig,
		working_directory: PathBuf,
	) -> Result<Self, ResetCardStartupError> {
		let vault = Arc::new(EnvironmentCredentialVault::load(host.reset_card_accounts())?);
		let capacity =
			RunnerCapacity::daemon().map_err(|_| ResetCardStartupError::CapacityUnavailable)?;
		let mut accounts = BTreeMap::new();

		for (account_id, config) in host.reset_card_accounts() {
			let binding_fingerprint = vault.binding_fingerprint(account_id);

			enroll_account(&store, account_id, config, binding_fingerprint.as_ref()).await?;
			accounts.insert(
				account_id.clone(),
				ConfiguredAccount { display_label: config.display_label().to_owned() },
			);
		}

		let worker_id = ServerIdentity::generate()
			.map_err(|_| ResetCardStartupError::WorkerIdentityUnavailable)?
			.as_str()
			.to_owned();
		let runtime = Self {
			inner: Arc::new(ResetCardRuntimeInner {
				store,
				accounts,
				vault,
				capacity,
				working_directory,
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
		if self.inner.accounts.is_empty() {
			ResetCardVaultStatus::NotConfigured
		} else if self.inner.vault.len() == self.inner.accounts.len() {
			ResetCardVaultStatus::Ready
		} else {
			ResetCardVaultStatus::Unavailable
		}
	}

	/// Return only explicitly configured accounts that currently admit manual reset-card use.
	pub(crate) async fn accounts(
		&self,
	) -> Result<Vec<ResetCardAccountView>, ResetCardServiceError> {
		let mut views = Vec::with_capacity(self.inner.accounts.len());

		for (account_id, configured) in &self.inner.accounts {
			let Some(account) = self
				.inner
				.store
				.account(account_id)
				.await
				.map_err(|_| ResetCardServiceError::ProductStateUnavailable)?
			else {
				return Err(ResetCardServiceError::ProductStateUnavailable);
			};

			if admit_manual_reset_card_use(account.state).is_ok() {
				views.push(ResetCardAccountView {
					account_id: account.account_id,
					display_label: configured.display_label.clone(),
					state: account.state,
					revision: account.revision,
				});
			}
		}

		Ok(views)
	}

	/// Read one fresh, strict provider inventory under a PostgreSQL revision fence.
	pub(crate) async fn inventory(
		&self,
		account_id: &AccountId,
	) -> Result<ResetCardInventoryView, ResetCardServiceError> {
		self.require_configured_vault(account_id)?;
		let account = self.admitted_account(account_id).await?;
		let inventory =
			run_inventory(Arc::clone(&self.inner), account_id.clone(), account.revision, None)
				.await?;

		if !self
			.inner
			.store
			.account_admits_reset_card_at_revision(account_id, account.revision)
			.await
			.map_err(|_| ResetCardServiceError::ProductStateUnavailable)?
		{
			return Err(ResetCardServiceError::AccountChanged);
		}

		Ok(ResetCardInventoryView {
			account_id: account_id.clone(),
			account_revision: account.revision,
			cards: inventory.available_cards().iter().map(|card| card.descriptor()).collect(),
		})
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
		if let Err(error) = self.require_configured_vault(account_id) {
			return self
				.replay_preparation_or(&command, account_id, expected_revision, descriptor, error)
				.await;
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
		let preparation = self
			.inner
			.store
			.prepare_reset_card_operation(&command, account_id, expected_revision, descriptor)
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
		self.inner
			.store
			.replay_reset_card_preparation(command, account_id, expected_revision, descriptor)
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

	fn require_configured_vault(
		&self,
		account_id: &AccountId,
	) -> Result<(), ResetCardServiceError> {
		if !self.inner.accounts.contains_key(account_id) {
			return Err(ResetCardServiceError::AccountNotFound);
		}
		if !self.inner.vault.contains(account_id) {
			return Err(ResetCardServiceError::VaultUnavailable);
		}

		Ok(())
	}

	async fn admitted_account(
		&self,
		account_id: &AccountId,
	) -> Result<AccountMetadata, ResetCardServiceError> {
		let account = self
			.inner
			.store
			.account(account_id)
			.await
			.map_err(|_| ResetCardServiceError::ProductStateUnavailable)?
			.ok_or(ResetCardServiceError::AccountNotFound)?;

		admit_manual_reset_card_use(account.state)
			.map_err(|_| ResetCardServiceError::AccountStateRejected)?;

		Ok(account)
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
	if !inner.accounts.contains_key(&claim.account_id) || !inner.vault.contains(&claim.account_id) {
		fail_before_effect(&inner, &claim, ResetCardFailureCode::VaultUnavailable).await;

		return;
	}
	if !claim.requires_reconciliation && !account_revision_admitted(&inner, &claim).await {
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
			let inventory = match run_with_claim_heartbeat(inner, claim, |permit| {
				run_inventory(
					Arc::clone(inner),
					claim.account_id.clone(),
					claim.account_revision,
					Some(permit),
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
				Err(ResetCardResolutionError::NotFound | ResetCardResolutionError::Ambiguous) => {
					fail_before_effect(inner, claim, ResetCardFailureCode::InventoryChanged).await;

					return None;
				},
			};
			if !account_revision_admitted(inner, claim).await {
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
	let readback = match run_with_claim_heartbeat(inner, claim, |permit| {
		run_consume(
			Arc::clone(inner),
			claim.account_id.clone(),
			claim.account_revision,
			exact_credit_id,
			idempotency_key,
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
	let inventory = match run_with_claim_heartbeat(inner, claim, |permit| {
		run_inventory(
			Arc::clone(inner),
			claim.account_id.clone(),
			claim.account_revision,
			Some(permit),
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
	account_revision: i64,
	claim_permit: Option<ClaimWorkPermit>,
) -> Result<ResetCardInventory, ResetCardServiceError> {
	let provider_permit =
		inner.provider_work.register().ok_or(ResetCardServiceError::ProductStateUnavailable)?;

	task::spawn_blocking(move || {
		require_provider_work_start(&provider_permit, claim_permit.as_ref())?;
		let command = AppServerCommand::new(inner.working_directory.clone())
			.map_err(|_| ResetCardServiceError::ProviderUnavailable)?;
		let binding = AccountBinding::shared_home(account_id.clone())
			.map_err(|_| ResetCardServiceError::ProviderUnavailable)?;
		let permit = inner
			.capacity
			.reserve(account_id, account_revision)
			.map_err(|_: CapacityExhausted| ResetCardServiceError::ResourceExhausted)?;
		let runner = ResetCardProcessRunner::new(command, binding, PROCESS_TIMEOUT);

		require_provider_work_start(&provider_permit, claim_permit.as_ref())?;
		runner.read_inventory(inner.vault.as_ref(), permit).map_err(map_process_error)
	})
	.await
	.map_err(|_| ResetCardServiceError::ResourceExhausted)?
}

async fn run_consume(
	inner: Arc<ResetCardRuntimeInner>,
	account_id: AccountId,
	account_revision: i64,
	exact_credit_id: ExactResetCreditId,
	idempotency_key: ResetCardIdempotencyKey,
	claim_permit: ClaimWorkPermit,
) -> Result<ResetCardConsumeReadback, ResetCardServiceError> {
	let provider_permit =
		inner.provider_work.register().ok_or(ResetCardServiceError::ProductStateUnavailable)?;

	task::spawn_blocking(move || {
		require_provider_work_start(&provider_permit, Some(&claim_permit))?;
		let command = AppServerCommand::new(inner.working_directory.clone())
			.map_err(|_| ResetCardServiceError::ProviderUnavailable)?;
		let binding = AccountBinding::shared_home(account_id.clone())
			.map_err(|_| ResetCardServiceError::ProviderUnavailable)?;
		let permit = inner
			.capacity
			.reserve(account_id, account_revision)
			.map_err(|_: CapacityExhausted| ResetCardServiceError::ResourceExhausted)?;
		let runner = ResetCardProcessRunner::new(command, binding, PROCESS_TIMEOUT);

		require_provider_work_start(&provider_permit, Some(&claim_permit))?;
		runner
			.consume_and_readback(inner.vault.as_ref(), permit, exact_credit_id, idempotency_key)
			.map_err(map_process_error)
	})
	.await
	.map_err(|_| ResetCardServiceError::ResourceExhausted)?
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

async fn account_revision_admitted(inner: &ResetCardRuntimeInner, claim: &ResetCardClaim) -> bool {
	inner
		.store
		.account_admits_reset_card_at_revision(&claim.account_id, claim.account_revision)
		.await
		.unwrap_or(false)
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

fn map_prepare_store_error(error: StoreError) -> ResetCardServiceError {
	match error {
		StoreError::IdempotencyConflict => ResetCardServiceError::IdempotencyConflict,
		StoreError::ResetCardCommitOutcomeUnknown => ResetCardServiceError::AcceptanceUnknown,
		StoreError::OwnershipLost(_) => ResetCardServiceError::AcceptanceUnknown,
		StoreError::RevisionConflict { actual: Some(actual), .. } =>
			ResetCardServiceError::ExpectedRevisionMismatch { actual },
		StoreError::RevisionConflict { .. } => ResetCardServiceError::AccountChanged,
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

async fn enroll_account(
	store: &PostgresStore,
	account_id: &AccountId,
	config: &ResetCardAccountConfig,
	binding_fingerprint: Option<&ProviderBindingFingerprint>,
) -> Result<(), ResetCardStartupError> {
	match store
		.account(account_id)
		.await
		.map_err(|_| ResetCardStartupError::AccountEnrollmentUnavailable)?
	{
		Some(existing) => match existing_enrollment_decision(
			&existing,
			config.display_label(),
			binding_fingerprint,
		)? {
			ExistingEnrollmentDecision::Ready => Ok(()),
			ExistingEnrollmentDecision::InitializeProviderBinding => {
				if store
					.reset_card_account_has_unsettled_operations(account_id)
					.await
					.map_err(|_| ResetCardStartupError::AccountEnrollmentUnavailable)?
				{
					return Err(ResetCardStartupError::AccountIdentityConflict);
				}
				let binding_fingerprint =
					binding_fingerprint.expect("initialization requires a current fingerprint");
				let mut metadata = existing.metadata;
				metadata
					.as_object_mut()
					.expect("validated enrollment metadata must be an object")
					.insert(
						RESET_CARD_PROVIDER_BINDING_METADATA_FIELD.into(),
						json!(binding_fingerprint.as_str()),
					);
				let request = format!(
					"decodex/reset-card-provider-binding/1\n{}\n{}",
					account_id.as_str(),
					binding_fingerprint.as_str(),
				);
				let command = CommandIdentity::new(
					format!("reset-card-provider-bind-v1-{}", account_id.as_str()),
					request.as_bytes(),
				)
				.map_err(|_| ResetCardStartupError::AccountEnrollmentUnavailable)?;

				store
					.mutate_account(
						&command,
						&AccountMutation {
							account_id: account_id.clone(),
							display_label: existing.display_label,
							state: existing.state,
							metadata,
							expected_revision: Some(existing.revision),
						},
					)
					.await
					.map(|_| ())
					.map_err(|error| match error {
						StoreError::RevisionConflict { .. } | StoreError::IdempotencyConflict =>
							ResetCardStartupError::AccountEnrollmentConflict,
						_ => ResetCardStartupError::AccountEnrollmentUnavailable,
					})
			},
		},
		None => {
			let mut metadata = json!({"manual_operation": ENROLLMENT_MARKER});
			if let Some(binding_fingerprint) = binding_fingerprint {
				metadata.as_object_mut().expect("new enrollment metadata is an object").insert(
					RESET_CARD_PROVIDER_BINDING_METADATA_FIELD.into(),
					json!(binding_fingerprint.as_str()),
				);
			}
			let request = format!(
				"decodex/reset-card-enrollment/1\n{}\n{}\n{}\n{}",
				account_id.as_str(),
				config.display_label(),
				state_text(config.initial_state()),
				binding_fingerprint.map_or("", ProviderBindingFingerprint::as_str),
			);
			let command = CommandIdentity::new(
				format!("reset-card-enroll-v1-{}", account_id.as_str()),
				request.as_bytes(),
			)
			.map_err(|_| ResetCardStartupError::AccountEnrollmentUnavailable)?;
			store
				.mutate_account(
					&command,
					&AccountMutation {
						account_id: account_id.clone(),
						display_label: config.display_label().to_owned(),
						state: config.initial_state(),
						metadata,
						expected_revision: None,
					},
				)
				.await
				.map(|_| ())
				.map_err(|error| match error {
					StoreError::RevisionConflict { .. } =>
						ResetCardStartupError::AccountEnrollmentConflict,
					_ => ResetCardStartupError::AccountEnrollmentUnavailable,
				})
		},
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExistingEnrollmentDecision {
	Ready,
	InitializeProviderBinding,
}

fn existing_enrollment_decision(
	existing: &AccountMetadata,
	expected_display_label: &str,
	current: Option<&ProviderBindingFingerprint>,
) -> Result<ExistingEnrollmentDecision, ResetCardStartupError> {
	if existing.display_label != expected_display_label
		|| existing.metadata.get("manual_operation").and_then(serde_json::Value::as_str)
			!= Some(ENROLLMENT_MARKER)
	{
		return Err(ResetCardStartupError::AccountEnrollmentConflict);
	}
	let stored = existing
		.metadata
		.get(RESET_CARD_PROVIDER_BINDING_METADATA_FIELD)
		.map(|value| {
			value
				.as_str()
				.filter(|value| valid_provider_binding_fingerprint(value))
				.ok_or(ResetCardStartupError::AccountIdentityConflict)
		})
		.transpose()?;

	match (stored, current) {
		(Some(stored), Some(current)) if stored == current.as_str() =>
			Ok(ExistingEnrollmentDecision::Ready),
		(Some(_), Some(_)) => Err(ResetCardStartupError::AccountIdentityConflict),
		(Some(_), None) | (None, None) => Ok(ExistingEnrollmentDecision::Ready),
		(None, Some(_)) => Ok(ExistingEnrollmentDecision::InitializeProviderBinding),
	}
}

const fn state_text(state: AccountState) -> &'static str {
	match state {
		AccountState::Available => "available",
		AccountState::Depleted => "depleted",
		AccountState::Unavailable => "unavailable",
		AccountState::Unknown => "unknown",
		AccountState::AuthFailed => "auth_failed",
		AccountState::PluginUnready => "plugin_unready",
		AccountState::Disabled => "disabled",
	}
}

struct EnvironmentCredentialVault {
	entries: BTreeMap<AccountId, VaultEntry>,
}
impl EnvironmentCredentialVault {
	fn load(
		configured: &BTreeMap<AccountId, ResetCardAccountConfig>,
	) -> Result<Self, ResetCardStartupError> {
		let entries = configured
			.iter()
			.filter_map(|(account_id, config)| {
				VaultEntry::load(config).map(|entry| (account_id.clone(), entry))
			})
			.collect();

		Self::from_entries(entries)
	}

	fn from_entries(
		entries: BTreeMap<AccountId, VaultEntry>,
	) -> Result<Self, ResetCardStartupError> {
		for (index, entry) in entries.values().enumerate() {
			if entries.values().take(index).any(|existing| {
				existing.provider_account_id == entry.provider_account_id
					|| existing.expected_email == entry.expected_email
			}) {
				return Err(ResetCardStartupError::AccountIdentityConflict);
			}
		}

		Ok(Self { entries })
	}

	fn contains(&self, account_id: &AccountId) -> bool {
		self.entries.contains_key(account_id)
	}

	fn len(&self) -> usize {
		self.entries.len()
	}

	fn binding_fingerprint(&self, account_id: &AccountId) -> Option<ProviderBindingFingerprint> {
		self.entries
			.get(account_id)
			.map(|entry| ProviderBindingFingerprint::for_entry(account_id, entry))
	}
}
impl CredentialVault for EnvironmentCredentialVault {
	fn project(
		&self,
		account_id: &AccountId,
		projection: &mut CredentialProjection<'_>,
	) -> Result<AccountIdentity, CredentialVaultError> {
		let entry = self.entries.get(account_id).ok_or(CredentialVaultError::Unavailable)?;

		projection.authenticate_chatgpt(
			entry.access_token.as_str(),
			entry.provider_account_id.as_str(),
			Some(entry.plan_type.as_str()),
		)?;

		Ok(AccountIdentity::from_observation("chatgpt", Some(entry.expected_email.as_str()), true))
	}
}
impl Debug for EnvironmentCredentialVault {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("EnvironmentCredentialVault")
			.field("entry_count", &self.entries.len())
			.finish()
	}
}

#[derive(Clone, Eq, PartialEq)]
struct ProviderBindingFingerprint(String);
impl ProviderBindingFingerprint {
	fn for_entry(account_id: &AccountId, entry: &VaultEntry) -> Self {
		let mut digest = Sha256::new();

		digest.update(PROVIDER_BINDING_FINGERPRINT_PROTOCOL);
		for component in [
			account_id.as_str(),
			"chatgpt",
			entry.provider_account_id.as_str(),
			entry.expected_email.as_str(),
			entry.plan_type.as_str(),
		] {
			let length = u64::try_from(component.len())
				.expect("bounded provider binding component length must fit u64");

			digest.update(length.to_be_bytes());
			digest.update(component.as_bytes());
		}

		Self(digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
	}

	fn as_str(&self) -> &str {
		self.0.as_str()
	}
}
impl Debug for ProviderBindingFingerprint {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("ProviderBindingFingerprint([REDACTED])")
	}
}

fn valid_provider_binding_fingerprint(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

struct VaultEntry {
	access_token: Zeroizing<String>,
	provider_account_id: Zeroizing<String>,
	expected_email: Zeroizing<String>,
	plan_type: Zeroizing<String>,
}
impl VaultEntry {
	fn load(config: &ResetCardAccountConfig) -> Option<Self> {
		let access_token = load_scalar(config.access_token_env_var(), MAX_ACCESS_TOKEN_BYTES)?;
		let provider_account_id =
			load_scalar(config.provider_account_id_env_var(), MAX_PROVIDER_ACCOUNT_ID_BYTES)?;
		let expected_email =
			load_scalar(config.expected_email_env_var(), MAX_EXPECTED_EMAIL_BYTES)?;

		Some(Self {
			access_token,
			provider_account_id,
			expected_email,
			plan_type: Zeroizing::new(config.plan_type().to_owned()),
		})
	}
}
impl Debug for VaultEntry {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("VaultEntry([REDACTED])")
	}
}

fn load_scalar(name: &str, maximum_bytes: usize) -> Option<Zeroizing<String>> {
	let value = Zeroizing::new(env::var(name).ok()?);

	(!value.is_empty()
		&& value.len() <= maximum_bytes
		&& value.trim() == value.as_str()
		&& !value.chars().any(char::is_control))
	.then_some(value)
}

#[cfg(test)]
mod tests {
	use std::{
		collections::BTreeMap,
		future::{pending, ready},
		sync::{Arc, mpsc},
		time::Duration,
	};

	use decodex_core::{AccountId, AccountState, RESET_CARD_PROVIDER_BINDING_METADATA_FIELD};
	use decodex_postgres::{AccountMetadata, StoreError};
	use tokio::{
		runtime::Builder,
		sync::{Notify, oneshot},
		task, time,
	};

	use super::{
		CLAIM_HEARTBEAT_INTERVAL, CLAIM_LEASE, ClaimWorkGate, EnvironmentCredentialVault,
		ExistingEnrollmentDecision, MAX_BLOCKING_PROCESS_DEADLINE, MAX_CLAIM_WORK_START_DELAY,
		ProviderBindingFingerprint, ProviderWorkLifecycle, ResetCardConsumeOutcome,
		ResetCardFailureCode, ResetCardServiceError, ResetCardStartupError, VaultEntry,
		existing_enrollment_decision, finish_guarded_work, maintain_claim_heartbeat,
		map_prepare_store_error, provider_idempotency_key, readback_confirms_outcome,
		require_claim_work_start,
	};
	use zeroize::Zeroizing;

	fn entry(provider_account_id: &str, expected_email: &str, plan_type: &str) -> VaultEntry {
		VaultEntry {
			access_token: Zeroizing::new(format!("token-{provider_account_id}")),
			provider_account_id: Zeroizing::new(provider_account_id.to_owned()),
			expected_email: Zeroizing::new(expected_email.to_owned()),
			plan_type: Zeroizing::new(plan_type.to_owned()),
		}
	}

	#[test]
	fn queued_and_detached_process_deadlines_are_strictly_inside_the_initial_lease() {
		assert_eq!(MAX_BLOCKING_PROCESS_DEADLINE, Duration::from_secs(212));
		assert_eq!(MAX_CLAIM_WORK_START_DELAY, Duration::from_secs(117));
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
	fn vault_rejects_two_configured_account_aliases_before_startup() {
		let first = AccountId::new("10000000-0000-4000-8000-000000000001").expect("valid account");
		let second = AccountId::new("10000000-0000-4000-8000-000000000002").expect("valid account");
		let duplicate_provider = BTreeMap::from([
			(first.clone(), entry("provider-alias", "first@example.test", "fixture")),
			(second.clone(), entry("provider-alias", "second@example.test", "fixture")),
		]);
		let duplicate_identity = BTreeMap::from([
			(first, entry("provider-a", "same@example.test", "fixture")),
			(second, entry("provider-b", "same@example.test", "fixture")),
		]);

		assert!(matches!(
			EnvironmentCredentialVault::from_entries(duplicate_provider),
			Err(ResetCardStartupError::AccountIdentityConflict)
		));
		assert!(matches!(
			EnvironmentCredentialVault::from_entries(duplicate_identity),
			Err(ResetCardStartupError::AccountIdentityConflict)
		));
	}

	#[test]
	fn restart_rejects_provider_identity_drift_under_the_same_uuid_and_revision() {
		let account_id =
			AccountId::new("10000000-0000-4000-8000-000000000001").expect("valid account");
		let original = ProviderBindingFingerprint::for_entry(
			&account_id,
			&entry("provider-a", "first@example.test", "team"),
		);
		let drifted = ProviderBindingFingerprint::for_entry(
			&account_id,
			&entry("provider-b", "second@example.test", "team"),
		);
		let existing = AccountMetadata {
			account_id,
			display_label: "Bound account".into(),
			state: AccountState::Available,
			metadata: serde_json::json!({
				"manual_operation": super::ENROLLMENT_MARKER,
				(RESET_CARD_PROVIDER_BINDING_METADATA_FIELD): original.as_str(),
			}),
			revision: 7,
		};

		assert_eq!(
			existing_enrollment_decision(&existing, "Bound account", Some(&original)),
			Ok(ExistingEnrollmentDecision::Ready),
		);
		assert_eq!(
			existing_enrollment_decision(&existing, "Bound account", Some(&drifted)),
			Err(ResetCardStartupError::AccountIdentityConflict),
		);
		assert_eq!(
			existing_enrollment_decision(&existing, "Bound account", None),
			Ok(ExistingEnrollmentDecision::Ready),
			"temporarily absent vault material must not erase the durable binding",
		);
	}

	#[test]
	fn provider_binding_fingerprint_covers_uuid_account_email_and_plan() {
		let first = AccountId::new("10000000-0000-4000-8000-000000000001").expect("valid account");
		let second = AccountId::new("10000000-0000-4000-8000-000000000002").expect("valid account");
		let baseline_entry = entry("provider-a", "first@example.test", "team");
		let baseline = ProviderBindingFingerprint::for_entry(&first, &baseline_entry);

		assert_eq!(baseline.as_str().len(), 64);
		assert_eq!(baseline, ProviderBindingFingerprint::for_entry(&first, &baseline_entry),);
		assert_ne!(baseline, ProviderBindingFingerprint::for_entry(&second, &baseline_entry),);
		assert_ne!(
			baseline,
			ProviderBindingFingerprint::for_entry(
				&first,
				&entry("provider-b", "first@example.test", "team"),
			),
		);
		assert_ne!(
			baseline,
			ProviderBindingFingerprint::for_entry(
				&first,
				&entry("provider-a", "second@example.test", "team"),
			),
		);
		assert_ne!(
			baseline,
			ProviderBindingFingerprint::for_entry(
				&first,
				&entry("provider-a", "first@example.test", "plus"),
			),
		);
	}

	#[test]
	fn active_same_key_reservation_is_acceptance_unknown() {
		assert_eq!(
			map_prepare_store_error(StoreError::OwnershipLost("command receipt claim is active",)),
			ResetCardServiceError::AcceptanceUnknown,
		);
	}
}
