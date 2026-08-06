//! Canonical PostgreSQL product-state authority for Decodex vNext.
//!
//! This crate owns the direct latest schema, explicit empty-database bootstrap, strict runtime
//! catalog and authority verification, and typed persistence operations. Runtime connections run
//! no DDL. Higher layers own capability composition, scheduling, process dispatch, and protocol
//! behavior.

mod account_lifecycle;
mod account_profiles;
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
mod outbox;
mod policies;
mod process_generations;
mod programs;
mod project_agents;
mod provider_attempts;
mod quick_task_routing;
mod quota;
mod reset_cards;
mod role_profiles;
mod routing;
mod routing_decisions;
mod runtime_sessions;
mod schema;
#[cfg(unix)] mod socket;
mod types;
mod wakes;
mod work_items;

pub use self::{
	account_lifecycle::{
		AccountAdministrationOutcome, AccountCommandKind, AccountCommandReceiptClaim,
		AccountCommandReceiptLease, AccountLifecycleMutation, AccountLifecycleMutationOutcome,
		AccountLifecycleRejection, AccountOperationPreparation, AccountStoreObservation,
		CodexAccountCapabilityAttestation, LocalAccountAuthorityAccount,
		LocalAccountAuthorityRestore, LocalAccountAuthorityRestoreFailure, RoutingControlOutcome,
	},
	account_profiles::{
		AccountProfileDailyUsage, AccountProfileObservation, AccountProfileObservationOutcome,
		AccountProfileSnapshot,
	},
	continuations::{ContinuationPlanEffect, PlanContinuation, PlanInitialThreadContinuation},
	conversations::{
		AdmitInitialQuickTaskTurn, BlobReclaimPage, ContextPackRecord, CreateArtifact,
		CreateConversation, CreateQuickTaskConversation, CreateQuickTaskRoutingSuccessor,
		HistoryCursor, HistoryEntry, HistoryPage, InitialQuickTaskTurnAdmissionOutcome,
		InitialQuickTaskTurnAdmissionReadback, InitialQuickTaskTurnAdmissionRejection,
		OrdinaryTaskConversationCursor, OrdinaryTaskConversationProjection,
		OrdinaryTaskConversationReadback, OrdinaryTaskPreSessionState, PersistContextPack,
		ProposeTransition, QuickTaskRequest, QuickTaskRoutingSuccessor,
		QuickTaskRoutingSuccessorOutcome, QuickTaskTerminalizationOutcome, RecordHistoryItem,
		StoredArtifact, StoredConversation, TerminalizeQuickTaskTurn, TurnReservationOutcome,
		TurnReservationReadback,
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
		ProviderAttemptRejection, RuntimeSessionBindingReceipt,
	},
	quick_task_routing::{
		BindQuickTaskContinuation, QuickTaskContinuationBinding, QuickTaskInitialRoute,
		QuickTaskInitialRouteOutcome, RouteQuickTaskInitial,
	},
	reset_cards::{
		RESET_CARD_API_CALLBACK_PROFILE_SHA256, ResetCardClaim, ResetCardFailureCode,
		ResetCardOperationStatus, ResetCardPreparation,
	},
	role_profiles::{
		BootstrapRoleProfiles, RoleProfileCommandOutcome, RoleProfileConfiguration,
		RoleProfileRejection, RoleProfileRevision, RoleProfileRole,
	},
	routing::{PublishRoutingEvidence, ReplaceRoutingPolicy, RoutingPolicyMemberInput},
	routing_decisions::{PersistedRoutingDecision, RouteAccount},
	runtime_sessions::{
		BindRuntimeSessionThread, BindRuntimeSessionThreadOutcome, CreateRuntimeSession,
		CreateRuntimeSessionAccountSnapshot, FenceRuntimeSessionThreadStart,
		FenceRuntimeSessionThreadStartOutcome, FreshQuickTaskProcessGeneration,
		FreshRuntimeSessionThreadStart, OrdinaryRuntimeSessionResumeReadback,
		PrepareQuickTaskProcessGeneration, PrepareQuickTaskProcessGenerationOutcome,
		QuickTaskThreadEstablishmentReadback, ReconcileQuickTaskThreadEstablishment,
		RuntimeSessionAccountSnapshot, RuntimeSessionCommandEffect, RuntimeSessionCommandOutcome,
		RuntimeSessionProfileSnapshot, RuntimeSessionRejection,
		RuntimeSessionThreadBindingReadback, StoredRuntimeSession,
		SuccessfulRuntimeSessionThreadStart,
	},
	schema::{BOOTSTRAP_AUTHORITY_REPORT_PREFIX, LatestSchemaBootstrapFailure},
	types::{
		AccountMetadata, ActivityRecord, CommandIdentity, CreateProject, HypotheticalFallbackFact,
		LeaseClaim, OutboxClaim, OutboxReconciliation, OutboxState, QuotaExclusionMutation,
		QuotaExclusionReceipt, QuotaTimestampMicros, QuotaWindow, QuotaWindowMutation,
		ReconciliationOutcome,
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
	AccountQuotaWindow, AccountRecord, AccountRegistryQuotaFact, AccountRegistryQuotaObservation,
	AccountRegistryRoutingDecision, AccountRegistryRoutingDecisionKind,
	AccountRegistryRoutingExclusion, AccountRegistryRoutingMember, AccountRegistryRoutingSnapshot,
	AccountRoutingControl, AccountSelectionMode, AccountSelectionRecovery, AccountState,
	AdmissionDescriptorDigest, AdmittedRepositoryIdentity, Agent, AgentId, AgentRole, AgentStatus,
	AggregateCheckpoint, AllocateRepositoryCommand, BeginCommitCommand, BeginRegistrationCommand,
	BeginWorktreeReadyCommand, CanonicalCommitIntent, CanonicalOperationDescriptor,
	CanonicalOperationPayload, CommitEvidence, CommitReadbackRequest, ContinuationCommandOutcome,
	ContinuationPlan, ContinuationPlanKind, ContinuationRejection, CredentialBinding,
	CredentialFingerprint, CredentialStoreSchemaVersion, CredentialVersion, ExactCommitEvidence,
	ExactRegistrationEvidence, ExactRepositoryReadbackScope, ExactWorktreeReadyEvidence,
	ExecutionAssignment, ExecutionAssignmentRole, ExecutorContractVersion, ManagedExecutionId,
	ManagedRepositoryError, ManagedRepositoryFacts, ManagedRepositoryId, ManagedRepositoryPhase,
	ManagedRunError, ManagedRunId, ManagedRunIdentity, ManagedRunLifecycle, ManagedRunPhase,
	ManagedRunState, ManagedRunWaitReason, ManagedWorktreeId, NoDispatch, Objective,
	ObjectiveCompletionEvidence, ObjectiveEvidenceId, ObjectiveId, ObjectiveState,
	OperationDescriptorVersion, OperationView, PersistedAbsolutePath, Policy, PolicyId,
	PolicyProvenance, PolicyRevision, PolicyRevisionAcceptance, PolicyRevisionId, PolicySnapshot,
	PolicySnapshotValue, PolicyStatus, PolicyTimestamp, PositiveAllocationEvidence,
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
use tokio_postgres::{Config, config::Host};

#[cfg(unix)] use self::socket::VerifiedSocketConnect;
use decodex_core::{Availability, PostgresConnectionConfig, PostgresIdentityConfig, ProductState};

/// PostgreSQL major accepted by the vNext storage authority.
pub const REQUIRED_POSTGRES_MAJOR: u32 = 18;
/// Stable reason returned after the bounded connection pool is explicitly closed.
pub const CLOSED: &str = "PostgreSQL store connection pool is closed";
/// Maximum lease, retry, and retention duration accepted by the product-state adapter.
/// Operational schedules are bounded to one year so interval multiplication and addition
/// to PostgreSQL's current timestamp remain far inside both database representations.
pub const MAX_OPERATION_DURATION_MILLISECONDS: u64 = 365 * 24 * 60 * 60 * 1_000;

const INVALID_DURATION: &str =
	"duration must be a positive whole number of milliseconds no greater than 365 days";
const INVALID_EVIDENCE: &str = "outbox evidence must contain a non-empty JSON value";
const TRUSTED_SESSION_STARTUP_OPTIONS: &str = "-csearch_path=pg_catalog -cTimeZone=+05:00";

/// Connected and exact-current-authority-verified PostgreSQL product-state store.
#[derive(Clone)]
pub struct PostgresStore {
	pool: Arc<Pool>,
	#[cfg(unix)]
	connector: VerifiedSocketConnect,
	configured_schema_owner_role: Arc<str>,
	configured_runtime_role: Arc<str>,
}
impl PostgresStore {
	/// Return the closed execution-path query and allowed function identities for fixtures.
	#[cfg(feature = "test-support")]
	#[doc(hidden)]
	pub fn execution_path_contract_fixture() -> (&'static str, Vec<&'static str>) {
		authority::execution_path_contract_fixture()
	}

	/// Apply the production trusted-session invariants to an isolated raw fixture.
	#[cfg(feature = "test-support")]
	#[doc(hidden)]
	pub fn apply_trusted_session_invariants_fixture(config: &mut Config) {
		apply_trusted_session_invariants(config);
	}

	/// Connect only the configured least-privilege runtime identity and verify the exact
	/// current catalog and authority. This path executes no DDL.
	pub async fn connect_runtime_explicit(
		config: &PostgresConnectionConfig,
		runtime_password: Option<&str>,
	) -> Result<Self, StoreError> {
		let runtime = connection_config(config, config.runtime(), runtime_password);

		Self::connect_runtime(runtime, config.expected_peer_uid()).await
	}

	/// Create the one latest schema on an empty target through an explicit schema-owner
	/// identity. The schema transaction also records the initial process execution epoch and
	/// verifies the exact resulting catalog and configured runtime authority before commit.
	#[cfg(unix)]
	pub async fn bootstrap_latest_schema_explicit(
		config: &PostgresConnectionConfig,
		schema_owner: &PostgresIdentityConfig,
		schema_owner_password: Option<&str>,
		authorization: &ProcessExecutionAuthorization,
	) -> Result<(), LatestSchemaBootstrapFailure> {
		if schema_owner.user() == config.runtime().user() {
			return Err(LatestSchemaBootstrapFailure::from(StoreError::UnsafeAuthority(
				"schema-owner and runtime PostgreSQL identities must be distinct",
			)));
		}

		let owner = connection_config(config, schema_owner, schema_owner_password);
		schema::bootstrap_latest_schema(
			owner,
			config.expected_peer_uid(),
			schema_owner.user(),
			config.runtime().user(),
			authorization,
		)
		.await
	}

	#[cfg(not(unix))]
	pub async fn bootstrap_latest_schema_explicit(
		_config: &PostgresConnectionConfig,
		_schema_owner: &PostgresIdentityConfig,
		_schema_owner_password: Option<&str>,
		_authorization: &ProcessExecutionAuthorization,
	) -> Result<(), LatestSchemaBootstrapFailure> {
		Err(LatestSchemaBootstrapFailure::from(StoreError::Incompatible(
			"PostgreSQL Unix sockets require a Unix host".into(),
		)))
	}

	/// Connect one explicit runtime identity to one verified Unix-socket endpoint and
	/// execute no schema mutation.
	#[cfg(unix)]
	async fn connect_runtime(runtime: Config, expected_peer_uid: u32) -> Result<Self, StoreError> {
		Self::connect_runtime_with_pool_size(runtime, expected_peer_uid, 32).await
	}

	/// Construct a single-connection runtime pool for cross-adapter contention fixtures.
	#[cfg(all(unix, feature = "test-support"))]
	#[doc(hidden)]
	pub async fn connect_runtime_fixture(
		runtime: Config,
		expected_peer_uid: u32,
	) -> Result<Self, StoreError> {
		Self::connect_runtime_with_pool_size(runtime, expected_peer_uid, 1).await
	}

	#[cfg(unix)]
	async fn connect_runtime_with_pool_size(
		mut runtime: Config,
		expected_peer_uid: u32,
		pool_size: usize,
	) -> Result<Self, StoreError> {
		validate_connection(&runtime)?;
		let configured_runtime_role = Arc::<str>::from(
			runtime.get_user().ok_or(StoreError::InvalidInput("runtime role is absent"))?,
		);
		let connector = verified_socket_connect(&runtime, expected_peer_uid)?;

		apply_trusted_session_invariants(&mut runtime);

		let manager = Manager::from_connect(
			runtime,
			connector.clone(),
			ManagerConfig { recycling_method: RecyclingMethod::Fast },
		);
		let pool = Pool::builder(manager).max_size(pool_size).build()?;
		let client = checkout(&pool, &connector).await?;
		schema::verify_platform(&**client).await?;
		let configured_schema_owner_role = current_schema_owner(&client).await?;

		authority::verify_runtime(
			&**client,
			&configured_schema_owner_role,
			&configured_runtime_role,
		)
		.await?;

		drop(client);

		if pool_size > 1 {
			let first = checkout(&pool, &connector).await?;
			let second = checkout(&pool, &connector).await?;

			drop((first, second));
		}

		Ok(Self {
			pool: Arc::new(pool),
			connector,
			configured_schema_owner_role,
			configured_runtime_role,
		})
	}

	#[cfg(not(unix))]
	async fn connect_runtime(
		_runtime: Config,
		_expected_peer_uid: u32,
	) -> Result<Self, StoreError> {
		Err(StoreError::Incompatible("PostgreSQL Unix sockets require a Unix host".into()))
	}

	/// Close the bounded pool. Existing checked-out connections finish before closure.
	pub fn close(&self) {
		self.pool.close();
	}

	/// Revalidate the retained endpoint, live runtime session, exact current catalog, and
	/// least-privilege authority without resolving schema-owner credentials or running DDL.
	#[cfg(unix)]
	pub async fn revalidate(&self) -> Result<(), StoreError> {
		let client = checkout(&self.pool, &self.connector).await?;

		client.simple_query("SELECT 1").await?;
		schema::verify_platform(&**client).await?;

		authority::verify_runtime(
			&**client,
			&self.configured_schema_owner_role,
			&self.configured_runtime_role,
		)
		.await?;

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

fn apply_trusted_session_invariants(config: &mut Config) {
	// Startup-packet options are applied by PostgreSQL before it parses the first query.
	// Replacing caller-provided options prevents role/database defaults from influencing
	// identity, schema, time-zone rendering, or configured-authority verification.
	config.options(TRUSTED_SESSION_STARTUP_OPTIONS);
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

fn is_meaningful_evidence(value: &Value) -> bool {
	ensure_meaningful_evidence(value).is_ok()
}

#[cfg(unix)]
async fn current_schema_owner(client: &Client) -> Result<Arc<str>, StoreError> {
	let owner = client
		.query_opt(
			"SELECT owner.rolname::text \
			 FROM pg_catalog.pg_namespace AS namespace \
			 JOIN pg_catalog.pg_roles AS owner ON owner.oid=namespace.nspowner \
			 WHERE namespace.nspname='decodex'",
			&[],
		)
		.await?
		.ok_or_else(|| StoreError::Incompatible("PostgreSQL Decodex schema is absent".into()))?
		.get::<_, String>(0);
	let runtime = client.query_one("SELECT current_user::text", &[]).await?.get::<_, String>(0);

	if owner == runtime {
		return Err(StoreError::UnsafeAuthority(
			"runtime PostgreSQL identity owns the Decodex schema",
		));
	}

	Ok(Arc::from(owner))
}

#[cfg(unix)]
async fn checkout(pool: &Pool, connector: &VerifiedSocketConnect) -> Result<Client, StoreError> {
	connector.verify()?;

	pool.get().await.map_err(StoreError::Pool)
}

#[cfg(all(test, feature = "test-support"))]
mod launch_gate_tests {
	use std::env;

	use tokio_postgres::{Config, NoTls};

	use super::{
		account_lifecycle, account_profiles, apply_trusted_session_invariants, continuations,
		conversations, process_generations, provider_attempts, quick_task_routing, reset_cards,
		routing, routing_decisions, runtime_sessions,
	};

	#[tokio::test]
	async fn changed_adapter_sql_prepares_against_current_authority()
	-> Result<(), Box<dyn std::error::Error>> {
		let database_url = match env::var("DECODEX_TEST_RUNTIME_DATABASE_URL") {
			Ok(database_url) => database_url,
			Err(env::VarError::NotPresent) => return Ok(()),
			Err(error) => return Err(error.into()),
		};
		let mut config: Config = database_url.parse()?;
		apply_trusted_session_invariants(&mut config);
		let (client, connection) = config.connect(NoTls).await?;
		let connection_task = tokio::spawn(connection);

		let prepared = [
			account_lifecycle::prepare_account_lifecycle_sql(&client).await?,
			account_profiles::prepare_account_profile_sql(&client).await?,
			process_generations::prepare_account_bound_process_generation_sql(&client).await?,
			reset_cards::prepare_account_bound_reset_card_sql(&client).await?,
			conversations::prepare_conversation_admission_sql(&client).await?,
			quick_task_routing::prepare_quick_task_routing_sql(&client).await?,
			routing::prepare_routing_decision_sql(&client).await?,
			routing_decisions::prepare_route_account_sql(&client).await?,
			continuations::prepare_continuation_plan_sql(&client).await?,
			provider_attempts::prepare_provider_attempt_sql(&client).await?,
			runtime_sessions::prepare_runtime_session_thread_establishment_sql(&client).await?,
		];
		assert!(prepared.iter().all(|count| *count > 0));

		drop(client);
		connection_task.await??;
		Ok(())
	}
}
