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
	account_launch::{AttestedAppServerProfile, ResetCardRuntime, ResetCardVaultStatus},
	account_observation::AccountObservationService,
	account_profile::AccountProfileRuntime,
	account_service::{AccountInspection, AccountService, OpenAiCredentialRefresher},
	application::{ProductStore, ServiceApplication},
	managed_repository_runtime::{
		ManagedRepositoryReadiness, ManagedRepositoryRuntime, ManagedRepositoryStartupError,
	},
	process_supervisor::{
		ProcessGenerationControl, ProcessGenerationReadiness, ProcessSupervisorError,
	},
	provider_attempt_service::{ProviderAttemptControl, ProviderAttemptReadiness},
};
use decodex_codex::CodexAdapter;
use decodex_core::{
	AccountLifecycleReadiness, AccountRecord, AccountRoutingControl, Availability, BlobStore,
	ConfigError, DecodexConfig, DecodexPaths, DecodexRoot, PathError, PostgresIdentityConfig,
	ProcessExecutionAuthorization, ProductState as _, ServerIdentity, ServerProfile,
};
use decodex_postgres::{BootstrapFailure, CodexAccountCapabilityAttestation, PostgresStore};
use decodex_protocol::{
	AppServerCapability, CURRENT_VERSION, DoctorCheck, DoctorComponent, DoctorIssue, DoctorReport,
	DoctorStatus, LocalTransportAuthority, LocalTransportListener, LocalTransportRefusal, ServerId,
};

const CONFIG_UNAVAILABLE: &str = "typed PostgreSQL configuration is unavailable";
const AUTHENTICATION_UNAVAILABLE: &str = "PostgreSQL authentication is unavailable";
const DATABASE_UNREACHABLE: &str = "configured PostgreSQL is unreachable";
const DATABASE_INCOMPATIBLE: &str = "configured PostgreSQL is incompatible";
const DATABASE_AUTHORITY_UNSAFE: &str = "configured PostgreSQL runtime authority is unsafe";
const MANAGED_REPOSITORY_UNAVAILABLE: &str = "managed repository runtime is unavailable";
const ACCOUNT_CALLBACK_ATTESTATION_TIMEOUT: Duration = Duration::from_secs(30);
const UNAVAILABLE_SHA256: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Value-free failure from the installer-owned local product-state provisioning command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalProvisionError {
	/// The platform root or typed configuration is unavailable or not local.
	Configuration,
	/// The configured migration credential is unavailable.
	Authentication,
	/// The owner-only process execution authorization is unavailable.
	ExecutionAuthorization,
	/// PostgreSQL rejected the bounded migration and authority-provisioning pass.
	Database(BootstrapFailure),
}
impl Display for LocalProvisionError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Configuration =>
				formatter.write_str("local product-state configuration is unavailable"),
			Self::Authentication =>
				formatter.write_str("local product-state authentication is unavailable"),
			Self::ExecutionAuthorization =>
				formatter.write_str("local process execution authorization is unavailable"),
			Self::Database(BootstrapFailure::Authentication) =>
				formatter.write_str("local product-state authentication failed"),
			Self::Database(BootstrapFailure::Unreachable) =>
				formatter.write_str("local product-state database is unreachable"),
			Self::Database(BootstrapFailure::Incompatible) =>
				formatter.write_str("local product-state database is incompatible"),
			Self::Database(BootstrapFailure::UnsafeAuthority) =>
				formatter.write_str("local product-state database authority is unsafe"),
			Self::Database(BootstrapFailure::UnsafeHostPath) =>
				formatter.write_str("local product-state database path is unsafe"),
		}
	}
}
impl std::error::Error for LocalProvisionError {}

/// Complete daemon bootstrap under one already-acquired singleton capability.
pub struct ServiceBootstrap {
	server_id: ServerId,
	store: ProductStore,
	managed_repositories: Option<ManagedRepositoryRuntime>,
	managed_repository_readiness: ManagedRepositoryReadiness,
	managed_repository_startup_error: Option<Arc<ManagedRepositoryStartupError>>,
	process_generations: Option<ProcessGenerationControl>,
	process_generation_readiness: ProcessGenerationReadiness,
	provider_attempts: Option<ProviderAttemptControl>,
	provider_attempt_readiness: ProviderAttemptReadiness,
	blob_store: Option<BlobStore>,
	accounts: Option<Arc<AccountService>>,
	account_profiles: Option<AccountProfileRuntime>,
	reset_cards: Option<ResetCardRuntime>,
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
		self.managed_repository_readiness
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
			managed_repository_readiness,
			managed_repository_startup_error,
			process_generations,
			process_generation_readiness: _,
			provider_attempts,
			provider_attempt_readiness: _,
			blob_store,
			accounts,
			account_profiles,
			reset_cards,
			doctor,
			daemon_authority,
		} = self;
		let listener = daemon_authority.map_err(ServerError::LocalTransport)?;
		let account_observations = match &accounts {
			Some(accounts) if account_profiles.is_some() || reset_cards.is_some() =>
				Some(AccountObservationService::new(
					Arc::clone(accounts),
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
				managed_repository_readiness,
				managed_repository_startup_error,
				process_generations,
				provider_attempts,
				CodexAdapter::unavailable(),
				blob_store,
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
	database: DoctorStatus,
	server_identity: DoctorStatus,
	shared_home: DoctorStatus,
	repositories: DoctorStatus,
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

async fn bootstrap_with_authority(
	paths: DecodexPaths,
	loaded: Result<DecodexConfig, ConfigError>,
	config_status: DoctorStatus,
	listener: LocalTransportListener,
) -> ServiceBootstrap {
	let identity = ServerIdentity::load_or_create(&paths);
	let (server_id, identity_status) = server_identity(identity);
	let mut repositories = loaded
		.as_ref()
		.map_or_else(|error| DoctorStatus::Unavailable(config_issue(*error)), server_repositories);
	let blob_store = BlobStore::open(paths.clone());
	let blob_integrity = match &blob_store {
		Ok(_) => DoctorStatus::Unknown(DoctorIssue::NotProbed),
		Err(_) => DoctorStatus::Unavailable(DoctorIssue::Integrity),
	};
	let shared_home = shared_codex_home();
	let (mut store, database, mut vault) = match (loaded.as_ref(), repositories) {
		(Ok(config), DoctorStatus::Ready) => connect_database(config).await,
		(Ok(_), _) => (
			ProductStore::Unavailable { reason: CONFIG_UNAVAILABLE },
			DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath),
			DoctorStatus::Unknown(DoctorIssue::NotProbed),
		),
		(Err(error), _) => (
			ProductStore::Unavailable { reason: CONFIG_UNAVAILABLE },
			DoctorStatus::Unavailable(database_config_issue(*error)),
			DoctorStatus::Unknown(DoctorIssue::Authentication),
		),
	};
	let postgres = match &store {
		ProductStore::Available(postgres) => Some(postgres.clone()),
		ProductStore::Unavailable { .. } => None,
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
	let (managed_repositories, managed_repository_readiness, managed_repository_startup_error) =
		match postgres.clone() {
			Some(postgres) => match ManagedRepositoryRuntime::start(postgres).await {
				Ok(runtime) => (Some(runtime), ManagedRepositoryReadiness::Ready, None),
				Err(error) => {
					let readiness = error.readiness();
					store = ProductStore::Unavailable { reason: MANAGED_REPOSITORY_UNAVAILABLE };
					repositories = DoctorStatus::Unavailable(DoctorIssue::Integrity);
					(None, readiness, Some(Arc::new(error)))
				},
			},
			None => (None, ManagedRepositoryReadiness::ProductStateUnavailable, None),
		};
	#[cfg(target_os = "macos")]
	let (accounts, account_profiles, reset_cards) = match postgres.clone() {
		Some(postgres) => {
			let (service, profiles, runtime, status) = bootstrap_macos_account_runtime(
				postgres,
				&paths,
				process_generations.clone(),
				process_execution_authorization.clone(),
			)
			.await;
			vault = status;
			(service, profiles, runtime)
		},
		None => (None, None, None),
	};
	#[cfg(not(target_os = "macos"))]
	let (accounts, account_profiles, reset_cards) = {
		vault = DoctorStatus::Unavailable(DoctorIssue::Authentication);
		(None, None, None)
	};
	let doctor = doctor_report(
		server_id.clone(),
		DoctorInputs {
			configuration: config_status,
			database,
			server_identity: identity_status,
			shared_home,
			repositories,
			blob_integrity,
			vault,
		},
	);

	ServiceBootstrap {
		server_id,
		store,
		managed_repositories,
		managed_repository_readiness,
		managed_repository_startup_error,
		process_generations,
		process_generation_readiness,
		provider_attempts,
		provider_attempt_readiness,
		blob_store: blob_store.ok(),
		accounts,
		account_profiles,
		reset_cards,
		doctor,
		daemon_authority: Ok(listener),
	}
}

#[cfg(target_os = "macos")]
async fn bootstrap_macos_account_runtime(
	postgres: PostgresStore,
	paths: &DecodexPaths,
	process_generations: Option<ProcessGenerationControl>,
	execution_authorization: Option<ProcessExecutionAuthorization>,
) -> (
	Option<Arc<AccountService>>,
	Option<AccountProfileRuntime>,
	Option<ResetCardRuntime>,
	DoctorStatus,
) {
	let refresher = match tokio::task::spawn_blocking(OpenAiCredentialRefresher::new).await {
		Ok(Ok(refresher)) => refresher,
		Ok(Err(_)) =>
			return (None, None, None, DoctorStatus::Unavailable(DoctorIssue::Authentication)),
		Err(_) => return (None, None, None, DoctorStatus::Unavailable(DoctorIssue::Integrity)),
	};
	let Ok(credentials) = MacosKeychainCredentialStore::new(paths) else {
		return (None, None, None, DoctorStatus::Unavailable(DoctorIssue::Integrity));
	};
	let credentials: Arc<dyn crate::HostCredentialStore> = Arc::new(credentials);
	let account_profiles =
		Some(AccountProfileRuntime::new(postgres.clone(), Arc::clone(&credentials)));
	let service = Arc::new(AccountService::new(postgres.clone(), credentials, Arc::new(refresher)));
	let launch_profile = match AttestedAppServerProfile::attest(
		paths.root().as_path().to_owned(),
		ACCOUNT_CALLBACK_ATTESTATION_TIMEOUT,
	) {
		Ok(profile) => profile,
		Err(_) => {
			let _ = service.attest_callback_capability(unavailable_callback_attestation()).await;
			let _ = service.reconcile_startup().await;
			return (
				Some(service),
				account_profiles,
				None,
				DoctorStatus::Unavailable(DoctorIssue::Integrity),
			);
		},
	};
	let attestation = launch_profile.account_callback_attestation();
	let mut closed_attestation = attestation.clone();
	closed_attestation.login_chatgpt_auth_tokens = false;
	closed_attestation.refresh_callback = false;
	if service.attest_callback_capability(closed_attestation.clone()).await.is_err()
		|| service.reconcile_startup().await.is_err()
	{
		return (
			Some(service),
			account_profiles,
			None,
			DoctorStatus::Unavailable(DoctorIssue::Integrity),
		);
	}
	let (Some(process_generations), Some(execution_authorization)) =
		(process_generations, execution_authorization)
	else {
		return (
			Some(service),
			account_profiles,
			None,
			DoctorStatus::Unavailable(DoctorIssue::Integrity),
		);
	};
	let inventory = match service.list_snapshot().await {
		Ok((accounts, routing)) => bootstrap_vault_inventory(&accounts, &routing),
		Err(_) => BootstrapVaultInventory::Unavailable,
	};
	let probe_account = match inventory {
		BootstrapVaultInventory::ReadyEmpty => None,
		BootstrapVaultInventory::Probe(account) => Some(*account),
		BootstrapVaultInventory::Unavailable => {
			return (
				Some(service),
				account_profiles,
				None,
				DoctorStatus::Unavailable(DoctorIssue::Integrity),
			);
		},
	};
	if probe_account.is_some() && service.arm_callback_capability_probe(&attestation).await.is_err()
	{
		let _ = service.attest_callback_capability(closed_attestation).await;
		return (
			Some(service),
			account_profiles,
			None,
			DoctorStatus::Unavailable(DoctorIssue::Integrity),
		);
	}
	let runtime = match ResetCardRuntime::start(
		postgres,
		Arc::clone(&service),
		process_generations,
		execution_authorization,
		launch_profile,
	) {
		Ok(runtime) => runtime,
		Err(_) => {
			let _ = service.attest_callback_capability(closed_attestation).await;
			return (
				Some(service),
				account_profiles,
				None,
				DoctorStatus::Unavailable(DoctorIssue::Integrity),
			);
		},
	};
	let Some(probe_account) = probe_account else {
		return (Some(service), account_profiles, Some(runtime), DoctorStatus::Ready);
	};
	let proved = runtime.prove_callback_capability(&probe_account).await.is_ok();
	if !proved || !service.attest_callback_capability(attestation).await.unwrap_or(false) {
		let _ = service.attest_callback_capability(closed_attestation).await;
		return (
			Some(service),
			account_profiles,
			None,
			DoctorStatus::Unavailable(DoctorIssue::Integrity),
		);
	}
	let status = match runtime.vault_status() {
		ResetCardVaultStatus::NotConfigured => DoctorStatus::Unknown(DoctorIssue::NotProbed),
		ResetCardVaultStatus::Ready => DoctorStatus::Ready,
		ResetCardVaultStatus::Unavailable => DoctorStatus::Unavailable(DoctorIssue::Authentication),
	};
	(Some(service), account_profiles, Some(runtime), status)
}

enum BootstrapVaultInventory {
	ReadyEmpty,
	Probe(Box<AccountRecord>),
	Unavailable,
}

fn bootstrap_vault_inventory(
	accounts: &[AccountInspection],
	routing: &AccountRoutingControl,
) -> BootstrapVaultInventory {
	let enabled = accounts.iter().filter(|candidate| candidate.account.enabled).collect::<Vec<_>>();
	if enabled.is_empty() {
		return BootstrapVaultInventory::ReadyEmpty;
	}
	if enabled.iter().any(|candidate| {
		candidate.account.tombstoned
			|| candidate.account.credential.is_none()
			|| candidate.account.unsettled_operation.is_some()
			|| candidate.readiness != AccountLifecycleReadiness::CallbackCapabilityUnready
			|| candidate.account.lifecycle_readiness
				!= AccountLifecycleReadiness::CallbackCapabilityUnready
	}) {
		return BootstrapVaultInventory::Unavailable;
	}
	routing
		.order
		.iter()
		.find_map(|account_id| {
			enabled
				.iter()
				.find(|candidate| candidate.account.account_id == *account_id)
				.map(|candidate| Box::new(candidate.account.clone()))
		})
		.map_or(BootstrapVaultInventory::Unavailable, BootstrapVaultInventory::Probe)
}

fn unavailable_callback_attestation() -> CodexAccountCapabilityAttestation {
	CodexAccountCapabilityAttestation {
		build_identity: "unavailable".to_owned(),
		executable_sha256: UNAVAILABLE_SHA256.to_owned(),
		schema_sha256: UNAVAILABLE_SHA256.to_owned(),
		callback_profile_sha256: UNAVAILABLE_SHA256.to_owned(),
		login_chatgpt_auth_tokens: false,
		refresh_callback: false,
	}
}

fn bootstrap_without_authority(
	refusal: LocalTransportRefusal,
	configuration: DoctorStatus,
	database: DoctorStatus,
) -> ServiceBootstrap {
	let server_id = unavailable_server_id();
	let doctor = doctor_report(
		server_id.clone(),
		DoctorInputs {
			configuration,
			database,
			server_identity: DoctorStatus::Unknown(DoctorIssue::NotProbed),
			shared_home: DoctorStatus::Unknown(DoctorIssue::NotProbed),
			repositories: DoctorStatus::Unknown(DoctorIssue::NotProbed),
			blob_integrity: DoctorStatus::Unknown(DoctorIssue::NotProbed),
			vault: DoctorStatus::Unknown(DoctorIssue::NotProbed),
		},
	);

	ServiceBootstrap {
		server_id,
		store: ProductStore::Unavailable { reason: CONFIG_UNAVAILABLE },
		managed_repositories: None,
		managed_repository_readiness: ManagedRepositoryReadiness::ProductStateUnavailable,
		managed_repository_startup_error: None,
		process_generations: None,
		process_generation_readiness: ProcessGenerationReadiness::ProductStateUnavailable,
		provider_attempts: None,
		provider_attempt_readiness: ProviderAttemptReadiness::ProductStateUnavailable,
		blob_store: None,
		accounts: None,
		account_profiles: None,
		reset_cards: None,
		doctor,
		daemon_authority: Err(refusal),
	}
}

fn bootstrap_without_root(issue: DoctorIssue) -> ServiceBootstrap {
	let server_id = unavailable_server_id();
	let doctor = doctor_report(
		server_id.clone(),
		DoctorInputs {
			configuration: DoctorStatus::Unavailable(issue),
			database: DoctorStatus::Unavailable(DoctorIssue::DatabaseNotConfigured),
			server_identity: DoctorStatus::Unavailable(DoctorIssue::ServerIdentityUnavailable),
			shared_home: DoctorStatus::Unknown(DoctorIssue::NotProbed),
			repositories: DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath),
			blob_integrity: DoctorStatus::Unavailable(DoctorIssue::Integrity),
			vault: DoctorStatus::Unknown(DoctorIssue::Authentication),
		},
	);

	ServiceBootstrap {
		server_id,
		store: ProductStore::Unavailable { reason: CONFIG_UNAVAILABLE },
		managed_repositories: None,
		managed_repository_readiness: ManagedRepositoryReadiness::ProductStateUnavailable,
		managed_repository_startup_error: None,
		process_generations: None,
		process_generation_readiness: ProcessGenerationReadiness::ProductStateUnavailable,
		provider_attempts: None,
		provider_attempt_readiness: ProviderAttemptReadiness::ProductStateUnavailable,
		blob_store: None,
		accounts: None,
		account_profiles: None,
		reset_cards: None,
		doctor,
		daemon_authority: Err(LocalTransportRefusal::ConfigurationUnavailable),
	}
}

fn doctor_report(server_id: ServerId, inputs: DoctorInputs) -> DoctorReport {
	let mut checks = vec![
		DoctorCheck::new(DoctorComponent::Configuration, inputs.configuration),
		DoctorCheck::new(DoctorComponent::Database, inputs.database),
		DoctorCheck::new(DoctorComponent::Protocol, DoctorStatus::Ready),
		DoctorCheck::new(DoctorComponent::ProtocolVersion, DoctorStatus::Ready),
		DoctorCheck::new(DoctorComponent::ServerIdentity, inputs.server_identity),
		DoctorCheck::new(DoctorComponent::SharedCodexHome, inputs.shared_home),
		DoctorCheck::new(DoctorComponent::ServerRepositories, inputs.repositories),
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

fn server_repositories(config: &DecodexConfig) -> DoctorStatus {
	if config
		.server_host()
		.repositories()
		.values()
		.all(|repository| host_directory(repository.as_server_path()).is_ok())
	{
		DoctorStatus::Ready
	} else {
		DoctorStatus::Unavailable(DoctorIssue::UnsafeHostPath)
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
		ConfigError::InvalidServerHostPath | ConfigError::InvalidPostgresHostPath =>
			DoctorIssue::UnsafeHostPath,
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

fn credential(identity: &PostgresIdentityConfig) -> Result<Option<String>, ()> {
	match identity.credential_env_var() {
		Some(name) => match env::var(name) {
			Ok(value) if !value.is_empty() => Ok(Some(value)),
			_ => Err(()),
		},
		None => Ok(None),
	}
}

pub(crate) async fn provision_local(root: DecodexRoot) -> Result<(), LocalProvisionError> {
	let paths = root.paths();
	let config = DecodexConfig::load(&paths).map_err(|_| LocalProvisionError::Configuration)?;
	if !matches!(config.active_profile(), ServerProfile::Local(_)) {
		return Err(LocalProvisionError::Configuration);
	}
	let migration_credential = credential(config.postgres().migration())
		.map_err(|()| LocalProvisionError::Authentication)?;
	let runtime_credential = credential(config.postgres().runtime())
		.map_err(|()| LocalProvisionError::Authentication)?;

	PostgresStore::migrate_and_provision_explicit(
		config.postgres(),
		migration_credential.as_deref(),
	)
	.await
	.map_err(|error| LocalProvisionError::Database(error.bootstrap_failure()))?;
	drop(
		PostgresStore::connect_explicit(
			config.postgres(),
			migration_credential.as_deref(),
			runtime_credential.as_deref(),
		)
		.await
		.map_err(|error| LocalProvisionError::Database(error.bootstrap_failure()))?,
	);

	let execution_authorization = ProcessExecutionAuthorization::load_or_create(&paths)
		.map_err(|_| LocalProvisionError::ExecutionAuthorization)?;
	PostgresStore::provision_process_execution_authorization_explicit(
		config.postgres(),
		migration_credential.as_deref(),
		&execution_authorization,
	)
	.await
	.map_err(|error| LocalProvisionError::Database(error.bootstrap_failure()))
}

async fn connect_database(config: &DecodexConfig) -> (ProductStore, DoctorStatus, DoctorStatus) {
	let postgres = config.postgres();
	let (migration_credential, runtime_credential) =
		match (credential(postgres.migration()), credential(postgres.runtime())) {
			(Ok(migration), Ok(runtime)) => (migration, runtime),
			_ => {
				return (
					ProductStore::Unavailable { reason: AUTHENTICATION_UNAVAILABLE },
					DoctorStatus::Unavailable(DoctorIssue::Authentication),
					DoctorStatus::Unavailable(DoctorIssue::Authentication),
				);
			},
		};
	let vault = DoctorStatus::Unknown(DoctorIssue::NotProbed);

	match PostgresStore::connect_explicit(
		postgres,
		migration_credential.as_deref(),
		runtime_credential.as_deref(),
	)
	.await
	{
		Ok(store) => (ProductStore::Available(store), DoctorStatus::Ready, vault),
		Err(error) => {
			let (reason, issue, vault) = match error.bootstrap_failure() {
				BootstrapFailure::Authentication =>
					(AUTHENTICATION_UNAVAILABLE, DoctorIssue::Authentication, vault),
				BootstrapFailure::Unreachable => (
					DATABASE_UNREACHABLE,
					DoctorIssue::DatabaseUnreachable,
					DoctorStatus::Unknown(DoctorIssue::Authentication),
				),
				BootstrapFailure::Incompatible =>
					(DATABASE_INCOMPATIBLE, DoctorIssue::DatabaseIncompatible, vault),
				BootstrapFailure::UnsafeAuthority =>
					(DATABASE_AUTHORITY_UNSAFE, DoctorIssue::UnsafeDatabaseAuthority, vault),
				BootstrapFailure::UnsafeHostPath => (
					DATABASE_UNREACHABLE,
					DoctorIssue::UnsafeHostPath,
					DoctorStatus::Unknown(DoctorIssue::Authentication),
				),
			};

			(ProductStore::Unavailable { reason }, DoctorStatus::Unavailable(issue), vault)
		},
	}
}

#[cfg(test)]
mod tests {
	use crate::{account_service::AccountInspection, bootstrap};
	use decodex_core::{
		AccountId, AccountLifecycleReadiness, AccountQuotaWindow, AccountQuotaWindowObservation,
		AccountRecord, AccountRoutingControl, AccountSelectionMode, AccountState,
	};
	use decodex_protocol::{DoctorComponent, DoctorIssue, DoctorStatus};

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

	#[test]
	fn account_vault_is_ready_empty_but_rejects_enabled_credential_loss() {
		let account_id =
			AccountId::new("10000000-0000-4000-8000-000000000001").expect("canonical account ID");
		let routing = AccountRoutingControl {
			revision: 1,
			mode: AccountSelectionMode::Balanced,
			order: vec![account_id.clone()],
		};
		let missing = AccountInspection {
			account: AccountRecord {
				account_id,
				label: "Account".to_owned(),
				enabled: false,
				revision: 1,
				observed_state: AccountState::Unknown,
				lifecycle_readiness: AccountLifecycleReadiness::CredentialAbsent,
				credential: None,
				unsettled_operation: None,
				five_hour_quota: AccountQuotaWindowObservation::unknown(
					AccountQuotaWindow::FIVE_HOURS_MINUTES,
				)
				.expect("supported quota window"),
				seven_day_quota: AccountQuotaWindowObservation::unknown(
					AccountQuotaWindow::SEVEN_DAYS_MINUTES,
				)
				.expect("supported quota window"),
				tombstoned: false,
			},
			readiness: AccountLifecycleReadiness::CredentialAbsent,
		};

		assert!(matches!(
			bootstrap::bootstrap_vault_inventory(
				&[],
				&AccountRoutingControl {
					revision: 1,
					mode: AccountSelectionMode::Balanced,
					order: Vec::new(),
				}
			),
			bootstrap::BootstrapVaultInventory::ReadyEmpty
		));
		assert!(matches!(
			bootstrap::bootstrap_vault_inventory(std::slice::from_ref(&missing), &routing),
			bootstrap::BootstrapVaultInventory::ReadyEmpty
		));

		let mut enabled_missing = missing;
		enabled_missing.account.enabled = true;

		assert!(matches!(
			bootstrap::bootstrap_vault_inventory(&[enabled_missing], &routing),
			bootstrap::BootstrapVaultInventory::Unavailable
		));
	}
}
