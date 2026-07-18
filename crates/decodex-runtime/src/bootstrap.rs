//! Typed fail-closed daemon bootstrap over the configuration and adapter owners.

use std::{
	env, fs,
	io::ErrorKind,
	net::{IpAddr, Ipv4Addr, SocketAddr},
	path::{Path, PathBuf},
};

use crate::{
	BoundServer, ProtocolServer, ServerConfig, ServerError,
	application::{ProductStore, ServiceApplication},
	managed_repository_runtime::ManagedRepositoryRuntime,
};
use decodex_codex::CodexAdapter;
use decodex_core::{
	Availability, BlobStore, ConfigError, DecodexConfig, DecodexRoot, PathError,
	PostgresIdentityConfig, ProductState as _, ServerIdentity,
};
use decodex_postgres::{BootstrapFailure, PostgresStore};
use decodex_protocol::{
	AppServerCapability, CURRENT_VERSION, DoctorCheck, DoctorComponent, DoctorIssue, DoctorReport,
	DoctorStatus, ServerId,
};

const DEFAULT_ADDRESS: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 49_152);
const CONFIG_UNAVAILABLE: &str = "typed PostgreSQL configuration is unavailable";
const AUTHENTICATION_UNAVAILABLE: &str = "PostgreSQL authentication is unavailable";
const DATABASE_UNREACHABLE: &str = "configured PostgreSQL is unreachable";
const DATABASE_INCOMPATIBLE: &str = "configured PostgreSQL is incompatible";
const DATABASE_AUTHORITY_UNSAFE: &str = "configured PostgreSQL runtime authority is unsafe";

/// Complete daemon bootstrap result, including its stable identity and typed report.
pub struct ServiceBootstrap {
	server_id: ServerId,
	address: SocketAddr,
	store: ProductStore,
	managed_repositories: Option<ManagedRepositoryRuntime>,
	blob_store: Option<BlobStore>,
	doctor: DoctorReport,
}
impl ServiceBootstrap {
	/// Loopback listener retained by the current security gate.
	pub const fn address(&self) -> SocketAddr {
		self.address
	}

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

	fn protocol_server(self, config: ServerConfig) -> ProtocolServer<ServiceApplication> {
		ProtocolServer::new(
			self.server_id,
			ServiceApplication::new(
				self.store,
				self.managed_repositories,
				CodexAdapter::unavailable(),
				self.blob_store,
				self.doctor,
			),
			config,
		)
	}

	/// Bind the bootstrapped daemon on an explicitly selected loopback address.
	pub async fn bind(
		self,
		address: SocketAddr,
		config: ServerConfig,
	) -> Result<BoundServer, ServerError> {
		self.protocol_server(config).bind(address).await
	}

	/// Run the bootstrapped daemon on its configured current local endpoint.
	pub async fn run(self, config: ServerConfig) -> Result<(), ServerError> {
		let address = self.address;

		self.protocol_server(config).run(address).await
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
	let identity = ServerIdentity::load_or_create(&paths);
	let (server_id, identity_status) = server_identity(identity);
	let loaded = DecodexConfig::load(&paths);
	let config_status = match &loaded {
		Ok(_) => DoctorStatus::Ready,
		Err(error) => DoctorStatus::Unavailable(config_issue(*error)),
	};
	let repositories = loaded
		.as_ref()
		.map_or_else(|error| DoctorStatus::Unavailable(config_issue(*error)), server_repositories);
	let blob_store = BlobStore::open(paths.clone());
	let blob_integrity = match &blob_store {
		Ok(_) => DoctorStatus::Unknown(DoctorIssue::NotProbed),
		Err(_) => DoctorStatus::Unavailable(DoctorIssue::Integrity),
	};
	let shared_home = shared_codex_home();
	let (store, database, vault) = match (loaded.as_ref(), repositories) {
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
	let managed_repositories = match &store {
		ProductStore::Available(store) => ManagedRepositoryRuntime::open(store.clone()),
		ProductStore::Unavailable { .. } => None,
	};
	let managed_repositories = match managed_repositories {
		Some(runtime) if runtime.reconcile_restart().await.is_ok() => Some(runtime),
		_ => None,
	};

	ServiceBootstrap {
		server_id,
		address: DEFAULT_ADDRESS,
		store,
		managed_repositories,
		blob_store: blob_store.ok(),
		doctor,
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
		address: DEFAULT_ADDRESS,
		store: ProductStore::Unavailable { reason: CONFIG_UNAVAILABLE },
		managed_repositories: None,
		blob_store: None,
		doctor,
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
			Err(error) if error.kind() == ErrorKind::NotFound =>
				return Err(HostDirectoryError::Missing),
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
