//! PostgreSQL product-state authority for Decodex vNext.
//!
//! This crate owns the XY-1267 persistence foundation and XY-1271 Conversation history:
//! immutable migrations, idempotent optimistic transactions, expiring leases, transactional
//! activity/outbox evidence, inert account/quota-window metadata, normalized history, blob
//! references, Context Packs, inert transition proposals, exact in-transaction receipts, and
//! immutable global RoleProfiles, inert ManagedRuns, and current-row
//! managed-repository authority with append-only operations/evidence, plus revisioned routing
//! policies, ordinary capability evidence, immutable routing fact snapshots, and uncomposed causal
//! Codex experiment intent, two one-shot fences, exact start receipts, retained-title
//! attestations, and positive observations, plus atomic routing decisions and
//! inert exactly-once continuation plans with atomic Context-Pack fallback, plus durable inert
//! ledger-first waiting-usage wake transitions, a derived scheduler head, fixed leases,
//! cancellation, supersession, and fresh-routing requests, plus durable fenced ProcessGenerations,
//! exact process identities, append-only positive death evidence, generic ProviderAttempts
//! with positive-only outcome evidence, and read-only execution-decision projection. It does not
//! compose a scheduler, dispatch work, switch
//! credentials, advance runs, replay turns or effects, or expose protocol/client behavior.

mod account_lifecycle;
mod accounts;
mod authority;
mod continuations;
mod conversations;
mod error;
mod exact_commands;
mod execution_decisions;
mod experiments;
mod leases;
mod managed_repositories;
mod managed_runs;
mod migrations;
mod outbox;
mod policies;
mod process_generations;
mod programs;
mod project_agents;
mod provider_attempts;
mod quota;
mod reset_cards;
mod role_profiles;
mod routing;
mod routing_decisions;
mod runtime_sessions;
#[cfg(unix)] mod socket;
mod types;
mod wakes;
mod work_items;

pub use self::{
	account_lifecycle::{
		AccountAdministrationOutcome, AccountCommandKind, AccountCommandReceiptClaim,
		AccountCommandReceiptLease, AccountLifecycleMutation, AccountLifecycleMutationOutcome,
		AccountLifecycleRejection, AccountMigrationReceipt, AccountOperationPreparation,
		AccountStoreObservation, CodexAccountCapabilityAttestation, RoutingControlOutcome,
	},
	continuations::{ContinuationPlanEffect, PlanContinuation},
	conversations::{
		BlobReclaimPage, ContextPackRecord, CreateArtifact, CreateConversation, HistoryCursor,
		HistoryEntry, HistoryPage, PersistContextPack, ProposeTransition, RecordHistoryItem,
		StoredArtifact, StoredConversation,
	},
	error::{BootstrapFailure, StoreError},
	execution_decisions::{ExecutionDecisionReadback, ExecutionQuotaExclusion},
	experiments::{
		AttestCodexExperimentRetainedTitle, BindCodexExperimentStart,
		CodexExperimentCreationFenceOutcome, CodexExperimentStartReceipt,
		CodexExperimentTitleSetFenceOutcome, FenceCodexExperimentTitleSet,
		FreshCodexExperimentCreation, FreshCodexExperimentTitleSet, PrepareCodexExperiment,
		RecordCodexExperimentObservation,
	},
	managed_repositories::{
		RepositoryAdmissionOutcome, RepositoryDispatchFenceOutcome, RepositoryDispatchReceipt,
		RepositoryPreparationOutcome, RepositoryReadbackEvidence, RepositoryReadbackWork,
		RepositoryReconciliationOutcome, RepositoryRestartState,
	},
	managed_runs::{ManagedRunProviderAttempt, StoredManagedRun},
	process_generations::{
		FreshProcessGenerationFence, PrepareProcessGenerationOutcome, ProcessGenerationMutation,
		ProcessGenerationMutationOutcome, ProcessGenerationRejection,
	},
	programs::{ObjectiveRecord, ProgramRecord, UpdateProgramContext},
	provider_attempts::{
		AuthorizeProviderDispatchOutcome, FreshPreparedProviderAttempt, FreshProviderDispatchFence,
		PrepareProviderAttemptOutcome, ProviderAttemptMutation, ProviderAttemptMutationOutcome,
		ProviderAttemptRejection,
	},
	reset_cards::{
		ResetCardClaim, ResetCardFailureCode, ResetCardOperationStatus, ResetCardPreparation,
	},
	role_profiles::{
		BootstrapRoleProfiles, RoleProfileCommandOutcome, RoleProfileConfiguration,
		RoleProfileRejection, RoleProfileRevision, RoleProfileRole,
	},
	routing::{PublishRoutingEvidence, ReplaceRoutingPolicy, RoutingPolicyMemberInput},
	routing_decisions::{PersistedRoutingDecision, RouteAccount},
	runtime_sessions::{
		CreateRuntimeSession, CreateRuntimeSessionAccountSnapshot, RuntimeSessionAccountSnapshot,
		RuntimeSessionCommandEffect, RuntimeSessionCommandOutcome, RuntimeSessionProfileSnapshot,
		RuntimeSessionRejection, StoredRuntimeSession,
	},
	types::{
		AccountMetadata, AccountMutation, ActivityRecord, CommandIdentity, CreateProject,
		HypotheticalFallbackFact, LeaseClaim, OutboxClaim, OutboxReconciliation, OutboxState,
		QuotaExclusionMutation, QuotaExclusionReceipt, QuotaTimestampMicros, QuotaWindow,
		QuotaWindowMutation, ReconciliationOutcome,
	},
	wakes::{
		CancelWaitingUsageWake, ClaimDueWaitingUsageWake, FireWaitingUsageWake,
		RegisterWaitingUsageWake, WaitingUsageWakeClaimEffect,
	},
	work_items::{
		AcceptWorkItem, CreateWorkItem, StoredWorkItem, UpdateWorkItem, WorkItemCommandEffect,
		WorkItemCommandOutcome, WorkItemReadinessBlocker, WorkItemReadinessBlockerKind,
		WorkItemRejection, WorkItemRelations,
	},
};
pub use decodex_core::{
	AcceptedPolicyRevision, AccountId, AccountLifecycleReadiness, AccountOperation,
	AccountOperationId, AccountOperationKind, AccountOperationPhase, AccountProvider,
	AccountQuotaWindow, AccountRecord, AccountRoutingControl, AccountSelectionMode,
	AccountSelectionRecovery, AccountState, AdmissionDescriptorDigest, AdmittedRepositoryIdentity,
	Agent, AgentId, AgentRole, AgentStatus, AggregateCheckpoint, AllocateRepositoryCommand,
	BeginCommitCommand, BeginRegistrationCommand, BeginWorktreeReadyCommand, CanonicalCommitIntent,
	CanonicalOperationDescriptor, CanonicalOperationPayload, CommitEvidence, CommitReadbackRequest,
	ContinuationCommandOutcome, ContinuationPlan, ContinuationPlanKind, ContinuationRejection,
	CredentialBinding, CredentialFingerprint, CredentialStoreSchemaVersion, CredentialVersion,
	ExactCommitEvidence, ExactRegistrationEvidence, ExactRepositoryReadbackScope,
	ExactWorktreeReadyEvidence, ExecutionAssignment, ExecutionAssignmentRole,
	ExecutorContractVersion, ManagedExecutionId, ManagedRepositoryError, ManagedRepositoryFacts,
	ManagedRepositoryId, ManagedRepositoryPhase, ManagedRunError, ManagedRunId, ManagedRunIdentity,
	ManagedRunLifecycle, ManagedRunPhase, ManagedRunState, ManagedRunWaitReason, ManagedWorktreeId,
	NoDispatch, Objective, ObjectiveCompletionEvidence, ObjectiveEvidenceId, ObjectiveId,
	ObjectiveState, OperationDescriptorVersion, OperationView, PersistedAbsolutePath, Policy,
	PolicyId, PolicyProvenance, PolicyRevision, PolicyRevisionAcceptance, PolicyRevisionId,
	PolicySnapshot, PolicySnapshotValue, PolicyStatus, PolicyTimestamp, PositiveAllocationEvidence,
	ProcessAccountQuarantine, ProcessAuthorityLossReason, ProcessBootIdentity, ProcessControlKind,
	ProcessDeathEvidence, ProcessDeathEvidenceId, ProcessDeathEvidenceKind,
	ProcessExecutionAuthorization, ProcessExecutionEpochId, ProcessGeneration,
	ProcessGenerationError, ProcessGenerationId, ProcessGenerationIntent, ProcessGenerationState,
	ProcessIdentity, ProcessIsolationKind, ProcessRunnerIdentity, ProcessStartIdentity, Program,
	ProgramCorrelationId, ProgramError, ProgramId, ProgramMetric, ProgramObservationId,
	ProgramObservationProvenance, ProgramProvenance, ProgramSignal, ProgramState, ProgramTimestamp,
	Project, ProjectAuthority, ProjectId, ProjectMetadata, ProjectMetadataValue,
	ProjectRepositoryBinding, ProjectStatus, ProviderAttempt, ProviderAttemptConsumer,
	ProviderAttemptError, ProviderAttemptId, ProviderAttemptPreparation, ProviderAttemptState,
	ProviderAttemptUnknownReason, ProviderDuplicateRisk, ProviderEvidenceId,
	ProviderEvidenceSource, ProviderIdentity, ProviderPositiveEvidence, ProviderRequestId,
	ProviderRequestKey, ProviderRequestKeys, ProviderTerminalOutcome, RegistrationEvidence,
	RegistrationReadbackRequest, RegistrationTarget, RepositoryAdmissionDescriptor,
	RepositoryAdmissionDescriptorVersion, RepositoryAdmissionFacts, RepositoryAdmittedGitLayout,
	RepositoryAllocationId, RepositoryAmbiguity, RepositoryAuthorityTip, RepositoryCommitActor,
	RepositoryCommitActorEmail, RepositoryCommitActorName, RepositoryCommitMessage,
	RepositoryContentRevision, RepositoryEvidenceId, RepositoryGitRegistrationRole,
	RepositoryIdentity, RepositoryObservationPath, RepositoryObservedObjectType,
	RepositoryOperationId, RepositoryOperationKind, RepositoryOperationResult,
	RepositoryOperationState, RepositoryPathObservation, RepositoryPathRegistrationRole,
	RepositoryReferenceName, RepositoryRegistrationId, ReviewCadence,
	SameThreadContinuationEvidence, WaitingUsageWakeCommandOutcome, WaitingUsageWakeLease,
	WaitingUsageWakeRejection, WaitingUsageWakeState, WaitingUsageWakeTerminalReason,
	WaitingUsageWakeTransition, WaitingUsageWakeTransitionKind, WorkItem, WorkItemCorrelationId,
	WorkItemEdge, WorkItemEdgeKind, WorkItemError, WorkItemId, WorkItemNode, WorkItemObjectiveRef,
	WorkItemPriority, WorkItemProgramRef, WorkItemProvenance, WorkItemState, WorkItemTimestamp,
	WorktreeReadyEvidence, WorktreeReadyPolicy, WorktreeReadyReadbackRequest,
};
pub use quota::parse_quota_timestamp_rfc3339;

use std::{sync::Arc, time::Duration};

use deadpool_postgres::{Client, Manager, ManagerConfig, Pool, RecyclingMethod};
use serde_json::Value;
#[cfg(test)] use tokio as _;
#[cfg(feature = "test-support")] use tokio_postgres::Client as TokioClient;
use tokio_postgres::{Config, config::Host};

#[cfg(unix)] use self::socket::VerifiedSocketConnect;
use decodex_core::{Availability, PostgresConnectionConfig, PostgresIdentityConfig, ProductState};

/// PostgreSQL major accepted by the vNext storage authority.
pub const REQUIRED_POSTGRES_MAJOR: u32 = 18;
/// Stable reason returned by the composition seam before explicit verified configuration.
pub const NOT_CONFIGURED: &str = "PostgreSQL store requires explicit verified configuration";
/// Stable reason returned after the bounded connection pool is explicitly closed.
pub const CLOSED: &str = "PostgreSQL store connection pool is closed";
/// Maximum lease, retry, and retention duration accepted by the product-state adapter.
/// Operational schedules are bounded to one year so interval multiplication and addition
/// to PostgreSQL's current timestamp remain far inside both database representations.
pub const MAX_OPERATION_DURATION_MILLISECONDS: u64 = 365 * 24 * 60 * 60 * 1_000;

const INVALID_DURATION: &str =
	"duration must be a positive whole number of milliseconds no greater than 365 days";
const INVALID_EVIDENCE: &str = "outbox evidence must contain a non-empty JSON value";
// Omitting pg_catalog makes PostgreSQL search it implicitly before the explicitly configured
// public ledger namespace, while leaving unqualified migration DDL targeted at public.
const TRUSTED_SESSION_OPTIONS: &str = "-csearch_path=public";

/// Product-state authority selected by this infrastructure owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductStateAuthority {
	/// PostgreSQL domain tables and their transactional mechanisms.
	Postgres,
}

/// Connected and migration-verified PostgreSQL product-state store.
#[derive(Clone)]
pub struct PostgresStore {
	pool: Arc<Pool>,
	#[cfg(unix)]
	connector: VerifiedSocketConnect,
	configured_migration_role: Arc<str>,
	configured_runtime_role: Arc<str>,
}
impl PostgresStore {
	/// Return the exact schema-manifest query for isolated dump/restore fixtures.
	#[cfg(feature = "test-support")]
	#[doc(hidden)]
	pub const fn schema_contract_sql_fixture() -> &'static str {
		authority::schema_contract_sql_fixture()
	}

	/// Return the configured-principal and ACL manifest query for isolated restore fixtures.
	#[cfg(feature = "test-support")]
	#[doc(hidden)]
	pub const fn configured_authority_sql_fixture() -> &'static str {
		authority::configured_authority_sql_fixture()
	}

	/// Return the closed execution-path query and allowed function identities for fixtures.
	#[cfg(feature = "test-support")]
	#[doc(hidden)]
	pub fn execution_path_contract_fixture() -> (&'static str, Vec<&'static str>) {
		authority::execution_path_contract_fixture()
	}

	/// Evaluate the production runtime-routine authority query without exporting catalog values.
	#[cfg(feature = "test-support")]
	#[doc(hidden)]
	pub async fn runtime_routine_authority_fixture(client: &TokioClient) -> Result<(), StoreError> {
		authority::runtime_routine_authority_fixture(client).await
	}

	/// Evaluate and serialize the finalized production semantic-authority contract.
	#[cfg(feature = "test-support")]
	#[doc(hidden)]
	pub async fn semantic_authority_fixture(
		client: &TokioClient,
		runtime_role: &str,
	) -> Result<serde_json::Value, StoreError> {
		authority::semantic_authority_fixture(client, runtime_role).await
	}

	/// Parse and prepare every changed V22/V27 embedded SQL source without executing it.
	#[cfg(feature = "test-support")]
	#[doc(hidden)]
	pub async fn prepare_changed_sql_fixture(client: &TokioClient) -> Result<usize, StoreError> {
		let retained_title = experiments::prepare_retained_title_sql(client).await?;
		let account_lifecycle = account_lifecycle::prepare_account_lifecycle_sql(client).await?;
		let process_generation =
			process_generations::prepare_account_bound_process_generation_sql(client).await?;
		let reset_card = reset_cards::prepare_account_bound_reset_card_sql(client).await?;

		Ok(retained_title + account_lifecycle + process_generation + reset_card)
	}

	/// Apply the production connection-startup invariant to an isolated raw fixture.
	#[cfg(feature = "test-support")]
	#[doc(hidden)]
	pub fn pin_session_search_path_fixture(config: &mut Config) {
		pin_session_search_path(config);
	}

	/// Run migration/verification through the migration identity, close it, then retain
	/// only a separately verified least-privilege runtime pool.
	pub async fn connect_explicit(
		config: &PostgresConnectionConfig,
		migration_password: Option<&str>,
		runtime_password: Option<&str>,
	) -> Result<Self, StoreError> {
		let migration = connection_config(config, config.migration(), migration_password);
		let runtime = connection_config(config, config.runtime(), runtime_password);

		Self::connect(migration, runtime, config.expected_peer_uid()).await
	}

	/// Apply embedded migrations and install the exact steady-state runtime authority through one
	/// single-use migration connection. No migration credential enters the retained runtime pool.
	#[cfg(unix)]
	pub async fn migrate_and_provision_explicit(
		config: &PostgresConnectionConfig,
		migration_password: Option<&str>,
	) -> Result<(), StoreError> {
		let mut migration = connection_config(config, config.migration(), migration_password);
		validate_connection(&migration)?;
		let connector = verified_socket_connect(&migration, config.expected_peer_uid())?;
		pin_session_search_path(&mut migration);
		let manager = Manager::from_connect(
			migration,
			connector.clone(),
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager).max_size(1).build()?;
		let mut client = checkout(&pool, &connector).await?;
		let result = async {
			migrations::run(&mut client).await?;
			authority::provision_runtime(&client, config.database(), config.runtime().user())
				.await?;
			migrations::verify(&client).await
		}
		.await;
		drop(client);
		pool.close();

		result
	}

	/// Apply the account-cutover migration with one session-local manifest handoff. If V27 already
	/// committed, this entrypoint performs exact receipt readback and does not write a second
	/// intent.
	#[cfg(unix)]
	pub async fn migrate_account_cutover_explicit(
		config: &PostgresConnectionConfig,
		migration_password: Option<&str>,
		manifest_sha256: &str,
		manifest: &Value,
		account_count: u32,
	) -> Result<bool, StoreError> {
		if manifest_sha256.len() != 64
			|| !manifest_sha256
				.bytes()
				.all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
			|| account_count > 512
		{
			return Err(StoreError::InvalidInput("account migration handoff is invalid"));
		}
		ensure_credential_negative_json(manifest)?;
		let count = i32::try_from(account_count)
			.map_err(|_| StoreError::InvalidInput("migration account count overflows"))?;
		let mut migration = connection_config(config, config.migration(), migration_password);
		validate_connection(&migration)?;
		let connector = verified_socket_connect(&migration, config.expected_peer_uid())?;
		pin_session_search_path(&mut migration);
		let manager = Manager::from_connect(
			migration,
			connector.clone(),
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager).max_size(1).build()?;
		let mut client = checkout(&pool, &connector).await?;
		let result = async {
			let history_exists: bool = client
				.query_one(
					"SELECT pg_catalog.to_regclass('public.refinery_schema_history') IS NOT NULL",
					&[],
				)
				.await?
				.get(0);
			let v27_applied = history_exists
				&& client
					.query_opt("SELECT 1 FROM public.refinery_schema_history WHERE version=27", &[])
					.await?
					.is_some();
			if !v27_applied {
				client
					.batch_execute(
						"CREATE TEMPORARY TABLE pg_temp.decodex_account_migration_handoff(\
						 manifest_sha256 text NOT NULL,\
						 manifest jsonb NOT NULL,\
						 account_count integer NOT NULL\
						 ) ON COMMIT PRESERVE ROWS",
					)
					.await?;
				client
					.execute(
						"INSERT INTO pg_temp.decodex_account_migration_handoff(\
						 manifest_sha256,manifest,account_count) VALUES($1,$2,$3)",
						&[&manifest_sha256, manifest, &count],
					)
					.await?;
			}
			migrations::run(&mut client).await?;
			if !v27_applied {
				let remaining: i64 = client
					.query_one(
						"SELECT pg_catalog.count(*) \
						 FROM pg_temp.decodex_account_migration_handoff",
						&[],
					)
					.await?
					.get(0);
				if remaining != 0 {
					return Err(StoreError::Incompatible(
						"V27 did not consume the account migration handoff".into(),
					));
				}
			}
			let receipt = client
				.query_opt(
					"SELECT manifest_sha256,manifest,account_count \
					 FROM decodex.account_migration_receipts WHERE singleton",
					&[],
				)
				.await?
				.ok_or_else(|| {
					StoreError::Incompatible(
						"account cutover migration intent is absent after V27".into(),
					)
				})?;
			let receipt_manifest: Value = receipt.get(1);
			if receipt.get::<_, &str>(0) != manifest_sha256
				|| &receipt_manifest != manifest
				|| receipt.get::<_, i32>(2) != count
			{
				return Err(StoreError::IdempotencyConflict);
			}
			authority::provision_runtime(&client, config.database(), config.runtime().user())
				.await?;
			migrations::verify(&client).await?;
			Ok(!v27_applied)
		}
		.await;
		drop(client);
		pool.close();

		result
	}

	/// Replace the frozen account-migration routing projection through one short-lived migration
	/// connection. The retained runtime pool has no authority for this migration-only SQL routine.
	#[cfg(unix)]
	pub async fn replace_account_routing_for_migration_explicit(
		config: &PostgresConnectionConfig,
		migration_password: Option<&str>,
		mode: &decodex_core::AccountSelectionMode,
		order: &[AccountId],
	) -> Result<decodex_core::AccountRoutingControl, StoreError> {
		let mut migration = connection_config(config, config.migration(), migration_password);
		validate_connection(&migration)?;
		let connector = verified_socket_connect(&migration, config.expected_peer_uid())?;
		pin_session_search_path(&mut migration);
		let manager = Manager::from_connect(
			migration,
			connector.clone(),
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager).max_size(1).build()?;
		let mut client = checkout(&pool, &connector).await?;
		let result = account_lifecycle::replace_account_routing_for_migration_explicit(
			&mut client,
			mode,
			order,
		)
		.await;
		drop(client);
		pool.close();

		result
	}

	/// Read the completed installer-only account migration receipt through one single-use
	/// migration connection. The steady-state runtime role receives no receipt-table authority.
	#[cfg(unix)]
	pub async fn read_completed_account_migration_receipt_explicit(
		config: &PostgresConnectionConfig,
		migration_password: Option<&str>,
	) -> Result<Option<AccountMigrationReceipt>, StoreError> {
		let mut migration = connection_config(config, config.migration(), migration_password);
		validate_connection(&migration)?;
		let connector = verified_socket_connect(&migration, config.expected_peer_uid())?;
		pin_session_search_path(&mut migration);
		let manager = Manager::from_connect(
			migration,
			connector.clone(),
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager).max_size(1).build()?;
		let client = checkout(&pool, &connector).await?;
		let row = client
			.query_opt(
				"SELECT manifest_sha256,manifest,destination_receipt,retirement_receipt,account_count \
				 FROM decodex.account_migration_receipts \
				 WHERE singleton AND phase='completed'",
				&[],
			)
			.await?;
		let receipt = row
			.map(|row| {
				let manifest: Value = row.get(1);
				let destination_receipt: Value = row.get(2);
				let retirement_receipt: Value = row.get(3);
				ensure_credential_negative_json(&manifest)?;
				ensure_credential_negative_json(&destination_receipt)?;
				ensure_credential_negative_json(&retirement_receipt)?;
				let account_count = u32::try_from(row.get::<_, i32>(4)).map_err(|_| {
					StoreError::Incompatible("migration account count is malformed".into())
				})?;
				Ok::<AccountMigrationReceipt, StoreError>(AccountMigrationReceipt {
					manifest_sha256: row.get(0),
					manifest,
					destination_receipt,
					retirement_receipt,
					account_count,
				})
			})
			.transpose()?;
		drop(client);
		pool.close();
		Ok(receipt)
	}

	/// Connect two explicit identities to one Unix-socket endpoint. The migration
	/// connection is never placed in or retained by the runtime pool.
	#[cfg(unix)]
	pub async fn connect(
		migration: Config,
		runtime: Config,
		expected_peer_uid: u32,
	) -> Result<Self, StoreError> {
		Self::connect_with_pool_size(migration, runtime, expected_peer_uid, 32).await
	}

	/// Construct a single-connection runtime pool for cross-adapter contention fixtures.
	#[cfg(all(unix, feature = "test-support"))]
	#[doc(hidden)]
	pub async fn connect_fixture(
		migration: Config,
		runtime: Config,
		expected_peer_uid: u32,
	) -> Result<Self, StoreError> {
		Self::connect_with_pool_size(migration, runtime, expected_peer_uid, 1).await
	}

	#[cfg(unix)]
	async fn connect_with_pool_size(
		migration: Config,
		mut runtime: Config,
		expected_peer_uid: u32,
		pool_size: usize,
	) -> Result<Self, StoreError> {
		validate_connection(&migration)?;
		validate_connection(&runtime)?;
		validate_separation(&migration, &runtime)?;

		let configured_migration_role = Arc::<str>::from(
			migration.get_user().ok_or(StoreError::InvalidInput("migration role is absent"))?,
		);
		let configured_runtime_role = Arc::<str>::from(
			runtime.get_user().ok_or(StoreError::InvalidInput("runtime role is absent"))?,
		);
		let connector = verified_socket_connect(&migration, expected_peer_uid)?;

		Self::migrate_with_connector(migration, connector.clone()).await?;

		pin_session_search_path(&mut runtime);

		let manager = Manager::from_connect(
			runtime,
			connector.clone(),
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager).max_size(pool_size).build()?;
		let client = checkout(&pool, &connector).await?;

		authority::verify_runtime(&client, &configured_migration_role, &configured_runtime_role)
			.await?;
		migrations::verify(&client).await?;

		drop(client);

		if pool_size > 1 {
			let first = checkout(&pool, &connector).await?;
			let second = checkout(&pool, &connector).await?;

			drop((first, second));
		}

		Ok(Self {
			pool: Arc::new(pool),
			connector,
			configured_migration_role,
			configured_runtime_role,
		})
	}

	#[cfg(not(unix))]
	pub async fn connect(
		_migration: Config,
		_runtime: Config,
		_expected_peer_uid: u32,
	) -> Result<Self, StoreError> {
		Err(StoreError::Incompatible("PostgreSQL Unix sockets require a Unix host".into()))
	}

	/// Run embedded forward migrations and migration-state verification through one
	/// single-use connection. This connection is closed before a live store is returned.
	#[cfg(unix)]
	pub async fn migrate(config: Config, expected_peer_uid: u32) -> Result<(), StoreError> {
		validate_connection(&config)?;

		let connector = verified_socket_connect(&config, expected_peer_uid)?;

		Self::migrate_with_connector(config, connector).await
	}

	/// Install or verify one external ProcessGeneration execution epoch through a single-use
	/// migration connection. The runtime pool never receives table authority or this method's
	/// external digest from PostgreSQL.
	#[cfg(unix)]
	pub async fn provision_process_execution_authorization_explicit(
		config: &PostgresConnectionConfig,
		migration_password: Option<&str>,
		authorization: &ProcessExecutionAuthorization,
	) -> Result<(), StoreError> {
		let mut migration = connection_config(config, config.migration(), migration_password);
		validate_connection(&migration)?;
		let connector = verified_socket_connect(&migration, config.expected_peer_uid())?;
		pin_session_search_path(&mut migration);
		let manager = Manager::from_connect(
			migration,
			connector.clone(),
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager).max_size(1).build()?;
		let mut client = checkout(&pool, &connector).await?;
		let transaction = client.transaction().await?;
		transaction
			.execute(
				"INSERT INTO decodex.process_generation_execution_epochs(\
				 execution_epoch_id,authorization_digest,authorized_at,retired_at) \
				 VALUES($1::text::uuid,$2,pg_catalog.clock_timestamp(),NULL) \
				 ON CONFLICT (execution_epoch_id) DO NOTHING",
				&[&authorization.epoch_id.as_str(), &authorization.authorization_digest],
			)
			.await?;
		let active = transaction
			.query(
				"SELECT execution_epoch_id::text,authorization_digest \
				 FROM decodex.process_generation_execution_epochs \
				 WHERE retired_at IS NULL FOR UPDATE",
				&[],
			)
			.await?;
		if active.len() != 1
			|| active[0].get::<_, &str>(0) != authorization.epoch_id.as_str()
			|| active[0].get::<_, &str>(1) != authorization.authorization_digest.as_str()
		{
			return Err(StoreError::InvalidInput(
				"active ProcessGeneration execution authorization conflicts",
			));
		}
		transaction.commit().await?;
		drop(client);
		pool.close();
		Ok(())
	}

	/// Apply the immutable migration ledger only through V7 for V8 boundary fixtures.
	#[cfg(all(unix, feature = "test-support"))]
	#[doc(hidden)]
	pub async fn migrate_fixture_through_v7(
		mut config: Config,
		expected_peer_uid: u32,
	) -> Result<(), StoreError> {
		validate_connection(&config)?;

		let connector = verified_socket_connect(&config, expected_peer_uid)?;

		pin_session_search_path(&mut config);

		let manager = Manager::from_connect(
			config,
			connector.clone(),
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager).max_size(1).build()?;
		let mut client = checkout(&pool, &connector).await?;
		let result = migrations::run_through_v7(&mut client).await;

		drop(client);

		pool.close();

		result
	}

	/// Apply the immutable migration ledger only through V8 for the V9 upgrade proof.
	#[cfg(all(unix, feature = "test-support"))]
	#[doc(hidden)]
	pub async fn migrate_fixture_through_v8(
		mut config: Config,
		expected_peer_uid: u32,
	) -> Result<(), StoreError> {
		validate_connection(&config)?;

		let connector = verified_socket_connect(&config, expected_peer_uid)?;

		pin_session_search_path(&mut config);

		let manager = Manager::from_connect(
			config,
			connector.clone(),
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager).max_size(1).build()?;
		let mut client = checkout(&pool, &connector).await?;
		let result = migrations::run_through_v8(&mut client).await;

		drop(client);

		pool.close();

		result
	}

	/// Apply the immutable migration ledger only through V9 for the V10 upgrade proof.
	#[cfg(all(unix, feature = "test-support"))]
	#[doc(hidden)]
	pub async fn migrate_fixture_through_v9(
		mut config: Config,
		expected_peer_uid: u32,
	) -> Result<(), StoreError> {
		validate_connection(&config)?;

		let connector = verified_socket_connect(&config, expected_peer_uid)?;

		pin_session_search_path(&mut config);

		let manager = Manager::from_connect(
			config,
			connector.clone(),
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager).max_size(1).build()?;
		let mut client = checkout(&pool, &connector).await?;
		let result = migrations::run_through_v9(&mut client).await;

		drop(client);

		pool.close();

		result
	}

	/// Apply the immutable migration ledger only through V10 for isolated acceptance fixtures.
	#[cfg(all(unix, feature = "test-support"))]
	#[doc(hidden)]
	pub async fn migrate_fixture_through_v10(
		mut config: Config,
		expected_peer_uid: u32,
	) -> Result<(), StoreError> {
		validate_connection(&config)?;

		let connector = verified_socket_connect(&config, expected_peer_uid)?;

		pin_session_search_path(&mut config);

		let manager = Manager::from_connect(
			config,
			connector.clone(),
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager).max_size(1).build()?;
		let mut client = checkout(&pool, &connector).await?;
		let result = migrations::run_through_v10(&mut client).await;

		drop(client);

		pool.close();

		result
	}

	/// Apply the immutable migration ledger only through V13 for the V14 authority upgrade proof.
	#[cfg(all(unix, feature = "test-support"))]
	#[doc(hidden)]
	pub async fn migrate_fixture_through_v13(
		mut config: Config,
		expected_peer_uid: u32,
	) -> Result<(), StoreError> {
		validate_connection(&config)?;

		let connector = verified_socket_connect(&config, expected_peer_uid)?;

		pin_session_search_path(&mut config);

		let manager = Manager::from_connect(
			config,
			connector.clone(),
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager).max_size(1).build()?;
		let mut client = checkout(&pool, &connector).await?;
		let result = migrations::run_through_v13(&mut client).await;

		drop(client);

		pool.close();

		result
	}

	/// Apply the immutable migration ledger only through V26 for the V27 account cutover gate.
	#[cfg(all(unix, feature = "test-support"))]
	#[doc(hidden)]
	pub async fn migrate_fixture_through_v26(
		mut config: Config,
		expected_peer_uid: u32,
	) -> Result<(), StoreError> {
		validate_connection(&config)?;

		let connector = verified_socket_connect(&config, expected_peer_uid)?;

		pin_session_search_path(&mut config);

		let manager = Manager::from_connect(
			config,
			connector.clone(),
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager).max_size(1).build()?;
		let mut client = checkout(&pool, &connector).await?;
		let result = migrations::run_through_v26(&mut client).await;

		drop(client);

		pool.close();

		result
	}

	#[cfg(not(unix))]
	pub async fn migrate(_config: Config, _expected_peer_uid: u32) -> Result<(), StoreError> {
		Err(StoreError::Incompatible("PostgreSQL Unix sockets require a Unix host".into()))
	}

	#[cfg(unix)]
	async fn migrate_with_connector(
		mut config: Config,
		connector: VerifiedSocketConnect,
	) -> Result<(), StoreError> {
		pin_session_search_path(&mut config);

		let manager = Manager::from_connect(
			config,
			connector.clone(),
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager).max_size(1).build()?;
		let mut client = checkout(&pool, &connector).await?;
		let result = async {
			migrations::run(&mut client).await?;

			migrations::verify(&client).await
		}
		.await;

		drop(client);

		pool.close();

		result
	}

	/// Report the concrete authority owned by this adapter.
	pub const fn authority(&self) -> ProductStateAuthority {
		ProductStateAuthority::Postgres
	}

	/// Close the bounded pool. Existing checked-out connections finish before closure.
	pub fn close(&self) {
		self.pool.close();
	}

	/// Revalidate the retained endpoint, live runtime session, immutable migrations, and
	/// least-privilege authority without reconnecting migration credentials or running DDL.
	#[cfg(unix)]
	pub async fn revalidate(&self) -> Result<(), StoreError> {
		let client = checkout(&self.pool, &self.connector).await?;

		client.simple_query("SELECT 1").await?;

		authority::verify_runtime(
			&client,
			&self.configured_migration_role,
			&self.configured_runtime_role,
		)
		.await?;
		migrations::verify(&client).await?;

		self.connector.verify()?;

		Ok(())
	}

	#[cfg(not(unix))]
	pub async fn revalidate(&self) -> Result<(), StoreError> {
		Err(StoreError::Incompatible("PostgreSQL Unix sockets require a Unix host".into()))
	}

	pub(crate) fn pool(&self) -> &Pool {
		&self.pool
	}
}

impl ProductState for PostgresStore {
	fn availability(&self) -> Availability {
		if self.pool.is_closed() {
			Availability::Unavailable { reason: CLOSED }
		} else {
			// This synchronous port reports verified configuration and local pool lifecycle.
			// Individual operations remain authoritative for live PostgreSQL connectivity.
			Availability::Available
		}
	}
}

/// Unconfigured composition seam used until the path/bootstrap owner supplies explicit
/// connection configuration. It never opens a default or ambient PostgreSQL service.
#[derive(Clone, Copy, Debug, Default)]
pub struct UnavailablePostgresStore;
impl UnavailablePostgresStore {
	/// Construct the fail-closed, unconfigured adapter seam.
	pub const fn new() -> Self {
		Self
	}

	/// Report the concrete authority selected by the seam.
	pub const fn authority(self) -> ProductStateAuthority {
		ProductStateAuthority::Postgres
	}
}

impl ProductState for UnavailablePostgresStore {
	fn availability(&self) -> Availability {
		Availability::Unavailable { reason: NOT_CONFIGURED }
	}
}

pub(crate) fn exact_milliseconds(duration: Duration) -> Result<i64, StoreError> {
	if duration.is_zero() || !duration.subsec_nanos().is_multiple_of(1_000_000) {
		return Err(StoreError::InvalidInput(INVALID_DURATION));
	}

	let milliseconds = u64::try_from(duration.as_millis())
		.map_err(|_| StoreError::InvalidInput(INVALID_DURATION))?;

	if milliseconds > MAX_OPERATION_DURATION_MILLISECONDS {
		return Err(StoreError::InvalidInput(INVALID_DURATION));
	}

	i64::try_from(milliseconds).map_err(|_| StoreError::InvalidInput(INVALID_DURATION))
}

pub(crate) fn ensure_meaningful_evidence(value: &Value) -> Result<(), StoreError> {
	let meaningful = match value {
		Value::Null => false,
		Value::Bool(_) | Value::Number(_) => true,
		Value::String(value) => !value.trim().is_empty(),
		Value::Array(entries) => entries.iter().any(is_meaningful_evidence),
		Value::Object(entries) => entries.values().any(is_meaningful_evidence),
	};

	if meaningful { Ok(()) } else { Err(StoreError::InvalidInput(INVALID_EVIDENCE)) }
}

pub(crate) fn ensure_credential_negative_text(value: &str) -> Result<(), StoreError> {
	if decodex_core::contains_credential_material(value) {
		Err(StoreError::CredentialRejected)
	} else {
		Ok(())
	}
}

pub(crate) fn ensure_credential_negative_json(value: &Value) -> Result<(), StoreError> {
	match value {
		Value::Object(entries) =>
			for (key, value) in entries {
				if decodex_core::is_credential_metadata_key(key) {
					return Err(StoreError::CredentialRejected);
				}

				ensure_credential_negative_json(value)?;
			},
		Value::Array(entries) =>
			for value in entries {
				ensure_credential_negative_json(value)?;
			},
		Value::String(value) => ensure_credential_negative_text(value)?,
		_ => {},
	}

	Ok(())
}

fn pin_session_search_path(config: &mut Config) {
	// Startup-packet options are applied by PostgreSQL before it parses the first query.
	// Replacing caller-provided options prevents role/database defaults from influencing
	// migration, identity, schema, or configured-authority verification.
	config.options(TRUSTED_SESSION_OPTIONS);
}

#[cfg(unix)]
fn verified_socket_connect(
	config: &Config,
	expected_peer_uid: u32,
) -> Result<VerifiedSocketConnect, StoreError> {
	let Some(Host::Unix(directory)) = config.get_hosts().first() else {
		return Err(StoreError::Incompatible(
			"PostgreSQL must use one explicit Unix socket host".into(),
		));
	};
	let port = config.get_ports().first().copied().unwrap_or(5_432);

	VerifiedSocketConnect::new(directory, port, expected_peer_uid)
}

fn connection_config(
	config: &PostgresConnectionConfig,
	identity: &PostgresIdentityConfig,
	password: Option<&str>,
) -> Config {
	let mut connection = Config::new();

	connection
		.host_path(config.socket_directory())
		.port(config.port())
		.dbname(config.database())
		.user(identity.user());

	if let Some(password) = password {
		connection.password(password);
	}

	connection
}

fn validate_connection(config: &Config) -> Result<(), StoreError> {
	if config.get_hosts().len() == 1 && matches!(config.get_hosts().first(), Some(Host::Unix(_))) {
		Ok(())
	} else {
		Err(StoreError::Incompatible("PostgreSQL must use one explicit Unix socket host".into()))
	}
}

fn validate_separation(migration: &Config, runtime: &Config) -> Result<(), StoreError> {
	let same_endpoint = migration.get_hosts() == runtime.get_hosts()
		&& migration.get_ports() == runtime.get_ports()
		&& migration.get_dbname() == runtime.get_dbname();
	let distinct_identity = migration.get_user().is_some()
		&& runtime.get_user().is_some()
		&& migration.get_user() != runtime.get_user();

	if !same_endpoint {
		return Err(StoreError::Incompatible(
			"migration and runtime identities must use one PostgreSQL endpoint".into(),
		));
	}
	if !distinct_identity {
		Err(StoreError::UnsafeAuthority(
			"migration and runtime PostgreSQL identities must be distinct",
		))
	} else {
		Ok(())
	}
}

fn is_meaningful_evidence(value: &Value) -> bool {
	ensure_meaningful_evidence(value).is_ok()
}

#[cfg(unix)]
async fn checkout(pool: &Pool, connector: &VerifiedSocketConnect) -> Result<Client, StoreError> {
	connector.verify()?;

	pool.get().await.map_err(StoreError::Pool)
}
