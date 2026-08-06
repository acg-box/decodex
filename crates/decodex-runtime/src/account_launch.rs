//! Runtime-owned PostgreSQL authorization and bounded process-capacity composition.

mod api_reset_card;
#[cfg(target_os = "macos")] mod macos_attested_spawn;
pub(crate) mod process;
mod protocol;
mod reset_card_types;

pub(crate) use api_reset_card::ApiResetCardRuntime;
pub(crate) use process::{AttestedAppServerLaunch, AttestedAppServerProfile, AttestedProcessChild};
pub(crate) use reset_card_types::{
	ResetCardInventoryObservation, ResetCardInventoryView, ResetCardObservationFailure,
	ResetCardServiceError, ResetCardVaultStatus,
};

use std::{
	error::Error,
	fmt::{Debug, Display, Formatter},
	sync::{
		Arc, Mutex, OnceLock, PoisonError, Weak,
		atomic::{AtomicU16, Ordering},
	},
};

use crate::account_launch::process::{
	CredentialVault, ExactThreadReconciler, ExactThreadReconciliation,
	ExactThreadReconciliationFailure, ExactThreadReconciliationResult, ProbeError,
	QuarantineSlotLease, ReadOnlyProbe, ReadOnlyProbeResult,
};
use decodex_codex::CapabilityCache;
use decodex_core::AccountId;
use decodex_postgres::PostgresStore;

const MAX_RUNNER_CAPACITY: u16 = 64;

/// Dormant explicit account observation composition; it contains no selector or routing policy.
///
/// Product capacity cannot be constructed or reserved outside this private owner:
///
/// ```compile_fail
/// use decodex_runtime::RunnerCapacity;
///
/// let _ = RunnerCapacity::daemon();
/// ```
#[derive(Clone)]
struct ManualAccountLauncher {
	store: PostgresStore,
	capacity: Arc<RunnerCapacity>,
}
impl ManualAccountLauncher {
	/// Bind the dormant composition to an already-authorized PostgreSQL adapter.
	fn new(store: &PostgresStore) -> Result<Self, CapacityExhausted> {
		Ok(Self { store: store.clone(), capacity: RunnerCapacity::daemon()? })
	}

	#[cfg(test)]
	fn with_capacity(store: &PostgresStore, limit: u16) -> Self {
		Self {
			store: store.clone(),
			capacity: Arc::new(RunnerCapacity::try_with_limit(limit).unwrap()),
		}
	}

	/// Produce one post-cleanup observation for an explicitly selected account and revision.
	///
	/// PostgreSQL is observed once before any process can spawn and again after the process is
	/// confirmed dead or its guard is quarantined. No database client or transaction spans the
	/// process or vault call. The returned value is not live-runner authority.
	async fn run_bound(
		&self,
		request: ManualAccountLaunchRequest,
		vault: &dyn CredentialVault,
		cache: &mut CapabilityCache,
	) -> Result<ManualAccountLaunchResult, ManualAccountLaunchError> {
		let ManualAccountLaunchRequest { account_id, expected_revision, probe } = request;

		if probe.account_id() != &account_id {
			return Err(ManualAccountLaunchError::BindingMismatch);
		}
		if !self
			.store
			.account_is_ready_at_revision(&account_id, expected_revision)
			.await
			.map_err(|_| ManualAccountLaunchError::ProductStateUnavailable)?
		{
			return Err(ManualAccountLaunchError::ReadinessRejected);
		}

		let guard = self
			.capacity
			.reserve(account_id.clone(), expected_revision)
			.map_err(|_| ManualAccountLaunchError::CapacityExhausted)?;
		let observation = probe
			.run_mechanical_with_lifetime_guard(vault, cache, guard)
			.map_err(ManualAccountLaunchError::Probe)?;

		if observation.account_id != account_id {
			return Err(ManualAccountLaunchError::BindingMismatch);
		}
		if !self
			.store
			.account_is_ready_at_revision(&account_id, expected_revision)
			.await
			.map_err(|_| ManualAccountLaunchError::ProductStateUnavailable)?
		{
			return Err(ManualAccountLaunchError::ReadinessRejected);
		}

		Ok(ManualAccountLaunchResult { observation, account_revision: expected_revision })
	}

	/// Execute one exact reconciliation under an explicitly selected ready account.
	///
	/// This private composition performs no selection, persistence, turn dispatch, or restart
	/// replay. PostgreSQL authority is checked before process mechanics and again after bounded
	/// cleanup.
	async fn reconcile_bound(
		&self,
		request: ManualReconciliationRequest,
		vault: &dyn CredentialVault,
		cache: &mut CapabilityCache,
	) -> Result<ManualReconciliationResult, ManualAccountLaunchError> {
		let ManualReconciliationRequest { account_id, expected_revision, reconciler, operation } =
			request;

		if reconciler.account_id() != &account_id {
			return Err(ManualAccountLaunchError::BindingMismatch);
		}
		if !self
			.store
			.account_is_ready_at_revision(&account_id, expected_revision)
			.await
			.map_err(|_| ManualAccountLaunchError::ProductStateUnavailable)?
		{
			return Err(ManualAccountLaunchError::ReadinessRejected);
		}

		let guard = self
			.capacity
			.reserve(account_id.clone(), expected_revision)
			.map_err(|_| ManualAccountLaunchError::CapacityExhausted)?;
		let result = reconciler
			.run_mechanical_with_lifetime_guard(vault, cache, guard, operation)
			.map_err(ManualAccountLaunchError::Reconciliation)?;

		if !self
			.store
			.account_is_ready_at_revision(&account_id, expected_revision)
			.await
			.map_err(|_| ManualAccountLaunchError::ProductStateUnavailable)?
		{
			return Err(ManualAccountLaunchError::ReadinessRejected);
		}

		Ok(ManualReconciliationResult { result, account_id, account_revision: expected_revision })
	}

	#[cfg(test)]
	fn active_capacity(&self) -> u16 {
		self.capacity.active()
	}
}

/// Exact caller-provided identity and revision for one dormant manual observation.
struct ManualAccountLaunchRequest {
	account_id: AccountId,
	expected_revision: i64,
	probe: ReadOnlyProbe,
}
impl ManualAccountLaunchRequest {
	/// Construct an explicit request without selecting or discovering an account.
	fn new(
		account_id: AccountId,
		expected_revision: i64,
		probe: ReadOnlyProbe,
	) -> Result<Self, ManualAccountLaunchError> {
		if expected_revision < 1 {
			return Err(ManualAccountLaunchError::ReadinessRejected);
		}

		Ok(Self { account_id, expected_revision, probe })
	}
}

/// Exact caller-provided account authority and one closed reconciliation operation.
struct ManualReconciliationRequest {
	account_id: AccountId,
	expected_revision: i64,
	reconciler: ExactThreadReconciler,
	operation: ExactThreadReconciliation,
}
impl ManualReconciliationRequest {
	fn new(
		account_id: AccountId,
		expected_revision: i64,
		reconciler: ExactThreadReconciler,
		operation: ExactThreadReconciliation,
	) -> Result<Self, ManualAccountLaunchError> {
		if expected_revision < 1 {
			return Err(ManualAccountLaunchError::ReadinessRejected);
		}

		Ok(Self { account_id, expected_revision, reconciler, operation })
	}
}
/// Non-live observation produced only after exact final PostgreSQL revalidation and cleanup.
struct ManualAccountLaunchResult {
	observation: ReadOnlyProbeResult,
	account_revision: i64,
}
impl ManualAccountLaunchResult {
	/// Mechanical Codex observation validated against the authorized account.
	const fn observation(&self) -> &ReadOnlyProbeResult {
		&self.observation
	}

	/// Exact positive PostgreSQL readiness revision re-observed after cleanup.
	const fn account_revision(&self) -> i64 {
		self.account_revision
	}
}
impl Debug for ManualAccountLaunchResult {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ManualAccountLaunchResult")
			.field("account_id", &self.observation.account_id)
			.field("account_revision", &self.account_revision)
			.field("process_id", &self.observation.process_id)
			.finish_non_exhaustive()
	}
}

/// Non-live reconciliation result released only after exact final account revalidation.
struct ManualReconciliationResult {
	result: ExactThreadReconciliationResult,
	account_id: AccountId,
	account_revision: i64,
}
impl Debug for ManualReconciliationResult {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		let operation = match self.result {
			ExactThreadReconciliationResult::List(_) => "list",
			ExactThreadReconciliationResult::Read(_) => "read",
			ExactThreadReconciliationResult::Archive(_) => "archive",
		};

		formatter
			.debug_struct("ManualReconciliationResult")
			.field("account_id", &self.account_id)
			.field("account_revision", &self.account_revision)
			.field("operation", &operation)
			.finish_non_exhaustive()
	}
}
/// Closed manual launch failure without database, account, credential, or provider text.
#[derive(Debug)]
enum ManualAccountLaunchError {
	/// PostgreSQL could not provide product-state authority.
	ProductStateUnavailable,
	/// The exact account revision was stale, missing, or not ready.
	ReadinessRejected,
	/// The runtime-private daemon capacity is occupied, including quarantined cleanup.
	CapacityExhausted,
	/// Mechanical probe binding contradicted the PostgreSQL-authorized account.
	BindingMismatch,
	/// Bounded process mechanics failed.
	Probe(ProbeError),
	/// Exact reconciliation mechanics failed closed.
	Reconciliation(ExactThreadReconciliationFailure),
}
impl Display for ManualAccountLaunchError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Probe(error) => Display::fmt(error, formatter),
			Self::Reconciliation(error) => write!(formatter, "{error:?}"),
			_ => write!(formatter, "{self:?}"),
		}
	}
}
impl Error for ManualAccountLaunchError {}

pub(crate) struct RunnerCapacity {
	inner: Arc<CapacityInner>,
}
impl RunnerCapacity {
	pub(crate) fn daemon() -> Result<Arc<Self>, CapacityExhausted> {
		static DAEMON: OnceLock<Mutex<Weak<RunnerCapacity>>> = OnceLock::new();

		let mut daemon = DAEMON
			.get_or_init(|| Mutex::new(Weak::new()))
			.lock()
			.unwrap_or_else(PoisonError::into_inner);

		if let Some(capacity) = daemon.upgrade() {
			return Ok(capacity);
		}

		let capacity = Arc::new(Self::try_with_limit(MAX_RUNNER_CAPACITY)?);

		*daemon = Arc::downgrade(&capacity);

		Ok(capacity)
	}

	fn try_with_limit(limit: u16) -> Result<Self, CapacityExhausted> {
		assert!((1..=MAX_RUNNER_CAPACITY).contains(&limit));

		Ok(Self {
			inner: Arc::new(CapacityInner {
				limit,
				active: AtomicU16::new(0),
				quarantine: process::ProcessQuarantine::try_new().map_err(|_| CapacityExhausted)?,
			}),
		})
	}

	pub(crate) fn reserve(
		&self,
		account_id: AccountId,
		account_revision: i64,
	) -> Result<RunnerPermit, CapacityExhausted> {
		if account_revision < 1 {
			return Err(CapacityExhausted);
		}

		let mut active = self.inner.active.load(Ordering::Acquire);

		loop {
			if active >= self.inner.limit {
				return Err(CapacityExhausted);
			}

			match self.inner.active.compare_exchange_weak(
				active,
				active + 1,
				Ordering::AcqRel,
				Ordering::Acquire,
			) {
				Ok(_) => break,
				Err(observed) => active = observed,
			}
		}

		let Some(quarantine_slot) = self.inner.quarantine.reserve_slot() else {
			self.inner.active.fetch_sub(1, Ordering::AcqRel);

			return Err(CapacityExhausted);
		};

		Ok(RunnerPermit {
			capacity: Arc::clone(&self.inner),
			account_id,
			account_revision,
			quarantine: Arc::clone(&self.inner.quarantine),
			quarantine_slot,
		})
	}

	#[cfg(test)]
	fn active(&self) -> u16 {
		self.inner.active.load(Ordering::Acquire)
	}
}

struct CapacityInner {
	limit: u16,
	active: AtomicU16,
	quarantine: Arc<process::ProcessQuarantine>,
}

pub(crate) struct RunnerPermit {
	capacity: Arc<CapacityInner>,
	account_id: AccountId,
	account_revision: i64,
	quarantine: Arc<process::ProcessQuarantine>,
	quarantine_slot: QuarantineSlotLease,
}
impl RunnerPermit {
	fn quarantine(&mut self) -> (Arc<process::ProcessQuarantine>, usize) {
		self.quarantine_slot.mark_installed();

		(Arc::clone(&self.quarantine), self.quarantine_slot.index())
	}

	#[cfg(test)]
	fn use_quarantine_for_test(&mut self, quarantine: &Arc<process::ProcessQuarantine>) {
		self.quarantine_slot = quarantine.reserve_slot().expect("test quarantine has a free slot");
		self.quarantine = Arc::clone(quarantine);
	}
}
impl Debug for RunnerPermit {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("RunnerPermit")
			.field("account_id", &self.account_id)
			.field("account_revision", &self.account_revision)
			.finish_non_exhaustive()
	}
}
impl Drop for RunnerPermit {
	fn drop(&mut self) {
		let previous = self.capacity.active.fetch_sub(1, Ordering::AcqRel);

		debug_assert!(previous > 0, "owned runner capacity cannot underflow");
	}
}

#[derive(Debug)]
pub(crate) struct CapacityExhausted;

#[cfg(test)]
mod tests {
	use std::{ptr, sync::Arc};

	use crate::account_launch::{CapacityExhausted, RunnerCapacity};
	use decodex_core::AccountId;

	fn account(suffix: u8) -> AccountId {
		AccountId::new(format!("10000000-0000-4000-8000-{suffix:012x}")).unwrap()
	}

	#[test]
	fn one_private_counter_rejects_parallel_capacity() {
		let capacity = RunnerCapacity::try_with_limit(1).unwrap();
		let permit = capacity.reserve(account(1), 7).unwrap();

		assert_eq!(capacity.active(), 1);
		assert!(matches!(capacity.reserve(account(2), 8), Err(CapacityExhausted)));

		drop(permit);

		assert_eq!(capacity.active(), 0);
	}

	#[test]
	fn daemon_registry_reuses_only_the_live_private_process_authority() {
		let first = RunnerCapacity::daemon().unwrap();
		let second = RunnerCapacity::daemon().unwrap();
		let weak = Arc::downgrade(&first);

		assert!(ptr::eq(Arc::as_ptr(&first), Arc::as_ptr(&second)));

		drop(first);
		drop(second);

		assert!(weak.upgrade().is_none());
		assert_eq!(RunnerCapacity::daemon().unwrap().active(), 0);
	}

	#[test]
	fn capacity_and_cleanup_slots_share_one_hard_bound() {
		let capacity = RunnerCapacity::try_with_limit(64).unwrap();
		let permits = (0..64)
			.map(|index| capacity.reserve(account(index), i64::from(index) + 1).unwrap())
			.collect::<Vec<_>>();

		assert_eq!(capacity.active(), 64);
		assert!(matches!(capacity.reserve(account(65), 66), Err(CapacityExhausted)));

		drop(permits);

		assert_eq!(capacity.active(), 0);
	}

	#[test]
	fn restart_constructs_no_persisted_capacity_or_assignment() {
		let first_process = RunnerCapacity::try_with_limit(1).unwrap();
		let permit = first_process.reserve(account(1), 3).unwrap();

		assert_eq!(first_process.active(), 1);

		drop(permit);

		let restarted_process = RunnerCapacity::try_with_limit(1).unwrap();

		assert_eq!(restarted_process.active(), 0);
	}
}
#[cfg(test)]
#[cfg(all(unix, feature = "account-binding-fixtures"))]
mod postgres_composition_tests {
	use std::{
		fs,
		path::Path,
		sync::{
			Arc, Mutex,
			mpsc::{self, Receiver, SyncSender},
		},
		time::Duration,
	};

	use tempfile::TempDir;
	use tokio::{runtime::Handle, task, time};
	use tokio_postgres::Client;

	use crate::account_launch::{
		ManualAccountLaunchError, ManualAccountLaunchRequest, ManualAccountLauncher,
		process::{
			AccountBinding, AccountIdentity, AppServerCommand, CredentialProjection,
			CredentialVault, CredentialVaultError, ProbeError, ReadOnlyProbe, SupervisionError,
		},
	};
	use decodex_codex::CapabilityCache;
	use decodex_core::{AccountId, AccountState};
	use decodex_postgres::{AccountMetadata, PostgresStore};

	struct MatchingVault;
	impl CredentialVault for MatchingVault {
		fn project(
			&self,
			_account_id: &AccountId,
			projection: &mut CredentialProjection<'_>,
		) -> Result<AccountIdentity, CredentialVaultError> {
			projection.authenticate_chatgpt(
				"synthetic-nonsecret-sentinel",
				"synthetic-provider-sentinel",
				Some("synthetic-plan"),
			)?;

			Ok(AccountIdentity::from_observation("chatgpt", Some("private@example.test"), true))
		}
	}

	struct BlockingVault {
		entered: SyncSender<()>,
		release: Mutex<Receiver<()>>,
	}
	impl CredentialVault for BlockingVault {
		fn project(
			&self,
			account_id: &AccountId,
			projection: &mut CredentialProjection<'_>,
		) -> Result<AccountIdentity, CredentialVaultError> {
			self.entered.send(()).map_err(|_| CredentialVaultError::Unavailable)?;
			self.release
				.lock()
				.map_err(|_| CredentialVaultError::Unavailable)?
				.recv_timeout(Duration::from_secs(5))
				.map_err(|_| CredentialVaultError::Unavailable)?;

			MatchingVault.project(account_id, projection)
		}
	}

	fn request(
		account_id: &AccountId,
		revision: i64,
		temp: &TempDir,
		mode: &str,
		extra: Option<&Path>,
	) -> ManualAccountLaunchRequest {
		let codex_home = temp.path().join("home/.codex");

		fs::create_dir_all(&codex_home).unwrap();

		ManualAccountLaunchRequest::new(
			account_id.clone(),
			revision,
			ReadOnlyProbe::fixture(
				AppServerCommand::fixture(mode, temp.path(), extra),
				AccountBinding::fixture(account_id.clone(), codex_home),
				Duration::from_secs(2),
			),
		)
		.unwrap()
	}

	fn blocking_vault() -> (Arc<BlockingVault>, Receiver<()>, SyncSender<()>) {
		let (entered_sender, entered_receiver) = mpsc::sync_channel(1);
		let (release_sender, release_receiver) = mpsc::sync_channel(1);

		(
			Arc::new(BlockingVault {
				entered: entered_sender,
				release: Mutex::new(release_receiver),
			}),
			entered_receiver,
			release_sender,
		)
	}

	fn fixture_state(state: AccountState) -> &'static str {
		match state {
			AccountState::Unknown => "unknown",
			AccountState::Available => "available",
			AccountState::Depleted => "depleted",
			_ => panic!("the manual launcher fixture accepts only exercised account states"),
		}
	}

	async fn read_fixture_account(
		store: &PostgresStore,
		account_id: &AccountId,
		expected_revision: i64,
		state: AccountState,
	) -> AccountMetadata {
		let account = store.account(account_id).await.unwrap().expect("fixture account exists");

		assert_eq!(account.account_id, *account_id);
		assert_eq!(account.display_label, "Manual fixture");
		assert_eq!(account.state, state);
		assert_eq!(account.revision, expected_revision);
		assert_eq!(account.metadata, serde_json::json!({"observation": "synthetic_fixture"}));

		account
	}

	async fn create_fixture_account(
		owner: &mut Client,
		store: &PostgresStore,
		account_id: &AccountId,
	) -> AccountMetadata {
		let transaction = owner.transaction().await.unwrap();
		let locked: bool = transaction
			.query_one("SELECT decodex.lock_account_routing_universe_exact()", &[])
			.await
			.unwrap()
			.get(0);

		assert!(locked);
		assert_eq!(
			transaction
				.execute(
					"INSERT INTO decodex.accounts(\
					 account_id,display_label,state,metadata,enabled\
					 ) VALUES(\
					 $1::text::uuid,'Manual fixture','unknown',\
					 '{\"observation\":\"synthetic_fixture\"}'::jsonb,true)",
					&[&account_id.as_str()],
				)
				.await
				.unwrap(),
			1
		);
		assert_eq!(
			transaction
				.execute(
					"INSERT INTO decodex.account_routing_order(account_id,position) \
					 SELECT $1::text::uuid,pg_catalog.count(*)::integer \
					 FROM decodex.account_routing_order",
					&[&account_id.as_str()],
				)
				.await
				.unwrap(),
			1
		);
		assert_eq!(
			transaction
				.execute(
					"UPDATE decodex.account_routing_control SET revision=revision+1,\
					 updated_at=pg_catalog.clock_timestamp() WHERE singleton",
					&[],
				)
				.await
				.unwrap(),
			1
		);
		transaction.commit().await.unwrap();

		let account = read_fixture_account(store, account_id, 1, AccountState::Unknown).await;
		let (accounts, routing) = store.read_account_registry_snapshot(512).await.unwrap();
		let record = accounts
			.iter()
			.find(|candidate| candidate.account_id == *account_id)
			.expect("fixture account is visible");

		assert!(record.enabled);
		assert_eq!(record.revision, account.revision);
		assert!(routing.order.contains(account_id));

		account
	}

	async fn update_fixture_account(
		owner: &Client,
		store: &PostgresStore,
		account_id: &AccountId,
		expected_revision: i64,
		state: AccountState,
	) -> AccountMetadata {
		let row = owner
			.query_one(
				"UPDATE decodex.accounts SET state=$3::text::decodex.account_state,\
				 revision=revision+1,observed_at=pg_catalog.clock_timestamp(),\
				 updated_at=pg_catalog.clock_timestamp() \
				 WHERE account_id=$1::text::uuid AND revision=$2 \
				 RETURNING revision",
				&[&account_id.as_str(), &expected_revision, &fixture_state(state)],
			)
			.await
			.unwrap();
		let revision: i64 = row.get(0);

		assert_eq!(revision, expected_revision + 1);
		read_fixture_account(store, account_id, revision, state).await
	}

	async fn assert_readiness_rejections_and_success(
		owner: &mut Client,
		store: &PostgresStore,
		launcher: &ManualAccountLauncher,
		account_id: &AccountId,
	) -> AccountMetadata {
		let unknown = create_fixture_account(owner, store, account_id).await;
		let rejected_temp = TempDir::new().unwrap();
		let rejected_marker = rejected_temp.path().join("unexpected-spawn");
		let rejected = launcher
			.run_bound(
				request(
					account_id,
					unknown.revision,
					&rejected_temp,
					"mark-spawn",
					Some(&rejected_marker),
				),
				&MatchingVault,
				&mut CapabilityCache::default(),
			)
			.await;

		assert!(matches!(rejected, Err(ManualAccountLaunchError::ReadinessRejected)));
		assert!(!rejected_marker.exists());

		let available = update_fixture_account(
			owner,
			store,
			account_id,
			unknown.revision,
			AccountState::Available,
		)
		.await;
		let stale_temp = TempDir::new().unwrap();
		let stale_marker = stale_temp.path().join("unexpected-stale-spawn");
		let stale = launcher
			.run_bound(
				request(
					account_id,
					unknown.revision,
					&stale_temp,
					"mark-spawn",
					Some(&stale_marker),
				),
				&MatchingVault,
				&mut CapabilityCache::default(),
			)
			.await;

		assert!(matches!(stale, Err(ManualAccountLaunchError::ReadinessRejected)));
		assert!(!stale_marker.exists());

		let success_temp = TempDir::new().unwrap();
		let success = launcher
			.run_bound(
				request(account_id, available.revision, &success_temp, "normal", None),
				&MatchingVault,
				&mut CapabilityCache::default(),
			)
			.await
			.unwrap();

		assert_eq!(success.account_revision(), available.revision);
		assert_eq!(success.observation().account_id, *account_id);
		assert_eq!(launcher.active_capacity(), 0);

		available
	}

	async fn assert_blocking_vault_releases_postgres(
		owner: &Client,
		store: &PostgresStore,
		launcher: &ManualAccountLauncher,
		account_id: &AccountId,
		available: &AccountMetadata,
	) -> AccountMetadata {
		let blocked_temp = TempDir::new().unwrap();
		let (vault, entered, release) = blocking_vault();
		let blocked_launcher = launcher.clone();
		let blocked_account = account_id.clone();
		let available_revision = available.revision;
		let runtime = Handle::current();
		let blocked = task::spawn_blocking(move || {
			runtime.block_on(blocked_launcher.run_bound(
				request(&blocked_account, available_revision, &blocked_temp, "normal", None),
				vault.as_ref(),
				&mut CapabilityCache::default(),
			))
		});

		entered.recv_timeout(Duration::from_secs(3)).unwrap();

		assert_eq!(launcher.active_capacity(), 1);

		let depleted = time::timeout(
			Duration::from_secs(1),
			update_fixture_account(
				owner,
				store,
				account_id,
				available.revision,
				AccountState::Depleted,
			),
		)
		.await
		.expect("the one-connection pool is free while the vault is blocked");

		release.send(()).unwrap();

		assert!(matches!(
			time::timeout(Duration::from_secs(5), blocked)
				.await
				.expect("released blocking vault must finish bounded fixture work")
				.unwrap(),
			Err(ManualAccountLaunchError::ReadinessRejected)
		));
		assert_eq!(launcher.active_capacity(), 0);

		update_fixture_account(owner, store, account_id, depleted.revision, AccountState::Available)
			.await
	}

	async fn assert_capacity_and_mismatch(
		launcher: &ManualAccountLauncher,
		account_id: &AccountId,
		available: &AccountMetadata,
	) {
		let capacity_temp = TempDir::new().unwrap();
		let (vault, entered, release) = blocking_vault();
		let capacity_launcher = launcher.clone();
		let capacity_account = account_id.clone();
		let capacity_revision = available.revision;
		let runtime = Handle::current();
		let occupied = task::spawn_blocking(move || {
			runtime.block_on(capacity_launcher.run_bound(
				request(&capacity_account, capacity_revision, &capacity_temp, "normal", None),
				vault.as_ref(),
				&mut CapabilityCache::default(),
			))
		});

		entered.recv_timeout(Duration::from_secs(3)).unwrap();

		let capacity_rejected = launcher
			.run_bound(
				request(account_id, available.revision, &TempDir::new().unwrap(), "normal", None),
				&MatchingVault,
				&mut CapabilityCache::default(),
			)
			.await;

		assert!(matches!(capacity_rejected, Err(ManualAccountLaunchError::CapacityExhausted)));

		release.send(()).unwrap();

		assert!(
			time::timeout(Duration::from_secs(5), occupied)
				.await
				.expect("released capacity fixture must finish")
				.unwrap()
				.is_ok()
		);
		assert_eq!(launcher.active_capacity(), 0);

		let mismatch_temp = TempDir::new().unwrap();
		let mismatch = launcher
			.run_bound(
				request(account_id, available.revision, &mismatch_temp, "account-switch", None),
				&MatchingVault,
				&mut CapabilityCache::default(),
			)
			.await;

		assert!(matches!(
			mismatch,
			Err(ManualAccountLaunchError::Probe(ProbeError::Supervision(
				SupervisionError::AccountChanged
			)))
		));
		assert_eq!(launcher.active_capacity(), 0);
	}
}
