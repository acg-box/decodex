//! Typed fail-closed daemon bootstrap over the configuration and adapter owners.

use std::{
	env, fs,
	io::ErrorKind,
	path::{Path, PathBuf},
	sync::Arc,
};

use crate::{
	BoundServer, ProtocolServer, ServerConfig, ServerError,
	account_launch::{ResetCardRuntime, ResetCardVaultStatus},
	application::{ProductStore, ServiceApplication},
	managed_repository_runtime::{
		ManagedRepositoryReadiness, ManagedRepositoryRuntime, ManagedRepositoryStartupError,
	},
};
use decodex_codex::CodexAdapter;
use decodex_core::{
	Availability, BlobStore, ConfigError, DecodexConfig, DecodexPaths, DecodexRoot, PathError,
	PostgresIdentityConfig, ProductState as _, ServerIdentity, ServerProfile,
};
use decodex_postgres::{BootstrapFailure, PostgresStore};
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

/// Complete daemon bootstrap under one already-acquired singleton capability.
pub struct ServiceBootstrap {
	server_id: ServerId,
	store: ProductStore,
	managed_repositories: Option<ManagedRepositoryRuntime>,
	managed_repository_readiness: ManagedRepositoryReadiness,
	managed_repository_startup_error: Option<Arc<ManagedRepositoryStartupError>>,
	blob_store: Option<BlobStore>,
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

	/// Move the already-owned same-UID listener into the server lifecycle.
	pub async fn bind(self, config: ServerConfig) -> Result<BoundServer, ServerError> {
		let Self {
			server_id,
			store,
			managed_repositories,
			managed_repository_readiness,
			managed_repository_startup_error,
			blob_store,
			reset_cards,
			doctor,
			daemon_authority,
		} = self;
		let listener = daemon_authority.map_err(ServerError::LocalTransport)?;
		let server = ProtocolServer::new(
			server_id,
			ServiceApplication::new(
				store,
				managed_repositories,
				managed_repository_readiness,
				managed_repository_startup_error,
				CodexAdapter::unavailable(),
				blob_store,
				doctor,
			)
			.with_reset_cards(reset_cards),
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
	let (managed_repositories, managed_repository_readiness, managed_repository_startup_error) =
		match postgres {
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
	let reset_cards = match (&store, loaded.as_ref()) {
		(ProductStore::Available(postgres), Ok(config)) => {
			match ResetCardRuntime::start(
				postgres.clone(),
				config.server_host(),
				paths.root().as_path().to_owned(),
			)
			.await
			{
				Ok(runtime) => {
					vault = match runtime.vault_status() {
						ResetCardVaultStatus::NotConfigured =>
							DoctorStatus::Unknown(DoctorIssue::NotProbed),
						ResetCardVaultStatus::Ready => DoctorStatus::Ready,
						ResetCardVaultStatus::Unavailable =>
							DoctorStatus::Unavailable(DoctorIssue::Authentication),
					};

					Some(runtime)
				},
				Err(_) => {
					vault = DoctorStatus::Unavailable(DoctorIssue::Integrity);

					None
				},
			}
		},
		_ => None,
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
		blob_store: blob_store.ok(),
		reset_cards,
		doctor,
		daemon_authority: Ok(listener),
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
		blob_store: None,
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
		blob_store: None,
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
	use crate::bootstrap;
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
}
