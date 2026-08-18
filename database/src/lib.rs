#![allow(missing_docs)] // Internal persistence DTOs are defined by the schema and owner APIs.
//! Bundled SQLite product-state authority for local Decodex.

mod account_lifecycle;
mod account_profiles;
mod accounts;
mod command;
mod continuations;
mod conversations;
mod credentials;
mod error;
mod migrations;
mod process_generations;
mod program_cycles;
mod provider_attempts;
mod quick_task_routing;
mod role_profiles;
mod runtime_sessions;
mod transfers;

pub use self::{
	account_lifecycle::{
		AccountAdministrationOutcome, AccountCommandKind, AccountCommandReceiptClaim,
		AccountCommandReceiptLease, AccountLifecycleMutation, AccountLifecycleMutationOutcome,
		AccountLifecycleRejection, AccountOperationPreparation, AccountStoreObservation,
		CodexAccountCapabilityAttestation, RoutingControlOutcome,
	},
	account_profiles::{
		AccountProfileDailyUsage, AccountProfileObservation, AccountProfileObservationOutcome,
		AccountProfileSnapshot,
	},
	accounts::AccountMetadata,
	command::CommandIdentity,
	continuations::{
		ContextPackRecord, ContinuationPlanEffect, PlanContinuation, PlanInitialThreadContinuation,
	},
	conversations::{
		AdmitInitialQuickTaskTurn, ArchiveLocalQuickTaskConversation,
		ArchiveLocalQuickTaskConversationOutcome, ArchiveQuickTaskConversation,
		ArchiveQuickTaskConversationOutcome, ArchivedQuickTaskConversation,
		CreateQuickTaskConversation, CreateQuickTaskRoutingSuccessor, HistoryCursor, HistoryEntry,
		HistoryPage, InitialQuickTaskTurnAdmissionOutcome, InitialQuickTaskTurnAdmissionReadback,
		InitialQuickTaskTurnAdmissionRejection, OrdinaryTaskConversationCursor,
		OrdinaryTaskConversationProjection, OrdinaryTaskConversationReadback,
		OrdinaryTaskPreSessionState, PendingQuickTaskTerminalizationReadback,
		QuickTaskAssistantPrefixReadback, QuickTaskRequest, QuickTaskRoutingSuccessor,
		QuickTaskRoutingSuccessorOutcome, QuickTaskTerminalizationOutcome,
		QuickTaskTerminalizationReadback, ReconcileStrandedQuickTaskTurn,
		ReconcileStrandedQuickTaskTurnOutcome, RecordHistoryItem, RecoverUnknownQuickTaskTurn,
		RecoverUnknownQuickTaskTurnOutcome, RecoveredUnknownQuickTaskTurn, StoredConversation,
		TerminalizeQuickTaskTurn, TurnReservationOutcome, TurnReservationReadback,
		UnknownQuickTaskAttemptReadback,
	},
	credentials::{CredentialKey, CredentialRecord},
	error::{BootstrapFailure, DatabaseError, StoreError},
	process_generations::{
		FreshProcessGenerationFence, PrepareProcessGenerationOutcome, ProcessGenerationMutation,
		ProcessGenerationMutationOutcome, ProcessGenerationRejection,
	},
	program_cycles::{
		BindProgramDomainPack, ContinueProgram, CreateProgramCycle, DomainPackIdentity,
		ProgramCharterRecord, ProgramClaimRecord, ProgramCycleRecord, ProgramDomainPackBinding,
		ProgramEvidenceInput, ProgramEvidenceRecord, ProgramObjectiveRecord, ProgramProposalRecord,
		ProgramReviewRecord, ProgramSignalRecord, ProgramSummaryRecord, ProgramWorkItemRecord,
		ProgramWorkItemDomainPack, RecordProgramReview,
	},
	provider_attempts::{
		AuthorizeProviderDispatchOutcome, FreshPreparedProviderAttempt, FreshProviderDispatchFence,
		PrepareProviderAttemptOutcome, ProviderAttemptMutation, ProviderAttemptMutationOutcome,
		ProviderAttemptRejection, RuntimeSessionBindingReceipt,
	},
	quick_task_routing::{
		BindQuickTaskContinuation, QuickTaskContinuationBinding, QuickTaskInitialRoute,
		QuickTaskInitialRouteOutcome, RouteQuickTaskInitial,
	},
	role_profiles::RoleProfileRole,
	runtime_sessions::{
		BindRuntimeSessionThread, BindRuntimeSessionThreadOutcome, FenceRuntimeSessionThreadStart,
		FenceRuntimeSessionThreadStartOutcome, FreshQuickTaskProcessGeneration,
		FreshRuntimeSessionThreadStart, OrdinaryRuntimeSessionResumeReadback,
		PrepareQuickTaskProcessGeneration, PrepareQuickTaskProcessGenerationOutcome,
		QuickTaskPreEffectEvidenceKind, QuickTaskProcessGenerationReadback,
		QuickTaskProcessGenerationRejection, QuickTaskThreadEstablishmentReadback,
		QuickTaskThreadStartNonEffect, ReconcileQuickTaskThreadEstablishment,
		RuntimeSessionAccountSnapshot, RuntimeSessionProfileSnapshot,
		RuntimeSessionThreadBindingReadback, RuntimeSessionThreadEstablishmentRejection,
		RuntimeSessionThreadFenceReadback, StoredRuntimeSession,
		SuccessfulRuntimeSessionThreadStart,
	},
	transfers::{
		LocalAccountTransfer, LocalAccountTransferBatch, LocalAccountTransferError,
		LocalAccountTransferOutcome,
	},
};

#[cfg(unix)] use std::os::unix::fs::MetadataExt as _;
use std::{
	fs::File,
	path::{Path, PathBuf},
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, Ordering},
	},
	time::{SystemTime, UNIX_EPOCH},
};

use decodex_core::{Availability, DecodexPaths, ProductState};
use rusqlite::{Connection, OpenFlags};

/// One concrete, daemon-owned local product store.
#[derive(Clone)]
pub struct SqliteStore {
	inner: Arc<StoreInner>,
}

struct StoreInner {
	connection: Mutex<Connection>,
	path: PathBuf,
	closed: AtomicBool,
	#[cfg(unix)]
	_identity_guard: File,
}

impl SqliteStore {
	/// Open or initialize the fixed owner-private Decodex product database.
	pub fn open(paths: &DecodexPaths) -> Result<Self, DatabaseError> {
		#[cfg(unix)]
		{
			let guard =
				paths.open_product_database_file().map_err(|_| DatabaseError::UnsafePath)?;
			Self::open_verified(paths.product_database_file(), guard)
		}

		#[cfg(not(unix))]
		{
			paths.ensure_layout().map_err(|_| DatabaseError::UnsafePath)?;
			Self::open_path(paths.product_database_file())
		}
	}

	#[cfg(unix)]
	fn open_verified(path: PathBuf, guard: File) -> Result<Self, DatabaseError> {
		let before = guard.metadata().map_err(|_| DatabaseError::UnsafePath)?;
		let sqlite_path = path.canonicalize().map_err(|_| DatabaseError::UnsafePath)?;
		let mut connection = open_connection(&sqlite_path)?;
		let after = path.metadata().map_err(|_| DatabaseError::UnsafePath)?;
		if before.dev() != after.dev() || before.ino() != after.ino() || after.nlink() != 1 {
			return Err(DatabaseError::UnsafePath);
		}
		migrations::configure(&connection)?;
		migrations::migrate(&mut connection)?;
		Ok(Self {
			inner: Arc::new(StoreInner {
				connection: Mutex::new(connection),
				path,
				closed: AtomicBool::new(false),
				_identity_guard: guard,
			}),
		})
	}

	#[cfg(not(unix))]
	fn open_path(path: PathBuf) -> Result<Self, DatabaseError> {
		let mut connection = open_connection(&path)?;
		migrations::configure(&connection)?;
		migrations::migrate(&mut connection)?;
		Ok(Self {
			inner: Arc::new(StoreInner {
				connection: Mutex::new(connection),
				path,
				closed: AtomicBool::new(false),
			}),
		})
	}

	/// Revalidate SQLite integrity, configuration, and the exact migration-owned schema.
	pub async fn revalidate(&self) -> Result<(), DatabaseError> {
		let store = self.clone();
		tokio::task::spawn_blocking(move || {
			store.with_connection(|connection| migrations::verify(connection))
		})
		.await
		.map_err(|_| DatabaseError::Unavailable)?
	}

	/// Close this store and every clone to new operations.
	pub fn close(&self) {
		self.inner.closed.store(true, Ordering::Release);
	}

	/// Fixed database path. This value is for diagnostics and never contains credentials.
	pub fn path(&self) -> &Path {
		&self.inner.path
	}

	pub(crate) fn with_connection<T>(
		&self,
		operation: impl FnOnce(&mut Connection) -> Result<T, DatabaseError>,
	) -> Result<T, DatabaseError> {
		if self.inner.closed.load(Ordering::Acquire) {
			return Err(DatabaseError::Closed);
		}
		let mut connection =
			self.inner.connection.lock().map_err(|_| DatabaseError::Unavailable)?;
		if self.inner.closed.load(Ordering::Acquire) {
			return Err(DatabaseError::Closed);
		}
		operation(&mut connection)
	}

	pub(crate) async fn run<T, F>(&self, operation: F) -> Result<T, StoreError>
	where
		T: Send + 'static,
		F: FnOnce(&mut Connection) -> Result<T, StoreError> + Send + 'static,
	{
		let inner = Arc::clone(&self.inner);
		tokio::task::spawn_blocking(move || {
			if inner.closed.load(Ordering::Acquire) {
				return Err(StoreError::from(DatabaseError::Closed));
			}
			let mut connection = inner.connection.lock().map_err(|_| DatabaseError::Unavailable)?;
			if inner.closed.load(Ordering::Acquire) {
				return Err(StoreError::from(DatabaseError::Closed));
			}
			operation(&mut connection)
		})
		.await
		.map_err(|_| StoreError::from(DatabaseError::Unavailable))?
	}

	#[cfg(test)]
	fn open_test(path: &Path) -> Result<Self, DatabaseError> {
		use std::fs::OpenOptions;
		#[cfg(unix)] use std::os::unix::fs::OpenOptionsExt as _;

		let mut options = OpenOptions::new();
		options.read(true).write(true).create(true);
		#[cfg(unix)]
		options.mode(0o600);
		let guard = options.open(path).map_err(|_| DatabaseError::UnsafePath)?;
		#[cfg(unix)]
		return Self::open_verified(path.to_path_buf(), guard);
		#[cfg(not(unix))]
		{
			drop(guard);
			Self::open_path(path.to_path_buf())
		}
	}
}

impl ProductState for SqliteStore {
	fn availability(&self) -> Availability {
		if self.inner.closed.load(Ordering::Acquire) {
			Availability::Unavailable { reason: "product database is closed" }
		} else {
			Availability::Available
		}
	}
}

fn open_connection(path: &Path) -> Result<Connection, DatabaseError> {
	let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
		| OpenFlags::SQLITE_OPEN_CREATE
		| OpenFlags::SQLITE_OPEN_NO_MUTEX
		| OpenFlags::SQLITE_OPEN_PRIVATE_CACHE
		| OpenFlags::SQLITE_OPEN_NOFOLLOW;
	Connection::open_with_flags(path, flags).map_err(error::sqlite_error)
}

pub(crate) fn unix_micros() -> Result<i64, DatabaseError> {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.ok()
		.and_then(|duration| i64::try_from(duration.as_micros()).ok())
		.filter(|value| *value > 0)
		.ok_or(DatabaseError::Unavailable)
}

#[cfg(test)]
mod tests {
	use std::fs;
	#[cfg(unix)] use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _, symlink};

	use rusqlite::Connection;
	use serde_json::json;
	use tempfile::tempdir;
	use zeroize::Zeroizing;

	use decodex_core::{
		AccountId, AccountLifecycleReadiness, AccountOperationId, AccountOperationKind,
		AccountOperationPhase, AccountProvider, AccountQuotaDisposition,
		AccountQuotaObservationError, AccountQuotaWindow, AccountSelectionMode, CredentialBinding,
		CredentialFingerprint, CredentialStoreSchemaVersion, CredentialVersion, ProviderIdentity,
	};

	use super::{
		AccountCommandKind, AccountCommandReceiptClaim, AccountLifecycleMutationOutcome,
		AccountOperationPreparation, CodexAccountCapabilityAttestation, CommandIdentity,
		CredentialKey, CredentialRecord, DatabaseError, SqliteStore, StoreError, migrations,
		unix_micros,
	};

	const ACCOUNT: &str = "10000000-0000-4000-8000-000000000001";
	const OPERATION_ONE: &str = "20000000-0000-4000-8000-000000000001";
	const OPERATION_TWO: &str = "20000000-0000-4000-8000-000000000002";
	const OPERATION_THREE: &str = "20000000-0000-4000-8000-000000000003";
	const DIGEST_ONE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
	const DIGEST_TWO: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

	fn fixture_key(version: u64, digest: &str, operation: &str) -> CredentialKey {
		CredentialKey {
			account_id: ACCOUNT.to_owned(),
			schema_version: 1,
			credential_version: version,
			fingerprint: digest.to_owned(),
			writer_operation_id: operation.to_owned(),
			provider: "chatgpt".to_owned(),
			provider_account_id: "provider-account".to_owned(),
		}
	}

	fn seed_operation(store: &SqliteStore, operation: &str) {
		store
			.with_connection(|connection| {
				connection
					.execute(
						"INSERT OR IGNORE INTO account_identities (account_id, created_at_micros)
						 VALUES (?1, 1)",
						rusqlite::params![ACCOUNT],
					)
					.map_err(super::error::sqlite_error)?;
				connection
					.execute(
						"INSERT INTO account_operations (
						   operation_id, account_id, kind, phase, provider, provider_account_id,
						   requested_display_label, requested_enabled, created_at_micros,
						   updated_at_micros
						 ) VALUES (?1, ?2, 'enroll', 'prepared', 'chatgpt',
						           'provider-account', 'Account', 1, 1, 1)",
						rusqlite::params![operation, ACCOUNT],
					)
					.map_err(super::error::sqlite_error)?;
				Ok(())
			})
			.expect("seed operation");
	}

	fn account_fixture() -> (AccountId, AccountOperationId, ProviderIdentity, CredentialBinding) {
		let account_id = AccountId::new(ACCOUNT).expect("account identity");
		let operation_id = AccountOperationId::new(OPERATION_ONE).expect("operation identity");
		let provider = ProviderIdentity::new(AccountProvider::Chatgpt, "provider-account")
			.expect("provider identity");
		let binding = CredentialBinding {
			schema_version: CredentialStoreSchemaVersion::V1,
			version: CredentialVersion::new(1).expect("credential version"),
			fingerprint: CredentialFingerprint::new(DIGEST_ONE).expect("credential fingerprint"),
			provider: provider.clone(),
			writer_operation_id: operation_id.clone(),
		};
		(account_id, operation_id, provider, binding)
	}

	#[test]
	fn initializes_and_reopens_exact_versioned_schema() {
		let directory = tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let store = SqliteStore::open_test(&path).expect("initialize store");
		let migrations: Vec<(i64, String)> = store
			.with_connection(|connection| {
				let mut statement = connection
					.prepare("SELECT version, sha256 FROM schema_migrations ORDER BY version")
					.map_err(super::error::sqlite_error)?;
				statement
					.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
					.map_err(super::error::sqlite_error)?
					.collect::<Result<Vec<_>, _>>()
					.map_err(super::error::sqlite_error)
			})
			.expect("read migration ledger");
		assert_eq!(
			migrations,
			migrations::expected_migration_digests()
				.into_iter()
				.enumerate()
				.map(|(index, digest)| ((index + 1) as i64, digest))
				.collect::<Vec<_>>()
		);
		drop(store);
		SqliteStore::open_test(&path).expect("reopen exact schema");
	}

	#[test]
	fn upgrades_v1_task_profile_to_the_executable_nonempty_contract() {
		let directory = tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let connection = Connection::open(&path).expect("open V1 fixture");
		migrations::configure(&connection).expect("configure V1 fixture");
		connection
			.execute_batch(include_str!("../migrations/0001_local_product.sql"))
			.expect("apply V1 fixture");
		connection
			.execute(
				"INSERT INTO schema_migrations (version, name, sha256, applied_at_micros)
				 VALUES (1, 'local_product', ?1, 1)",
				rusqlite::params![migrations::expected_migration_digests()[0]],
			)
			.expect("record V1 fixture");
		connection
			.pragma_update(None, "application_id", migrations::APPLICATION_ID)
			.expect("set V1 application identity");
		connection.pragma_update(None, "user_version", 1).expect("set V1 version");
		drop(connection);

		let store = SqliteStore::open_test(&path).expect("upgrade V1 fixture");
		let (revision, instructions, table_sql): (i64, String, String) = store
			.with_connection(|connection| {
				let (revision, instructions) = connection
					.query_row(
						"SELECT revision, instructions FROM role_profiles WHERE role = 'task'",
						[],
						|row| Ok((row.get(0)?, row.get(1)?)),
					)
					.map_err(super::error::sqlite_error)?;
				let table_sql = connection
					.query_row(
						"SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'role_profiles'",
						[],
						|row| row.get(0),
					)
					.map_err(super::error::sqlite_error)?;
				Ok((revision, instructions, table_sql))
			})
			.expect("read upgraded Task profile");
		assert_eq!(revision, 2);
		assert_eq!(instructions, "Follow the user request for this task.");
		assert!(table_sql.contains("BETWEEN 1 AND 65536"));
	}

	#[tokio::test]
	async fn upgrades_v7_refresh_ambiguity_without_settling_or_rewriting_it() {
		let directory = tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let connection = Connection::open(&path).expect("open V7 fixture");
		migrations::configure(&connection).expect("configure V7 fixture");
		let sources = [
			("local_product", include_str!("../migrations/0001_local_product.sql")),
			(
				"nonempty_task_instructions",
				include_str!("../migrations/0002_nonempty_task_instructions.sql"),
			),
			(
				"quick_task_execution_controls",
				include_str!("../migrations/0003_quick_task_execution_controls.sql"),
			),
			("context_pack_fallback", include_str!("../migrations/0004_context_pack_fallback.sql")),
			(
				"adaptive_factory_spine",
				include_str!("../migrations/0005_adaptive_factory_spine.sql"),
			),
			(
				"repeatable_program_loop",
				include_str!("../migrations/0006_repeatable_program_loop.sql"),
			),
			(
				"builtin_domain_pack_binding",
				include_str!("../migrations/0007_builtin_domain_pack_binding.sql"),
			),
		];
		let digests = migrations::expected_migration_digests();
		for (index, (name, source)) in sources.into_iter().enumerate() {
			connection.execute_batch(source).expect("apply V7 migration fixture");
			connection
				.execute(
					"INSERT INTO schema_migrations (version, name, sha256, applied_at_micros)
					 VALUES (?1, ?2, ?3, ?4)",
					rusqlite::params![(index + 1) as i64, name, digests[index], (index + 1) as i64],
				)
				.expect("record V7 migration fixture");
		}
		connection
			.pragma_update(None, "application_id", migrations::APPLICATION_ID)
			.expect("set application identity");
		connection.pragma_update(None, "user_version", 7).expect("set V7 version");
		connection
			.execute(
				"INSERT INTO account_identities (account_id, created_at_micros) VALUES (?1, 1)",
				rusqlite::params![ACCOUNT],
			)
			.expect("seed account identity");
		connection
			.execute(
				"INSERT INTO account_operations (
				   operation_id, account_id, kind, phase, expected_account_revision,
				   provider, provider_account_id, recovery_code, created_at_micros,
				   updated_at_micros
				 ) VALUES (?1, ?2, 'refresh', 'recovery_required', 1, 'chatgpt',
				           'provider-account', 'provider_refresh_ambiguous', 1, 1)",
				rusqlite::params![OPERATION_TWO, ACCOUNT],
			)
			.expect("seed refresh ambiguity");
		drop(connection);

		let store = SqliteStore::open_test(&path).expect("upgrade V7 fixture");
		let ambiguity = store
			.read_account_operation(
				&AccountOperationId::new(OPERATION_TWO).expect("operation identity"),
			)
			.await
			.expect("read upgraded ambiguity")
			.expect("ambiguity remains present");
		assert_eq!(ambiguity.phase, AccountOperationPhase::RecoveryRequired);
		assert_eq!(ambiguity.recovery_code.as_deref(), Some("provider_refresh_ambiguous"));
		assert!(ambiguity.recovery_operation_id.is_none());
		assert!(ambiguity.superseded_by_operation_id.is_none());
	}

	#[test]
	fn rejects_a_forged_migration_digest() {
		let directory = tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let store = SqliteStore::open_test(&path).expect("initialize store");
		store
			.with_connection(|connection| {
				connection
					.execute(
						"UPDATE schema_migrations SET sha256 = ?1 WHERE version = 1",
						rusqlite::params![DIGEST_ONE],
					)
					.map_err(super::error::sqlite_error)?;
				Ok(())
			})
			.expect("forge digest");
		drop(store);
		assert_eq!(SqliteStore::open_test(&path).err(), Some(DatabaseError::Incompatible));
	}

	#[test]
	fn credential_compare_and_swap_is_exact_and_debug_is_redacted() {
		let directory = tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let store = SqliteStore::open_test(&path).expect("initialize store");
		seed_operation(&store, OPERATION_ONE);
		let first = fixture_key(1, DIGEST_ONE, OPERATION_ONE);
		store
			.create_credential(CredentialRecord {
				key: first.clone(),
				payload: Zeroizing::new(b"secret-one".to_vec()),
			})
			.expect("create credential");
		let read = store.read_credential(ACCOUNT).expect("read credential");
		assert_eq!(read.key, first);
		assert!(!format!("{read:?}").contains("secret-one"));
		store
			.with_connection(|connection| {
				connection
					.execute(
						"UPDATE account_operations
						 SET phase = 'committed', updated_at_micros = 2, completed_at_micros = 2
						 WHERE operation_id = ?1",
						rusqlite::params![OPERATION_ONE],
					)
					.map_err(super::error::sqlite_error)?;
				Ok(())
			})
			.expect("complete initial operation");

		seed_operation(&store, OPERATION_TWO);
		let second = fixture_key(2, DIGEST_TWO, OPERATION_TWO);
		let mut stale = first.clone();
		stale.fingerprint = DIGEST_TWO.to_owned();
		assert_eq!(
			store.rotate_credential(
				&stale,
				CredentialRecord {
					key: second.clone(),
					payload: Zeroizing::new(b"secret-two".to_vec()),
				},
			),
			Err(DatabaseError::Conflict)
		);
		store
			.rotate_credential(
				&first,
				CredentialRecord {
					key: second.clone(),
					payload: Zeroizing::new(b"secret-two".to_vec()),
				},
			)
			.expect("rotate exact credential");
		assert_eq!(store.read_credential(ACCOUNT).expect("read rotated").key, second);
	}

	#[tokio::test]
	#[allow(clippy::too_many_lines)] // One full persistence-and-reopen contract is easier to audit together.
	async fn account_lifecycle_routing_receipts_and_restart_are_durable() {
		let directory = tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let store = SqliteStore::open_test(&path).expect("initialize store");
		let (account_id, operation_id, provider, binding) = account_fixture();

		assert!(
			store
				.attest_codex_account_capability(&CodexAccountCapabilityAttestation {
					build_identity: "codex-test-build".to_owned(),
					executable_sha256: DIGEST_ONE.to_owned(),
					schema_sha256: DIGEST_TWO.to_owned(),
					callback_profile_sha256: DIGEST_ONE.to_owned(),
					login_chatgpt_auth_tokens: true,
					refresh_callback: true,
				})
				.await
				.expect("attest capability")
		);
		let prepared = store
			.prepare_account_operation(&AccountOperationPreparation {
				operation_id: operation_id.clone(),
				account_id: account_id.clone(),
				kind: AccountOperationKind::Enroll,
				display_label: Some("Primary".to_owned()),
				enabled: Some(true),
				expected_account_revision: None,
				expected: None,
				target: Some(binding.clone()),
				provider,
			})
			.await
			.expect("prepare account operation");
		assert!(matches!(
			prepared,
			AccountLifecycleMutationOutcome::Applied(ref mutation)
				if mutation.account_revision == 0
					&& mutation.phase == AccountOperationPhase::Prepared
		));

		store
			.create_credential(CredentialRecord {
				key: fixture_key(1, DIGEST_ONE, OPERATION_ONE),
				payload: Zeroizing::new(b"opaque-test-bundle".to_vec()),
			})
			.expect("write exact credential");
		let applied = store
			.advance_account_operation(
				&operation_id,
				AccountOperationPhase::Prepared,
				AccountOperationPhase::StoreApplied,
				None,
			)
			.await
			.expect("record credential effect");
		assert!(matches!(
			applied,
			AccountLifecycleMutationOutcome::Applied(ref mutation)
				if mutation.phase == AccountOperationPhase::StoreApplied
		));
		let committed = store
			.advance_account_operation(
				&operation_id,
				AccountOperationPhase::StoreApplied,
				AccountOperationPhase::Committed,
				None,
			)
			.await
			.expect("commit account registry");
		assert!(matches!(
			committed,
			AccountLifecycleMutationOutcome::Applied(ref mutation)
				if mutation.account_revision == 1
					&& mutation.phase == AccountOperationPhase::Committed
		));

		let (accounts, routing) =
			store.read_account_registry_snapshot(512).await.expect("read account registry");
		assert_eq!(accounts.len(), 1);
		assert_eq!(accounts[0].lifecycle_readiness, AccountLifecycleReadiness::Ready);
		assert_eq!(accounts[0].five_hour_quota.disposition, AccountQuotaDisposition::Unknown);
		assert_eq!(accounts[0].seven_day_quota.disposition, AccountQuotaDisposition::Unknown);
		assert_eq!(routing.order, vec![account_id.clone()]);
		assert_eq!(routing.mode, AccountSelectionMode::Balanced);

		let routed = store
			.set_fixed_account_selection(routing.revision, &account_id, accounts[0].revision)
			.await
			.expect("set fixed account");
		let fixed_revision = match routed {
			super::RoutingControlOutcome::Updated { routing } => {
				assert_eq!(routing.mode, AccountSelectionMode::Fixed(account_id.clone()));
				routing.revision
			},
			_ => panic!("fixed routing was rejected"),
		};
		assert!(fixed_revision > routing.revision);

		let observed = unix_micros().expect("current time");
		store
			.observe_account_quota_error(
				&account_id,
				AccountQuotaWindow::FIVE_HOURS_MINUTES,
				AccountQuotaObservationError::UnsupportedWindow,
				observed,
			)
			.await
			.expect("observe unsupported quota window");
		store
			.observe_account_quota(
				&account_id,
				AccountQuotaWindow::new(
					AccountQuotaWindow::SEVEN_DAYS_MINUTES,
					25,
					observed + 60_000_000,
				)
				.expect("quota fact"),
				observed,
			)
			.await
			.expect("observe quota fact");

		let command = CommandIdentity::new("account-command-one", b"enable primary")
			.expect("command identity");
		let lease = match store
			.reserve_account_command(
				&command,
				AccountCommandKind::SetEnabled,
				account_id.as_str(),
				Some(1),
			)
			.await
			.expect("reserve command")
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Replayed(_) => panic!("new command replayed"),
		};
		let response = json!({ "status": "ok", "revision": 1 });
		store.complete_account_command(lease, &response).await.expect("complete command");
		match store
			.reserve_account_command(
				&command,
				AccountCommandKind::SetEnabled,
				account_id.as_str(),
				Some(1),
			)
			.await
			.expect("replay command")
		{
			AccountCommandReceiptClaim::Replayed(actual) => assert_eq!(actual, response),
			AccountCommandReceiptClaim::Owned(_) => panic!("completed command was reclaimed"),
		}
		let conflicting = CommandIdentity::new("account-command-one", b"disable primary")
			.expect("conflicting command identity");
		assert!(matches!(
			store
				.reserve_account_command(
					&conflicting,
					AccountCommandKind::SetEnabled,
					account_id.as_str(),
					Some(1),
				)
				.await,
			Err(StoreError::IdempotencyConflict)
		));

		drop(store);
		let reopened = SqliteStore::open_test(&path).expect("reopen store");
		let (accounts, routing) =
			reopened.read_account_registry_snapshot(512).await.expect("read restarted registry");
		assert_eq!(accounts.len(), 1);
		assert_eq!(accounts[0].five_hour_quota.disposition, AccountQuotaDisposition::Unknown);
		assert!(matches!(
			accounts[0].seven_day_quota.disposition,
			AccountQuotaDisposition::Current(fact) if fact.used_percent == 25
		));
		assert_eq!(routing.mode, AccountSelectionMode::Fixed(account_id));
	}

	#[tokio::test]
	async fn recovery_phase_requires_one_bounded_recovery_code() {
		let directory = tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let store = SqliteStore::open_test(&path).expect("initialize store");
		let operation_id = AccountOperationId::new(OPERATION_ONE).expect("operation identity");
		assert!(matches!(
			store
				.advance_account_operation(
					&operation_id,
					AccountOperationPhase::Prepared,
					AccountOperationPhase::RecoveryRequired,
					None,
				)
				.await,
			Err(StoreError::InvalidInput(_))
		));
	}

	#[tokio::test]
	#[allow(clippy::too_many_lines)] // One complete persisted takeover regression is easier to audit than split fixture phases.
	async fn verified_reauthentication_can_replace_a_targetless_ambiguous_refresh() {
		let directory = tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let store = SqliteStore::open_test(&path).expect("initialize store");
		let (account_id, enrollment_id, provider, current) = account_fixture();

		store
			.prepare_account_operation(&AccountOperationPreparation {
				operation_id: enrollment_id.clone(),
				account_id: account_id.clone(),
				kind: AccountOperationKind::Enroll,
				display_label: Some("Primary".to_owned()),
				enabled: Some(true),
				expected_account_revision: None,
				expected: None,
				target: Some(current.clone()),
				provider: provider.clone(),
			})
			.await
			.expect("prepare enrollment");
		store
			.create_credential(CredentialRecord {
				key: fixture_key(1, DIGEST_ONE, OPERATION_ONE),
				payload: Zeroizing::new(b"opaque-current-bundle".to_vec()),
			})
			.expect("write current credential");
		store
			.advance_account_operation(
				&enrollment_id,
				AccountOperationPhase::Prepared,
				AccountOperationPhase::StoreApplied,
				None,
			)
			.await
			.expect("record enrollment store effect");
		store
			.advance_account_operation(
				&enrollment_id,
				AccountOperationPhase::StoreApplied,
				AccountOperationPhase::Committed,
				None,
			)
			.await
			.expect("commit enrollment");

		let ambiguous_id = AccountOperationId::new(OPERATION_TWO).expect("ambiguous operation");
		store
			.prepare_account_operation(&AccountOperationPreparation {
				operation_id: ambiguous_id.clone(),
				account_id: account_id.clone(),
				kind: AccountOperationKind::Refresh,
				display_label: None,
				enabled: None,
				expected_account_revision: Some(1),
				expected: Some(current.clone()),
				target: None,
				provider: provider.clone(),
			})
			.await
			.expect("prepare provider refresh");
		store
			.advance_account_operation(
				&ambiguous_id,
				AccountOperationPhase::Prepared,
				AccountOperationPhase::ProviderEffectPending,
				None,
			)
			.await
			.expect("record possible provider effect");
		store
			.advance_account_operation(
				&ambiguous_id,
				AccountOperationPhase::ProviderEffectPending,
				AccountOperationPhase::RecoveryRequired,
				Some("provider_refresh_ambiguous"),
			)
			.await
			.expect("preserve ambiguous provider effect");

		let reauthentication_id =
			AccountOperationId::new(OPERATION_THREE).expect("reauthentication operation");
		let target = CredentialBinding {
			schema_version: CredentialStoreSchemaVersion::V1,
			version: CredentialVersion::new(2).expect("successor credential version"),
			fingerprint: CredentialFingerprint::new(DIGEST_TWO).expect("target fingerprint"),
			provider: provider.clone(),
			writer_operation_id: reauthentication_id.clone(),
		};
		let prepared = store
			.prepare_account_reauthentication_takeover(
				&AccountOperationPreparation {
					operation_id: reauthentication_id.clone(),
					account_id,
					kind: AccountOperationKind::Refresh,
					display_label: None,
					enabled: None,
					expected_account_revision: Some(1),
					expected: Some(current),
					target: Some(target.clone()),
					provider,
				},
				&ambiguous_id,
			)
			.await
			.expect("prepare verified reauthentication");

		assert!(matches!(
			prepared,
			AccountLifecycleMutationOutcome::Applied(ref mutation)
				if mutation.phase == AccountOperationPhase::Prepared
		));
		let ambiguity_before_effect = store
			.read_account_operation(&ambiguous_id)
			.await
			.expect("read pre-effect ambiguity")
			.expect("pre-effect ambiguity remains durable");
		assert!(ambiguity_before_effect.superseded_by_operation_id.is_none());
		assert!(
			store
				.read_account_registry(None, 512)
				.await
				.expect("read pre-effect account")
				.pop()
				.expect("pre-effect account remains present")
				.unsettled_operation
				.is_some()
		);
		store
			.rotate_credential(
				&fixture_key(1, DIGEST_ONE, OPERATION_ONE),
				CredentialRecord {
					key: fixture_key(2, DIGEST_TWO, OPERATION_THREE),
					payload: Zeroizing::new(b"opaque-verified-bundle".to_vec()),
				},
			)
			.expect("replace exact current credential");
		store
			.advance_account_operation(
				&reauthentication_id,
				AccountOperationPhase::Prepared,
				AccountOperationPhase::StoreApplied,
				None,
			)
			.await
			.expect("record verified credential effect");
		store
			.advance_account_operation(
				&reauthentication_id,
				AccountOperationPhase::StoreApplied,
				AccountOperationPhase::Committed,
				None,
			)
			.await
			.expect("commit verified reauthentication");

		let ambiguity = store
			.read_account_operation(&ambiguous_id)
			.await
			.expect("read ambiguity")
			.expect("ambiguity remains durable");
		assert_eq!(ambiguity.phase, AccountOperationPhase::RecoveryRequired);
		assert_eq!(ambiguity.recovery_code.as_deref(), Some("provider_refresh_ambiguous"));
		assert_eq!(ambiguity.target, None);
		assert_eq!(ambiguity.superseded_by_operation_id, Some(reauthentication_id.clone()));
		let takeover = store
			.read_account_operation(&reauthentication_id)
			.await
			.expect("read takeover")
			.expect("takeover remains durable");
		assert_eq!(takeover.recovery_operation_id, Some(ambiguous_id));
		assert_eq!(takeover.target, Some(target.clone()));
		let account = store
			.read_account_registry(None, 512)
			.await
			.expect("read settled account")
			.pop()
			.expect("account remains present");
		assert_eq!(account.revision, 2);
		assert_eq!(account.credential, Some(target));
		assert!(account.unsettled_operation.is_none());
		assert!(
			store
				.read_unsettled_account_operations(512)
				.await
				.expect("read unsettled operations")
				.is_empty()
		);
	}

	#[test]
	fn foreign_keys_reject_an_unowned_execution_edge() {
		let directory = tempdir().expect("temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let store = SqliteStore::open_test(&path).expect("initialize store");
		let result = store.with_connection(|connection| {
			connection
				.execute(
					"INSERT INTO turns (
					   turn_id, conversation_id, sequence, role, possible_side_effects,
					   status, revision, created_at_micros, updated_at_micros
					 ) VALUES (
					   '30000000-0000-4000-8000-000000000001',
					   '40000000-0000-4000-8000-000000000001',
					   1, 'user', 'none', 'active', 1, 1, 1
					 )",
					[],
				)
				.map(|_| ())
				.map_err(super::error::sqlite_error)
		});
		assert_eq!(result, Err(DatabaseError::Unavailable));
	}

	#[cfg(unix)]
	#[test]
	fn database_and_wal_files_are_owner_private_and_symlink_open_is_rejected() {
		let directory = tempdir().expect("temporary directory");
		fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
			.expect("private temporary directory");
		let path = directory.path().join("decodex.sqlite3");
		let store = SqliteStore::open_test(&path).expect("initialize store");
		assert_eq!(path.metadata().expect("database metadata").mode() & 0o777, 0o600);
		store
			.with_connection(|connection| {
				connection
					.execute_batch("BEGIN IMMEDIATE; COMMIT;")
					.map_err(super::error::sqlite_error)
			})
			.expect("touch WAL");
		let wal = path.with_file_name("decodex.sqlite3-wal");
		if wal.exists() {
			assert_eq!(wal.metadata().expect("WAL metadata").mode() & 0o077, 0);
		}
		drop(store);
		let link = directory.path().join("linked.sqlite3");
		symlink(&path, &link).expect("create symlink");
		assert!(
			Connection::open_with_flags(
				&link,
				rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
					| rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
			)
			.is_err()
		);
	}
}
