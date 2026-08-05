//! Typed fail-closed daemon bootstrap over the configuration and adapter owners.

use std::{
	env,
	fmt::{Display, Formatter},
	fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	sync::Arc,
	time::Duration,
};

#[cfg(target_os = "macos")] use crate::host_credentials::MacosKeychainCredentialStore;
use crate::{
	BoundServer, ProtocolServer, ServerConfig, ServerError,
	account_api::AccountApiRuntime,
	account_launch::{ApiResetCardRuntime, AttestedAppServerProfile, RunnerCapacity},
	account_observation::AccountObservationService,
	account_profile::AccountProfileRuntime,
	account_service::{AccountService, OpenAiCredentialRefresher},
	application::{ProductStore, ProductStoreUnavailableReason, ServiceApplication},
	managed_repository_runtime::{
		ManagedRepositoryCapability, ManagedRepositoryReadiness, ManagedRepositoryRuntime,
		ManagedRepositoryUnavailableReason,
	},
	process_supervisor::{
		ProcessGenerationControl, ProcessGenerationReadiness, ProcessSupervisorError,
	},
	provider_attempt_service::{ProviderAttemptControl, ProviderAttemptReadiness},
	quick_task::{QuickTaskCapability, QuickTaskReadiness, QuickTaskRuntime},
};
use decodex_codex::CodexAdapter;
use decodex_core::{
	Availability, BlobStore, ConfigError, DecodexConfig, DecodexPaths, DecodexRoot, PathError,
	PostgresIdentityConfig, ProcessExecutionAuthorization, ProductState as _, ServerIdentity,
	ServerProfile,
};
use decodex_postgres::{BOOTSTRAP_AUTHORITY_REPORT_PREFIX, BootstrapFailure, PostgresStore};
use decodex_protocol::{
	AppServerCapability, CURRENT_VERSION, DoctorCheck, DoctorComponent, DoctorIssue, DoctorReport,
	DoctorStatus, LocalTransportAuthority, LocalTransportListener, LocalTransportRefusal,
	QuickTaskUnavailableReason, ServerId,
};

const ACCOUNT_CALLBACK_ATTESTATION_TIMEOUT: Duration = Duration::from_secs(30);

/// Credential-negative failure from the explicit empty-target latest-schema bootstrap command.
#[derive(Clone, Eq, PartialEq)]
pub enum LatestSchemaBootstrapError {
	/// The platform root or typed configuration is unavailable or not local.
	Configuration,
	/// The explicit schema-owner credential is unavailable.
	Authentication,
	/// The owner-only process execution authorization is unavailable.
	ExecutionAuthorization,
	/// PostgreSQL rejected the empty-target schema transaction or its authority proof.
	Database {
		/// Stable value-free failure class used by operator output and callers.
		failure: BootstrapFailure,
		/// Optional bounded canonical post-schema report; no credential-bearing values.
		report_json: Option<String>,
	},
}
impl LatestSchemaBootstrapError {
	/// Return the one bounded canonical report line emitted only by the hidden operator command.
	#[doc(hidden)]
	pub fn authority_report_line(&self) -> Option<String> {
		match self {
			Self::Database { report_json: Some(report), .. } =>
				Some(format!("{BOOTSTRAP_AUTHORITY_REPORT_PREFIX}{report}")),
			_ => None,
		}
	}
}
impl std::fmt::Debug for LatestSchemaBootstrapError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Configuration => formatter.write_str("Configuration"),
			Self::Authentication => formatter.write_str("Authentication"),
			Self::ExecutionAuthorization => formatter.write_str("ExecutionAuthorization"),
			Self::Database { failure, .. } =>
				formatter.debug_tuple("Database").field(failure).finish(),
		}
	}
}
impl Display for LatestSchemaBootstrapError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Configuration =>
				formatter.write_str("latest-schema bootstrap configuration is unavailable"),
			Self::Authentication =>
				formatter.write_str("latest-schema bootstrap authentication is unavailable"),
			Self::ExecutionAuthorization =>
				formatter.write_str("local process execution authorization is unavailable"),
			Self::Database { failure: BootstrapFailure::Authentication, .. } =>
				formatter.write_str("latest-schema bootstrap authentication failed"),
			Self::Database { failure: BootstrapFailure::Unreachable, .. } =>
				formatter.write_str("latest-schema bootstrap database is unreachable"),
			Self::Database { failure: BootstrapFailure::Incompatible, .. } =>
				formatter.write_str("latest-schema bootstrap target is incompatible"),
			Self::Database { failure: BootstrapFailure::UnsafeAuthority, .. } =>
				formatter.write_str("latest-schema bootstrap authority is unsafe"),
			Self::Database { failure: BootstrapFailure::UnsafeHostPath, .. } =>
				formatter.write_str("latest-schema bootstrap database path is unsafe"),
		}
	}
}
impl std::error::Error for LatestSchemaBootstrapError {}

/// Value-free failure from the explicit read-only current-authority command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CurrentAuthorityValidationError {
	/// Typed local PostgreSQL configuration is unavailable.
	Configuration,
	/// The explicit runtime database credential is unavailable.
	Authentication,
	/// PostgreSQL rejected the read-only current-authority proof.
	Database(BootstrapFailure),
}
impl Display for CurrentAuthorityValidationError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Configuration =>
				formatter.write_str("current PostgreSQL authority configuration is unavailable"),
			Self::Authentication =>
				formatter.write_str("current PostgreSQL authority authentication is unavailable"),
			Self::Database(BootstrapFailure::Authentication) =>
				formatter.write_str("current PostgreSQL authority authentication failed"),
			Self::Database(BootstrapFailure::Unreachable) =>
				formatter.write_str("current PostgreSQL authority is unreachable"),
			Self::Database(BootstrapFailure::Incompatible) =>
				formatter.write_str("current PostgreSQL latest schema is incompatible"),
			Self::Database(BootstrapFailure::UnsafeAuthority) =>
				formatter.write_str("current PostgreSQL authority is unsafe"),
			Self::Database(BootstrapFailure::UnsafeHostPath) =>
				formatter.write_str("current PostgreSQL authority path is unsafe"),
		}
	}
}
impl std::error::Error for CurrentAuthorityValidationError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RuntimeConnectionFailure {
	Authentication,
	Database(BootstrapFailure),
}

/// Complete daemon bootstrap under one already-acquired singleton capability.
pub struct ServiceBootstrap {
	server_id: ServerId,
	store: ProductStore,
	managed_repositories: ManagedRepositoryCapability,
	process_generations: Option<ProcessGenerationControl>,
	process_generation_readiness: ProcessGenerationReadiness,
	provider_attempts: Option<ProviderAttemptControl>,
	provider_attempt_readiness: ProviderAttemptReadiness,
	blob_store: Option<BlobStore>,
	accounts: Option<Arc<AccountService>>,
	account_profiles: Option<AccountProfileRuntime>,
	account_api: Option<Arc<AccountApiRuntime>>,
	reset_cards: Option<ApiResetCardRuntime>,
	quick_tasks: QuickTaskCapability,
	doctor: DoctorReport,
	// Keep the one acquired daemon capability last so an unbound bootstrap
	// releases it after every mutation service and application dependency.
	daemon_authority: Result<LocalTransportListener, LocalTransportRefusal>,
}
impl ServiceBootstrap {
	/// Stable server-host identity used by welcome, pinning, and doctor readback.
	pub fn server_id(&self) -> &ServerId {
		&self.server_id
	}

	/// Authoritative bounded report assembled during bootstrap.
	pub fn doctor(&self) -> &DoctorReport {
		&self.doctor
	}

	/// Product-state availability retained by the daemon application owner.
	pub fn product_state_availability(&self) -> Availability {
		self.store.availability()
	}

	/// Managed-repository readiness after executor verification and bounded restart reconciliation.
	pub const fn managed_repository_readiness(&self) -> ManagedRepositoryReadiness {
		self.managed_repositories.readiness()
	}

	/// Return the independent immutable Quick Task startup projection.
	pub const fn quick_task_readiness(&self) -> QuickTaskReadiness {
		self.quick_tasks.readiness()
	}

	/// Return the independent ProcessGeneration service readiness.
	pub const fn process_generation_readiness(&self) -> ProcessGenerationReadiness {
		self.process_generation_readiness
	}

	/// Borrow the exact diagnostic/reconciliation port while this owner retains authority.
	pub const fn process_generation_control(&self) -> Option<&ProcessGenerationControl> {
		self.process_generations.as_ref()
	}

	/// Return the independent ProviderAttempt restore and reconciliation readiness.
	pub const fn provider_attempt_readiness(&self) -> ProviderAttemptReadiness {
		self.provider_attempt_readiness
	}

	/// Borrow the bounded positive-reconciliation port while this owner retains authority.
	pub const fn provider_attempt_control(&self) -> Option<&ProviderAttemptControl> {
		self.provider_attempts.as_ref()
	}

	/// Move the already-owned same-UID listener into the server lifecycle.
	pub async fn bind(self, config: ServerConfig) -> Result<BoundServer, ServerError> {
		let Self {
			server_id,
			store,
			managed_repositories,
			process_generations,
			process_generation_readiness: _,
			provider_attempts,
			provider_attempt_readiness: _,
			blob_store,
			accounts,
			account_profiles,
			account_api,
			reset_cards,
			quick_tasks,
			doctor,
			daemon_authority,
		} = self;
		let listener = daemon_authority.map_err(ServerError::LocalTransport)?;
		let account_observations = match (&accounts, &account_api) {
			(Some(accounts), Some(account_api))
				if account_profiles.is_some() || reset_cards.is_some() =>
				Some(AccountObservationService::new(
					Arc::clone(accounts),
					Some(Arc::clone(account_api)),
					account_profiles.clone(),
					reset_cards.clone(),
				)),
			_ => None,
		};
		let server = ProtocolServer::new(
			server_id,
			ServiceApplication::new(
				store,
				managed_repositories,
				process_generations,
				provider_attempts,
				CodexAdapter::unavailable(),
				blob_store,
				quick_tasks,
				doctor,
			)
			.with_accounts(accounts)
			.with_reset_cards(reset_cards)
			.with_account_observations(account_observations),
			config,
		);
		Ok(server.bind_listener(listener))
	}
}

struct DoctorInputs {
	configuration: DoctorStatus,
	product_store: DoctorStatus,
	quick_task: DoctorStatus,
	server_identity: DoctorStatus,
	shared_home: DoctorStatus,
	managed_repository: DoctorStatus,
	blob_integrity: DoctorStatus,
	vault: DoctorStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HostDirectoryError {
	Missing,
	Unsafe,
}

pub(crate) async fn bootstrap_default() -> ServiceBootstrap {
	match DecodexRoot::platform_default() {
		Ok(root) => bootstrap(root).await,
		Err(_) => bootstrap_without_root(DoctorIssue::UnsafeHostPath),
	}
}

pub(crate) async fn bootstrap(root: DecodexRoot) -> ServiceBootstrap {
	let paths = root.paths();
	let loaded = DecodexConfig::load(&paths);
	let configured_transport =
		loaded.as_ref().map_or(Err(LocalTransportRefusal::ConfigurationUnavailable), |config| {
			match config.active_profile() {
				ServerProfile::Local(profile) => LocalTransportAuthority::new(
					paths.clone(),
					profile.policy(),
					profile.service_owner_uid(),
				),
				ServerProfile::Remote(_) => Err(LocalTransportRefusal::Disabled),
			}
		});
	let config_status = match &loaded {
		Ok(_) => DoctorStatus::Ready,
		Err(error) => DoctorStatus::Unavailable(config_issue(*error)),
	};
	let listener = match configured_transport {
		Ok(authority) => match authority.bind().await {
			Ok(listener) => listener,
			Err(refusal) => {
				return bootstrap_without_authority(
					refusal,
					config_status,
					DoctorStatus::Unknown(DoctorIssue::NotProbed),
				);
			},
		},
		Err(refusal) => {
			return bootstrap_without_authority(
				refusal,
				config_status,
				loaded.as_ref().map_or_else(
					|error| DoctorStatus::Unavailable(database_config_issue(*error)),
					|_| DoctorStatus::Unknown(DoctorIssue::NotProbed),
				),
			);
		},
	};

	bootstrap_with_authority(paths, loaded, config_status, listener).await
}

async fn bootstrap_managed_repositories(
	postgres: Option<PostgresStore>,
) -> ManagedRepositoryCapability {
	match postgres {
		Some(postgres) => match ManagedRepositoryRuntime::start(postgres).await {
			Ok(Some(runtime)) => ManagedRepositoryCapability::Ready { _runtime: runtime },
			Ok(None) => ManagedRepositoryCapability::Disabled,
			Err(error) => ManagedRepositoryCapability::startup_failed(error),
		},
		None => ManagedRepositoryCapability::unavailable(
			ManagedRepositoryUnavailableReason::ProductStore,
		),
	}
}

#[cfg(target_os = "macos")]
struct QuickTaskComposition {
	postgres: Option<PostgresStore>,
	blob_store: Option<BlobStore>,
	accounts: Option<Arc<AccountService>>,
	process_generations: Option<ProcessGenerationControl>,
	provider_attempts: Option<ProviderAttemptControl>,
	execution_authorization: Option<ProcessExecutionAuthorization>,
	launch_profile: Option<AttestedAppServerProfile>,
}

#[cfg(target_os = "macos")]
fn compose_quick_tasks(composition: QuickTaskComposition) -> QuickTaskCapability {
	let QuickTaskComposition {
		postgres,
		blob_store,
		accounts,
		process_generations,
		provider_attempts,
		execution_authorization,
		launch_profile,
	} = composition;
	match (
		postgres,
		blob_store,
		accounts,
		process_generations,
		provider_attempts,
		execution_authorization,
		launch_profile,
	) {
		(None, _, _, _, _, _, _) =>
			QuickTaskCapability::Unavailable(QuickTaskUnavailableReason::ProductState),
		(_, None, _, _, _, _, _) =>
			QuickTaskCapability::Unavailable(QuickTaskUnavailableReason::BlobStore),
		(_, _, None, _, _, _, _) =>
			QuickTaskCapability::Unavailable(QuickTaskUnavailableReason::AccountService),
		(_, _, _, None, _, _, _) =>
			QuickTaskCapability::Unavailable(QuickTaskUnavailableReason::ProcessGeneration),
		(_, _, _, _, None, _, _) =>
			QuickTaskCapability::Unavailable(QuickTaskUnavailableReason::ProviderAttempt),
		(_, _, _, _, _, None, _) =>
			QuickTaskCapability::Unavailable(QuickTaskUnavailableReason::ExecutionAuthorization),
		(_, _, _, _, _, _, None) =>
			QuickTaskCapability::Unavailable(QuickTaskUnavailableReason::AppServerProfile),
		(
			Some(postgres),
			Some(blob_store),
			Some(accounts),
			Some(process_generations),
			Some(provider_attempts),
			Some(execution_authorization),
			Some(launch_profile),
		) => match RunnerCapacity::daemon() {
			Ok(capacity) => QuickTaskCapability::Ready(QuickTaskRuntime::new(
				postgres,
				blob_store,
				accounts,
				process_generations,
				provider_attempts,
				execution_authorization,
				launch_profile,
				capacity,
			)),
			Err(_) => QuickTaskCapability::Unavailable(QuickTaskUnavailableReason::RunnerCapacity),
		},
	}
}

async fn bootstrap_with_authority(
	paths: DecodexPaths,
	loaded: Result<DecodexConfig, ConfigError>,
	config_status: DoctorStatus,
	listener: LocalTransportListener,
) -> ServiceBootstrap {
	let identity = ServerIdentity::load_or_create(&paths);
	let (server_id, identity_status) = server_identity(identity);
	let blob_store = BlobStore::open(paths.clone());
	#[cfg(target_os = "macos")]
	let quick_task_blob_store = match &blob_store {
		Ok(store) => Some(store.clone()),
		Err(_) => None,
	};
	let blob_integrity = match &blob_store {
		Ok(_) => DoctorStatus::Unknown(DoctorIssue::NotProbed),
		Err(_) => DoctorStatus::Unavailable(DoctorIssue::Integrity),
	};
	let shared_home = shared_codex_home();
	let (store, product_store, mut vault) = match loaded.as_ref() {
		Ok(config) => connect_database(config).await,
		Err(error) => (
			ProductStore::Unavailable(ProductStoreUnavailableReason::Configuration),
			DoctorStatus::Unavailable(database_config_issue(*error)),
			DoctorStatus::Unknown(DoctorIssue::Authentication),
		),
	};
	let postgres = match &store {
		ProductStore::Available(postgres) => Some(postgres.clone()),
		ProductStore::Unavailable(_) => None,
	};
	let (process_generations, process_generation_readiness) = match postgres.clone() {
		Some(postgres) => match ProcessGenerationControl::start(postgres).await {
			Ok(control) => (Some(control), ProcessGenerationReadiness::Ready),
			Err(ProcessSupervisorError::Platform) =>
				(None, ProcessGenerationReadiness::PlatformUnavailable),
			Err(_) => (None, ProcessGenerationReadiness::ProductStateUnavailable),
		},
		None => (None, ProcessGenerationReadiness::ProductStateUnavailable),
	};
	#[cfg(target_os = "macos")]
	let process_execution_authorization = ProcessExecutionAuthorization::load(&paths).ok();
	let (provider_attempts, provider_attempt_readiness) = match postgres.clone() {
		Some(postgres) => match ProviderAttemptControl::start(postgres).await {
			Ok(control) => (Some(control), ProviderAttemptReadiness::Ready),
			Err(_) => (None, ProviderAttemptReadiness::ProductStateUnavailable),
		},
		None => (None, ProviderAttemptReadiness::ProductStateUnavailable),
	};
	let managed_repositories = bootstrap_managed_repositories(postgres.clone()).await;
	let managed_repository = managed_repository_doctor(&managed_repositories);
	#[cfg(target_os = "macos")]
	let (accounts, account_profiles, account_api, reset_cards, quick_task_launch_profile) =
		match postgres.clone() {
			Some(postgres) => {
				let (service, profiles, api, runtime, launch_profile, status) =
					bootstrap_macos_account_runtime(
						postgres,
						&paths,
						process_generations.clone(),
						process_execution_authorization.clone(),
					)
					.await;
				vault = status;
				(service, profiles, api, runtime, launch_profile)
			},
			None => (None, None, None, None, None),
		};
	#[cfg(not(target_os = "macos"))]
	let (accounts, account_profiles, account_api, reset_cards) = {
		vault = DoctorStatus::Unavailable(DoctorIssue::Authentication);
		(None, None, None, None)
	};
	#[cfg(target_os = "macos")]
	let quick_tasks = compose_quick_tasks(QuickTaskComposition {
		postgres,
		blob_store: quick_task_blob_store,
		accounts: accounts.as_ref().map(Arc::clone),
		process_generations: process_generations.clone(),
		provider_attempts: provider_attempts.clone(),
		execution_authorization: process_execution_authorization,
		launch_profile: quick_task_launch_profile,
	});
	#[cfg(not(target_os = "macos"))]
	let quick_tasks =
		QuickTaskCapability::Unavailable(QuickTaskUnavailableReason::UnsupportedPlatform);
	let quick_task = quick_task_doctor(&quick_tasks);
	let doctor = doctor_report(
		server_id.clone(),
		DoctorInputs {
			configuration: config_status,
			product_store,
			quick_task,
			server_identity: identity_status,
			shared_home,
			managed_repository,
			blob_integrity,
			vault,
		},
	);

	ServiceBootstrap {
		server_id,
		store,
		managed_repositories,
		process_generations,
		process_generation_readiness,
		provider_attempts,
		provider_attempt_readiness,
		blob_store: blob_store.ok(),
		accounts,
		account_profiles,
		account_api,
		reset_cards,
		quick_tasks,
		doctor,
		daemon_authority: Ok(listener),
	}
}

#[cfg(target_os = "macos")]
type MacosAccountRuntimeBootstrap = (
	Option<Arc<AccountService>>,
	Option<AccountProfileRuntime>,
	Option<Arc<AccountApiRuntime>>,
	Option<ApiResetCardRuntime>,
	Option<AttestedAppServerProfile>,
	DoctorStatus,
);

#[cfg(target_os = "macos")]
struct MacosAccountServiceComposition {
	service: Arc<AccountService>,
	account_profiles: Option<AccountProfileRuntime>,
}

#[cfg(target_os = "macos")]
async fn compose_macos_account_service(
	postgres: &PostgresStore,
	paths: &DecodexPaths,
) -> Result<MacosAccountServiceComposition, DoctorIssue> {
	let refresher = match tokio::task::spawn_blocking(OpenAiCredentialRefresher::new).await {
		Ok(Ok(refresher)) => refresher,
		Ok(Err(_)) => return Err(DoctorIssue::Authentication),
		Err(_) => return Err(DoctorIssue::Integrity),
	};
	let credentials =
		MacosKeychainCredentialStore::new(paths).map_err(|_| DoctorIssue::Integrity)?;
	let credentials: Arc<dyn crate::HostCredentialStore> = Arc::new(credentials);
	let account_profiles =
		Some(AccountProfileRuntime::new(postgres.clone(), Arc::clone(&credentials)));
	let service = Arc::new(AccountService::new(postgres.clone(), credentials, Arc::new(refresher)));
	Ok(MacosAccountServiceComposition { service, account_profiles })
}

#[cfg(target_os = "macos")]
fn unavailable_macos_account_runtime(
	service: Option<Arc<AccountService>>,
	account_profiles: Option<AccountProfileRuntime>,
	issue: DoctorIssue,
) -> MacosAccountRuntimeBootstrap {
	(service, account_profiles, None, None, None, DoctorStatus::Unavailable(issue))
}

#[cfg(target_os = "macos")]
async fn bootstrap_macos_account_runtime(
	_postgres: PostgresStore,
	paths: &DecodexPaths,
	_process_generations: Option<ProcessGenerationControl>,
	_execution_authorization: Option<ProcessExecutionAuthorization>,
) -> MacosAccountRuntimeBootstrap {
	let MacosAccountServiceComposition { service, account_profiles } =
		match compose_macos_account_service(&_postgres, paths).await {
			Ok(composition) => composition,
			Err(issue) => return unavailable_macos_account_runtime(None, None, issue),
		};
	let _ = service.reconcile_startup().await;
	// Quick Tasks may still use the optional Codex process adapter, but account health no longer
	// depends on its executable, callback, schema, or version.  Failure here only disables that
	// separate capability.
	let quick_task_launch_profile = AttestedAppServerProfile::attest(
		paths.root().as_path().to_owned(),
		ACCOUNT_CALLBACK_ATTESTATION_TIMEOUT,
	)
	.ok();
	if let Some(profile) = &quick_task_launch_profile {
		let _ = service.attest_callback_capability(profile.account_callback_attestation()).await;
	}
	let api = AccountApiRuntime::new(Arc::clone(&service)).ok().map(Arc::new);
	let status = match &api {
		Some(_) => DoctorStatus::Ready,
		None => DoctorStatus::Unavailable(DoctorIssue::Authentication),
	};
	let reset_cards = api.as_ref().and_then(|api| {
		ApiResetCardRuntime::start(_postgres, Arc::clone(&service), Arc::clone(api)).ok()
	});
	(Some(service), account_profiles, api, reset_cards, quick_task_launch_profile, status)
}

fn bootstrap_without_authority(
	refusal: LocalTransportRefusal,
	configuration: DoctorStatus,
	product_store: DoctorStatus,
) -> ServiceBootstrap {
	let server_id = unavailable_server_id();
	let managed_repositories =
		ManagedRepositoryCapability::unavailable(ManagedRepositoryUnavailableReason::ProductStore);
	let quick_tasks = QuickTaskCapability::Unavailable(QuickTaskUnavailableReason::ProductState);
	let doctor = doctor_report(
		server_id.clone(),
		DoctorInputs {
			configuration,
			product_store,
			quick_task: quick_task_doctor(&quick_tasks),
			server_identity: DoctorStatus::Unknown(DoctorIssue::NotProbed),
			shared_home: DoctorStatus::Unknown(DoctorIssue::NotProbed),
			managed_repository: managed_repository_doctor(&managed_repositories),
			blob_integrity: DoctorStatus::Unknown(DoctorIssue::NotProbed),
			vault: DoctorStatus::Unknown(DoctorIssue::NotProbed),
		},
	);

	ServiceBootstrap {
		server_id,
		store: ProductStore::Unavailable(ProductStoreUnavailableReason::Configuration),
		managed_repositories,
		process_generations: None,
		process_generation_readiness: ProcessGenerationReadiness::ProductStateUnavailable,
		provider_attempts: None,
		provider_attempt_readiness: ProviderAttemptReadiness::ProductStateUnavailable,
		blob_store: None,
		accounts: None,
		account_profiles: None,
		account_api: None,
		reset_cards: None,
		quick_tasks,
		doctor,
		daemon_authority: Err(refusal),
	}
}

fn bootstrap_without_root(issue: DoctorIssue) -> ServiceBootstrap {
	let server_id = unavailable_server_id();
	let managed_repositories =
		ManagedRepositoryCapability::unavailable(ManagedRepositoryUnavailableReason::ProductStore);
	let quick_tasks = QuickTaskCapability::Unavailable(QuickTaskUnavailableReason::ProductState);
	let doctor = doctor_report(
		server_id.clone(),
		DoctorInputs {
			configuration: DoctorStatus::Unavailable(issue),
			product_store: DoctorStatus::Unavailable(DoctorIssue::DatabaseNotConfigured),
			quick_task: quick_task_doctor(&quick_tasks),
			server_identity: DoctorStatus::Unavailable(DoctorIssue::ServerIdentityUnavailable),
			shared_home: DoctorStatus::Unknown(DoctorIssue::NotProbed),
			managed_repository: managed_repository_doctor(&managed_repositories),
			blob_integrity: DoctorStatus::Unavailable(DoctorIssue::Integrity),
			vault: DoctorStatus::Unknown(DoctorIssue::Authentication),
		},
	);

	ServiceBootstrap {
		server_id,
		store: ProductStore::Unavailable(ProductStoreUnavailableReason::Configuration),
		managed_repositories,
		process_generations: None,
		process_generation_readiness: ProcessGenerationReadiness::ProductStateUnavailable,
		provider_attempts: None,
		provider_attempt_readiness: ProviderAttemptReadiness::ProductStateUnavailable,
		blob_store: None,
		accounts: None,
		account_profiles: None,
		account_api: None,
		reset_cards: None,
		quick_tasks,
		doctor,
		daemon_authority: Err(LocalTransportRefusal::ConfigurationUnavailable),
	}
}

fn managed_repository_doctor(capability: &ManagedRepositoryCapability) -> DoctorStatus {
	match capability.readiness() {
		ManagedRepositoryReadiness::Ready => DoctorStatus::Ready,
		ManagedRepositoryReadiness::Disabled => DoctorStatus::Unavailable(DoctorIssue::Disabled),
		ManagedRepositoryReadiness::Unavailable(
			ManagedRepositoryUnavailableReason::ProductStore,
		) => DoctorStatus::Unavailable(DoctorIssue::DatabaseNotConfigured),
		ManagedRepositoryReadiness::Unavailable(
			ManagedRepositoryUnavailableReason::Executor
			| ManagedRepositoryUnavailableReason::Reconciliation
			| ManagedRepositoryUnavailableReason::RestartWorkResidual,
		) => DoctorStatus::Unavailable(DoctorIssue::Integrity),
	}
}

fn quick_task_doctor(capability: &QuickTaskCapability) -> DoctorStatus {
	match capability.readiness() {
		QuickTaskReadiness::Ready => DoctorStatus::Ready,
		QuickTaskReadiness::Unavailable(QuickTaskUnavailableReason::ProductState) =>
			DoctorStatus::Unavailable(DoctorIssue::DatabaseNotConfigured),
		QuickTaskReadiness::Unavailable(
			QuickTaskUnavailableReason::AccountService
			| QuickTaskUnavailableReason::ExecutionAuthorization,
		) => DoctorStatus::Unavailable(DoctorIssue::Authentication),
		QuickTaskReadiness::Unavailable(QuickTaskUnavailableReason::UnsupportedPlatform) =>
			DoctorStatus::Unavailable(DoctorIssue::Disabled),
		QuickTaskReadiness::Unavailable(
			QuickTaskUnavailableReason::BlobStore
			| QuickTaskUnavailableReason::ProcessGeneration
			| QuickTaskUnavailableReason::ProviderAttempt
			| QuickTaskUnavailableReason::AppServerProfile
			| QuickTaskUnavailableReason::RunnerCapacity,
		) => DoctorStatus::Unavailable(DoctorIssue::Integrity),
	}
}

fn doctor_report(server_id: ServerId, inputs: DoctorInputs) -> DoctorReport {
	let mut checks = vec![
		DoctorCheck::new(DoctorComponent::Configuration, inputs.configuration),
		DoctorCheck::new(DoctorComponent::ProductStore, inputs.product_store),
		DoctorCheck::new(DoctorComponent::QuickTask, inputs.quick_task),
		DoctorCheck::new(DoctorComponent::Protocol, DoctorStatus::Ready),
		DoctorCheck::new(DoctorComponent::ProtocolVersion, DoctorStatus::Ready),
		DoctorCheck::new(DoctorComponent::ServerIdentity, inputs.server_identity),
		DoctorCheck::new(DoctorComponent::SharedCodexHome, inputs.shared_home),
		DoctorCheck::new(DoctorComponent::ManagedRepository, inputs.managed_repository),
		DoctorCheck::new(DoctorComponent::BlobIntegrity, inputs.blob_integrity),
		DoctorCheck::new(DoctorComponent::CredentialVault, inputs.vault),
		DoctorCheck::new(
			DoctorComponent::PluginReadiness,
			DoctorStatus::Unknown(DoctorIssue::Plugin),
		),
	];

	checks.extend(AppServerCapability::ALL.into_iter().map(|capability| {
		DoctorCheck::new(
			DoctorComponent::AppServerCapability(capability),
			DoctorStatus::Unknown(DoctorIssue::NotProbed),
		)
	}));

	DoctorReport::new(server_id, CURRENT_VERSION, checks)
		.expect("the closed daemon doctor report is bounded and unique")
}

fn server_identity(identity: Result<ServerIdentity, ConfigError>) -> (ServerId, DoctorStatus) {
	match identity {
		Ok(identity) => (
			ServerId::new(identity.as_str()).expect("canonical UUID is a bounded server ID"),
			DoctorStatus::Ready,
		),
		Err(_) => (
			unavailable_server_id(),
			DoctorStatus::Unavailable(DoctorIssue::ServerIdentityUnavailable),
		),
	}
}

fn unavailable_server_id() -> ServerId {
	ServerIdentity::generate().map_or_else(
		|_| ServerId::new("server-identity-unavailable").expect("fallback marker is bounded"),
		|identity| ServerId::new(identity.as_str()).expect("canonical UUID is a bounded server ID"),
	)
}

fn shared_codex_home() -> DoctorStatus {
	let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) else {
		return DoctorStatus::Unknown(DoctorIssue::NotProbed);
	};
	let path = Path::new(&home).join(".codex");

	match host_directory(&path) {
		Ok(()) => DoctorStatus::Ready,
		Err(HostDirectoryError::Missing) => DoctorStatus::Unknown(DoctorIssue::NotProbed),
		Err(HostDirectoryError::Unsafe) => DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath),
	}
}

fn host_directory(path: &Path) -> Result<(), HostDirectoryError> {
	let mut current = PathBuf::new();

	for component in path.components() {
		current.push(component.as_os_str());

		match fs::symlink_metadata(&current) {
			Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {},
			Ok(_) => return Err(HostDirectoryError::Unsafe),
			Err(error) if error.kind() == ErrorKind::NotFound => {
				return Err(HostDirectoryError::Missing);
			},
			Err(_) => return Err(HostDirectoryError::Unsafe),
		}
	}

	Ok(())
}

fn config_issue(error: ConfigError) -> DoctorIssue {
	match error {
		ConfigError::UnsupportedVersion => DoctorIssue::ConfigurationVersion,
		ConfigError::InvalidPostgresHostPath => DoctorIssue::UnsafeHostPath,
		ConfigError::Path(PathError::Io { kind: ErrorKind::NotFound, .. }) =>
			DoctorIssue::ConfigurationMissing,
		ConfigError::Path(
			PathError::UnsafeRoot
			| PathError::CodexOwnedRoot
			| PathError::Escape
			| PathError::Symlink
			| PathError::UnexpectedDirectoryKind
			| PathError::UnexpectedFileKind
			| PathError::InsecurePermissions,
		) => DoctorIssue::UnsafeHostPath,
		_ => DoctorIssue::ConfigurationMalformed,
	}
}

fn database_config_issue(error: ConfigError) -> DoctorIssue {
	match config_issue(error) {
		DoctorIssue::ConfigurationMissing => DoctorIssue::DatabaseNotConfigured,
		DoctorIssue::UnsafeHostPath => DoctorIssue::UnsafeHostPath,
		_ => DoctorIssue::DatabaseMalformedConfig,
	}
}

pub(crate) fn credential(identity: &PostgresIdentityConfig) -> Result<Option<String>, ()> {
	match identity.credential_env_var() {
		Some(name) => match env::var(name) {
			Ok(value) if !value.is_empty() => Ok(Some(value)),
			_ => Err(()),
		},
		None => Ok(None),
	}
}

pub(crate) async fn bootstrap_latest_schema(
	root: DecodexRoot,
	schema_owner_user: String,
	schema_owner_credential_env_var: Option<String>,
) -> Result<(), LatestSchemaBootstrapError> {
	let paths = root.paths();
	let config =
		DecodexConfig::load(&paths).map_err(|_| LatestSchemaBootstrapError::Configuration)?;
	if !matches!(config.active_profile(), ServerProfile::Local(_)) {
		return Err(LatestSchemaBootstrapError::Configuration);
	}
	let schema_owner =
		PostgresIdentityConfig::new(schema_owner_user, schema_owner_credential_env_var)
			.map_err(|_| LatestSchemaBootstrapError::Configuration)?;
	let runtime = config.postgres().runtime();
	if schema_owner.user() == runtime.user()
		|| schema_owner.credential_env_var().is_some()
			&& schema_owner.credential_env_var() == runtime.credential_env_var()
	{
		return Err(LatestSchemaBootstrapError::Configuration);
	}
	let schema_owner_credential =
		credential(&schema_owner).map_err(|()| LatestSchemaBootstrapError::Authentication)?;
	let execution_authorization = ProcessExecutionAuthorization::load_or_create(&paths)
		.map_err(|_| LatestSchemaBootstrapError::ExecutionAuthorization)?;

	PostgresStore::bootstrap_latest_schema_explicit(
		config.postgres(),
		&schema_owner,
		schema_owner_credential.as_deref(),
		&execution_authorization,
	)
	.await
	.map_err(|error| LatestSchemaBootstrapError::Database {
		failure: error.bootstrap_failure(),
		report_json: error.report_json().map(str::to_owned),
	})
}

pub(crate) async fn validate_current_authority(
	root: DecodexRoot,
) -> Result<(), CurrentAuthorityValidationError> {
	let paths = root.paths();
	let config =
		DecodexConfig::load(&paths).map_err(|_| CurrentAuthorityValidationError::Configuration)?;
	if !matches!(config.active_profile(), ServerProfile::Local(_)) {
		return Err(CurrentAuthorityValidationError::Configuration);
	}
	let store = connect_runtime_store(&config).await.map_err(|error| match error {
		RuntimeConnectionFailure::Authentication => CurrentAuthorityValidationError::Authentication,
		RuntimeConnectionFailure::Database(error) =>
			CurrentAuthorityValidationError::Database(error),
	})?;
	store.close();
	Ok(())
}

async fn connect_runtime_store(
	config: &DecodexConfig,
) -> Result<PostgresStore, RuntimeConnectionFailure> {
	let postgres = config.postgres();
	let runtime_credential =
		credential(postgres.runtime()).map_err(|()| RuntimeConnectionFailure::Authentication)?;
	PostgresStore::connect_runtime_explicit(postgres, runtime_credential.as_deref())
		.await
		.map_err(|error| RuntimeConnectionFailure::Database(error.bootstrap_failure()))
}

async fn connect_database(config: &DecodexConfig) -> (ProductStore, DoctorStatus, DoctorStatus) {
	let vault = DoctorStatus::Unknown(DoctorIssue::NotProbed);
	match connect_runtime_store(config).await {
		Ok(store) => (ProductStore::Available(store), DoctorStatus::Ready, vault),
		Err(RuntimeConnectionFailure::Authentication) => (
			ProductStore::Unavailable(ProductStoreUnavailableReason::Authentication),
			DoctorStatus::Unavailable(DoctorIssue::Authentication),
			DoctorStatus::Unavailable(DoctorIssue::Authentication),
		),
		Err(RuntimeConnectionFailure::Database(error)) => {
			let (reason, issue, vault) = match error {
				BootstrapFailure::Authentication => (
					ProductStoreUnavailableReason::Authentication,
					DoctorIssue::Authentication,
					vault,
				),
				BootstrapFailure::Unreachable => (
					ProductStoreUnavailableReason::Unreachable,
					DoctorIssue::DatabaseUnreachable,
					DoctorStatus::Unknown(DoctorIssue::Authentication),
				),
				BootstrapFailure::Incompatible => (
					ProductStoreUnavailableReason::Incompatible,
					DoctorIssue::DatabaseIncompatible,
					vault,
				),
				BootstrapFailure::UnsafeAuthority => (
					ProductStoreUnavailableReason::UnsafeAuthority,
					DoctorIssue::UnsafeDatabaseAuthority,
					vault,
				),
				BootstrapFailure::UnsafeHostPath => (
					ProductStoreUnavailableReason::UnsafeHostPath,
					DoctorIssue::UnsafeHostPath,
					DoctorStatus::Unknown(DoctorIssue::Authentication),
				),
			};
			(ProductStore::Unavailable(reason), DoctorStatus::Unavailable(issue), vault)
		},
	}
}

#[cfg(test)]
mod tests {
	use crate::bootstrap;
	use decodex_postgres::BootstrapFailure;
	use decodex_protocol::{DoctorComponent, DoctorIssue, DoctorStatus};

	#[test]
	fn latest_schema_bootstrap_report_has_one_hidden_command_line() {
		let error = bootstrap::LatestSchemaBootstrapError::Database {
			failure: BootstrapFailure::Incompatible,
			report_json: Some("{\"schema\":\"decodex/bootstrap-authority-report/1\"}".into()),
		};
		assert_eq!(
			error.authority_report_line().as_deref(),
			Some(
				"DECODEX_BOOTSTRAP_AUTHORITY_REPORT={\"schema\":\"decodex/bootstrap-authority-report/1\"}"
			)
		);
		assert_eq!(error.to_string(), "latest-schema bootstrap target is incompatible");
		assert_eq!(format!("{error:?}"), "Database(Incompatible)");
	}

	#[test]
	fn rootless_bootstrap_never_reports_ephemeral_server_identity_as_ready() {
		let bootstrap = bootstrap::bootstrap_without_root(DoctorIssue::UnsafeHostPath);
		let identity = bootstrap
			.doctor
			.check(DoctorComponent::ServerIdentity)
			.expect("rootless bootstrap includes server identity status");

		assert_eq!(
			identity.status,
			DoctorStatus::Unavailable(DoctorIssue::ServerIdentityUnavailable)
		);
	}
}
