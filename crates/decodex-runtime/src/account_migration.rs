//! Installer-only, offline one-shot transfer into the Account Service authority.

use std::{
	collections::{BTreeMap, BTreeSet},
	env,
	error::Error,
	ffi::{CStr, OsStr},
	fmt::{Display, Formatter},
	fs::{self, File, OpenOptions},
	io::{Read as _, Take, Write as _},
	os::{
		fd::RawFd,
		unix::{
			ffi::OsStrExt as _,
			fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _},
		},
	},
	path::{Component, Path, PathBuf},
	sync::Arc,
};
#[cfg(feature = "account-migration-transition-gate")]
use std::{
	os::fd::{AsRawFd as _, FromRawFd as _},
	process::Command,
	sync::{Mutex, OnceLock},
};

use decodex_core::{
	AccountId, AccountOperation, AccountOperationId, AccountOperationKind, AccountOperationPhase,
	AccountProvider, AccountSelectionMode, CredentialBinding, CredentialFingerprint,
	CredentialStoreSchemaVersion, CredentialVersion, DecodexConfig, DecodexRoot, LocalTrustPolicy,
	PostgresIdentityConfig, ProcessExecutionAuthorization, ProviderIdentity, ServerProfile,
};
#[cfg(feature = "account-migration-transition-gate")]
use decodex_core::{
	AccountQuotaWindow, AccountSelectionRecovery, DecodexPaths, ResetCardDescriptor,
	ResetCardTimestamp,
};
use decodex_postgres::{
	AccountAdministrationOutcome, AccountMigrationReceipt, PostgresStore, StoreError,
};
#[cfg(feature = "account-migration-transition-gate")]
use decodex_postgres::{
	AccountCommandKind, AccountCommandReceiptClaim, AccountLifecycleRejection,
	CodexAccountCapabilityAttestation, CommandIdentity,
};
use decodex_protocol::LocalTransportAuthority;
#[cfg(feature = "account-migration-transition-gate")]
use decodex_protocol::{AccountManualRecoveryActionDto, CommandPayload, EntityId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

use crate::{
	AccountService, CredentialRefreshError, HostCredentialStore, MacosKeychainCredentialStore,
	account_import::read_explicit_credential_file,
	account_service::{
		AccountLifecycleError, AccountMigrationTransition, CredentialRefreshPort,
		CredentialRefreshResult,
	},
	daemon_wrapper::{
		DaemonWrapperDescriptor, daemon_wrapper_descriptor_sha256, verify_current_daemon_wrapper,
		verify_launch_agent_daemon_wrapper,
	},
	host_credentials::{CredentialSecretBundle, CredentialStoreError},
};
#[cfg(feature = "account-migration-transition-gate")]
use crate::{
	ProcessGenerationControl,
	account_launch::{ResetCardRuntime, ResetCardServiceError},
	account_service::AccountManualRecoveryAction,
};

const MANIFEST_SCHEMA: &str = "decodex/account-migration-manifest/1";
const TARGET_HOST_STORE: &str = "macos_keychain_generic_password_v1";
const TARGET_POSTGRES_PROJECTION: &str = "decodex.accounts_credential_binding_v27";
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_CREDENTIAL_SOURCE_BYTES: u64 = 256 * 1024;
const MAX_LAUNCH_AGENT_BYTES: u64 = 64 * 1024;
const MAX_INSTALLED_ASSET_BYTES: u64 = 512 * 1024 * 1024;
const MAX_ACCOUNTS: usize = 512;
const SOURCE_ROLES: [&str; 4] =
	["legacy_account_pool", "legacy_account_config", "vnext_uuid_bridge", "vnext_account_config"];
#[cfg(feature = "account-migration-transition-gate")]
const ACCOUNT_MIGRATION_GATE_RUN_SCHEMA: &str = "decodex/account-migration-gate-run/1";
#[cfg(feature = "account-migration-transition-gate")]
const ACCOUNT_MIGRATION_GATE_RUN_FILE: &str = "account-migration-gate-run.json";

#[cfg(feature = "account-migration-transition-gate")]
static ACCOUNT_MIGRATION_TRANSITION_GATE: OnceLock<Mutex<File>> = OnceLock::new();

#[cfg(feature = "account-migration-transition-gate")]
fn reject_aliased_account_migration_descriptors(
	installer_lock_fd: RawFd,
	transition_gate_fd: Option<RawFd>,
) -> Result<(), OfflineAccountMigrationError> {
	if transition_gate_fd == Some(installer_lock_fd) {
		return Err(OfflineAccountMigrationError::InstallerLockUnavailable);
	}
	Ok(())
}

#[cfg(feature = "account-migration-transition-gate")]
fn configure_account_migration_transition_gate(
	raw_fd: Option<RawFd>,
) -> Result<(), OfflineAccountMigrationError> {
	let Some(raw_fd) = raw_fd else {
		return Ok(());
	};
	if raw_fd < 3 || ACCOUNT_MIGRATION_TRANSITION_GATE.get().is_some() {
		return Err(OfflineAccountMigrationError::InstallerLockUnavailable);
	}
	// SAFETY: `F_GETFD` reads descriptor flags and retains no process memory pointer.
	let inherited_flags = unsafe { libc::fcntl(raw_fd, libc::F_GETFD) };
	if inherited_flags == -1 {
		return Err(OfflineAccountMigrationError::InstallerLockUnavailable);
	}
	// SAFETY: `F_GETFD` proved that this process owns the inherited descriptor.
	let file = unsafe { File::from_raw_fd(raw_fd) };
	// SAFETY: the owned descriptor remains open and the integer flags are valid.
	if unsafe { libc::fcntl(raw_fd, libc::F_SETFD, inherited_flags | libc::FD_CLOEXEC) } == -1 {
		return Err(OfflineAccountMigrationError::InstallerLockUnavailable);
	}
	// SAFETY: `F_GETFD` reads back the flags of the still-owned descriptor.
	let applied_flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
	if applied_flags == -1 || applied_flags & libc::FD_CLOEXEC == 0 {
		return Err(OfflineAccountMigrationError::InstallerLockUnavailable);
	}
	ACCOUNT_MIGRATION_TRANSITION_GATE
		.set(Mutex::new(file))
		.map_err(|_| OfflineAccountMigrationError::InstallerLockUnavailable)
}

#[cfg(feature = "account-migration-transition-gate")]
fn verify_installer_lock_cloexec(
	installer_lock: &File,
) -> Result<(), OfflineAccountMigrationError> {
	const PROBE: &str = r#"import os,sys
target=(int(sys.argv[1]),int(sys.argv[2]))
try:
    metadata=os.fstat(int(sys.argv[3]))
except OSError:
    raise SystemExit(0)
if (metadata.st_dev,metadata.st_ino)==target:
    raise SystemExit(1)
"#;
	let metadata = installer_lock
		.metadata()
		.map_err(|_| OfflineAccountMigrationError::InstallerLockUnavailable)?;
	let status = Command::new("/usr/bin/python3")
		.arg("-c")
		.arg(PROBE)
		.arg(metadata.dev().to_string())
		.arg(metadata.ino().to_string())
		.arg(installer_lock.as_raw_fd().to_string())
		.env_clear()
		.status()
		.map_err(|_| OfflineAccountMigrationError::InstallerLockUnavailable)?;
	if !status.success() {
		return Err(OfflineAccountMigrationError::InstallerLockUnavailable);
	}
	account_migration_transition_checkpoint("installer_lock_cloexec_verified", None)
}

#[cfg(feature = "account-migration-transition-gate")]
pub(crate) fn account_migration_transition_checkpoint(
	name: &str,
	account_id: Option<&AccountId>,
) -> Result<(), OfflineAccountMigrationError> {
	let Some(gate) = ACCOUNT_MIGRATION_TRANSITION_GATE.get() else {
		return Ok(());
	};
	let mut gate = gate.lock().map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?;
	writeln!(gate, "{name}|{}", account_id.map_or("-", AccountId::as_str))
		.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?;
	gate.flush().map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?;
	let mut acknowledgement = [0_u8; 1];
	gate.read_exact(&mut acknowledgement)
		.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?;
	if acknowledgement != [b'c'] {
		return Err(OfflineAccountMigrationError::DestinationMismatch);
	}
	Ok(())
}

/// Exact installer-produced inputs. Every path is opened locally and is never persisted as secret.
pub struct OfflineAccountMigrationOptions {
	/// Installer-borrowed descriptor for the retained local namespace lock.
	pub installer_lock_fd: RawFd,
	/// Gate-only inherited phase barrier used by the canonical transition harness.
	#[cfg(feature = "account-migration-transition-gate")]
	pub transition_gate_fd: Option<RawFd>,
	/// Reviewed repository configuration used for PostgreSQL routing.
	pub config: PathBuf,
	/// Installer-normalized credential-negative migration manifest.
	pub manifest: PathBuf,
	/// Owner-private directory that contains exact credential source files.
	pub credential_directory: PathBuf,
	/// Installed LaunchAgent property list verified before the transfer.
	pub launch_agent: PathBuf,
}

/// Installer-owned final readback after config swap and exact staging retirement.
pub struct OfflineAccountMigrationFinalizeOptions {
	/// Installer-borrowed descriptor for the retained local namespace lock.
	pub installer_lock_fd: RawFd,
	/// Gate-only inherited phase barrier used by the canonical transition harness.
	#[cfg(feature = "account-migration-transition-gate")]
	pub transition_gate_fd: Option<RawFd>,
	/// Active post-cutover repository configuration.
	pub config: PathBuf,
	/// Installer-normalized migration manifest.
	pub manifest: PathBuf,
	/// Installed LaunchAgent property list.
	pub launch_agent: PathBuf,
	/// Exact retired staging configuration path.
	pub retired_staging_config: PathBuf,
	/// Exact retired credential source directory.
	pub retired_credential_directory: PathBuf,
	/// Exact active legacy sources that must be absent.
	pub retired_active_sources: Vec<PathBuf>,
	/// Installed runtime assets covered by final verification.
	pub installed_assets: Vec<PathBuf>,
}

/// Installer-only exact destination readback for a prepared cutover after config swap.
pub struct OfflineAccountMigrationDestinationVerifyOptions {
	/// Installer-borrowed descriptor for the retained local namespace lock.
	pub installer_lock_fd: RawFd,
	/// Gate-only inherited phase barrier used by the canonical transition harness.
	#[cfg(feature = "account-migration-transition-gate")]
	pub transition_gate_fd: Option<RawFd>,
	/// Active post-cutover repository configuration.
	pub config: PathBuf,
	/// Frozen migration manifest.
	pub manifest: PathBuf,
	/// Installed LaunchAgent property list.
	pub launch_agent: PathBuf,
}

/// Credential-negative completed-cutover readback used by reinstall and upgrade.
pub struct OfflineAccountMigrationVerifyOptions {
	/// Installer-borrowed descriptor for the retained local namespace lock.
	pub installer_lock_fd: RawFd,
	/// Gate-only inherited phase barrier used by the canonical transition harness.
	#[cfg(feature = "account-migration-transition-gate")]
	pub transition_gate_fd: Option<RawFd>,
	/// Active post-cutover repository configuration.
	pub config: PathBuf,
	/// Installed LaunchAgent property list.
	pub launch_agent: PathBuf,
	/// Exact retired staging configuration path.
	pub retired_staging_config: PathBuf,
	/// Exact retired credential source directory.
	pub retired_credential_directory: PathBuf,
	/// Exact active legacy sources that must remain absent.
	pub retired_active_sources: Vec<PathBuf>,
	/// Installed runtime assets covered by verification.
	pub installed_assets: Vec<PathBuf>,
}

/// Credential-negative one-shot result suitable for operator automation.
#[derive(Serialize)]
pub struct OfflineAccountMigrationReport {
	/// Stable operator-result schema.
	pub schema: &'static str,
	/// Stable completed outcome.
	pub outcome: &'static str,
	/// Digest of the normalized source manifest.
	pub manifest_sha256: String,
	/// Number of migrated accounts.
	pub account_count: usize,
	/// Canonical migrated account identities.
	pub account_ids: Vec<String>,
	/// Whether the PostgreSQL migration intent was recorded by this run.
	pub intent_recorded: bool,
	/// Whether the exact completed receipt is now durable.
	pub receipt_completed: bool,
}

/// Non-secret evidence from real runtime admission owners at one gate boundary.
#[cfg(feature = "account-migration-transition-gate")]
#[derive(Serialize)]
pub struct AccountMigrationAdmissionGateReport {
	/// Stable gate-only report schema.
	pub schema: &'static str,
	/// Requested migration boundary.
	pub boundary: &'static str,
	/// Exact Account Service initial-selection result.
	pub initial_selection: &'static str,
	/// Exact Account Service process-credential admission result.
	pub process_spawn_admission: &'static str,
	/// Exact Reset Card preparation admission result.
	pub reset_card_admission: &'static str,
	/// Whether the gate composed the production ProcessGeneration owner.
	pub process_generation_owner_composed: bool,
}

/// Non-secret evidence that manifest-bound cancellation was refused by both recovery owners.
#[cfg(feature = "account-migration-transition-gate")]
#[derive(Serialize)]
pub struct AccountMigrationRecoveryGateReport {
	/// Stable gate-only report schema.
	pub schema: &'static str,
	/// Exact manifest operation phase exercised by this probe.
	pub phase: &'static str,
	/// Direct Account Service recovery returned the typed unsettled rejection.
	pub direct_cancel_refused: bool,
	/// Logical-command recovery retained the standard typed unsettled result.
	pub logical_command_cancel_refused: bool,
	/// Same-key logical-command readback returned the exact durable result.
	pub logical_command_receipt_replayed: bool,
	/// The typed operation projection remained equal across both refusal paths.
	pub operation_unchanged: bool,
	/// The typed account projection remained equal across both refusal paths.
	pub account_unchanged: bool,
}

/// Non-secret result from retaining the real local transport owner for a gate barrier.
#[cfg(feature = "account-migration-transition-gate")]
#[derive(Serialize)]
pub struct AccountMigrationLiveDaemonGateReport {
	/// Stable gate-only report schema.
	pub schema: &'static str,
	/// Whether the real local transport listener retained the namespace.
	pub namespace_retained: bool,
}

#[cfg(feature = "account-migration-transition-gate")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountMigrationGateRunDescriptor {
	schema: String,
	run_id: String,
}

#[cfg(feature = "account-migration-transition-gate")]
pub(crate) struct AccountMigrationGateRun {
	pub(crate) run_id: String,
	pub(crate) fixture_root: PathBuf,
	pub(crate) paths: DecodexPaths,
}

#[derive(Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct CompletedDestinationReceipt {
	schema: String,
	keychain_verified: bool,
	postgresql_verified: bool,
	routing: CompletedRoutingReceipt,
	accounts: Vec<CompletedDestinationAccount>,
}

#[derive(Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct CompletedRoutingReceipt {
	mode: String,
	fixed_account_id: Option<String>,
	order: Vec<String>,
	revision: i64,
}

#[derive(Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct CompletedDestinationAccount {
	account_id: String,
	display_label: String,
	enabled: bool,
	revision: i64,
	provider: String,
	provider_account_id: String,
	host_store: String,
	postgres_projection: String,
	store_schema_version: u32,
	credential_version: u64,
	fingerprint_sha256: String,
	writer_operation_id: String,
	provider_account_id_sha256: String,
}

#[derive(Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct CompletedRetirementReceipt {
	schema: String,
	legacy_source_untouched: bool,
	runtime_legacy_authority_removed: bool,
	final_config_swapped: bool,
	staging_secrets_retired: bool,
	active_legacy_sources_retired: bool,
	installed_assets_verified: bool,
	daemon_wrapper_verified: bool,
	daemon_wrapper_identity_sha256: String,
	final_config_path: String,
	final_config_sha256: String,
	launch_agent_path: String,
	launch_agent_sha256: String,
	retired_staging_config: String,
	retired_credential_directory: String,
	retired_active_sources: Vec<String>,
	installed_assets: Vec<CompletedInstalledAsset>,
	supervisor_profile: String,
}

#[derive(Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CompletedInstalledAsset {
	name: String,
	path: String,
	sha256: String,
	byte_count: u64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MigrationManifest {
	schema: String,
	sources: Vec<MigrationSource>,
	quota_policy: String,
	usage_profile_policy: String,
	history_policy: String,
	daemon_wrapper: DaemonWrapperDescriptor,
	decision_fingerprints: MigrationDecisionFingerprints,
	accounts: Vec<MigrationAccount>,
	routing: MigrationRouting,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct MigrationDecisionFingerprints {
	credentials_sha256: String,
	labels_sha256: String,
	enabled_sha256: String,
	routing_sha256: String,
	provider_sha256: String,
	quota_sha256: String,
	usage_profile_sha256: String,
	history_sha256: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MigrationSource {
	role: String,
	path: String,
	present: bool,
	byte_count: Option<u64>,
	sha256: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MigrationAccount {
	source_ordinal: usize,
	account_id: String,
	operation_id: String,
	provider: String,
	provider_account_id_sha256: String,
	display_label: String,
	enabled: bool,
	credential_source_sha256: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	target: Option<MigrationCredentialTarget>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct MigrationCredentialTarget {
	host_store: String,
	postgres_projection: String,
	store_schema_version: u16,
	credential_version: u64,
	writer_operation_id: String,
	fingerprint_sha256: String,
	provider_account_id: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum MigrationRouting {
	Balanced { order: Vec<String> },
	Fixed { account_id: String, order: Vec<String> },
}

struct ParsedAccount {
	account_id: AccountId,
	operation_id: AccountOperationId,
	provider_account_id_sha256: String,
	display_label: String,
	enabled: bool,
	credential_source_sha256: String,
	target: Option<CredentialBinding>,
}

struct ValidatedManifest {
	raw: Value,
	document: MigrationManifest,
	digest: String,
	daemon_wrapper: DaemonWrapperDescriptor,
	sources: Vec<MigrationSource>,
	accounts: Vec<ParsedAccount>,
	routing: AccountSelectionMode,
	order: Vec<AccountId>,
}

struct PreparedMigrationCredential {
	provider: ProviderIdentity,
	bundle: CredentialSecretBundle,
	target: CredentialBinding,
}

struct OfflineRefresher;
impl CredentialRefreshPort for OfflineRefresher {
	fn refresh(
		&self,
		_current: &CredentialSecretBundle,
	) -> Result<CredentialRefreshResult, CredentialRefreshError> {
		Err(CredentialRefreshError::Unavailable)
	}
}

fn account_migration_error_at(
	stage: &'static str,
	error: OfflineAccountMigrationError,
) -> OfflineAccountMigrationError {
	#[cfg(feature = "account-migration-transition-gate")]
	eprintln!("decodex-account-migration-gate-failure:{stage}");
	#[cfg(not(feature = "account-migration-transition-gate"))]
	let _ = stage;
	error
}

/// Exercise the existing runtime admission owners without starting a daemon worker.
#[cfg(feature = "account-migration-transition-gate")]
pub async fn exercise_account_migration_admission_for_gate(
	config_path: &Path,
	account_id: &str,
	expected_revision: i64,
	boundary: &str,
) -> Result<AccountMigrationAdmissionGateReport, OfflineAccountMigrationError> {
	const BUILD_IDENTITY: &str = "codex-cli 0.145.0-alpha.18";
	const EXECUTABLE_SHA256: &str =
		"f0b214b476e04175bee104fe441caea874baeef3efc3828bfb79e972266156a9";
	const SCHEMA_SHA256: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
	const CALLBACK_SHA256: &str =
		"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
	const OBSERVED_AT_MICROS: i64 = 2_000_000_000_000_000;
	const RESETS_AT_MICROS: i64 = 2_100_000_000_000_000;

	if !matches!(boundary, "unsettled" | "completed") || expected_revision < 1 {
		return Err(OfflineAccountMigrationError::InvalidManifest);
	}
	let config_bytes = read_private_file(config_path, MAX_MANIFEST_BYTES)?;
	let config = DecodexConfig::parse(&config_bytes)
		.map_err(|_| OfflineAccountMigrationError::InvalidConfig)?;
	validate_configured_local_authority(&config)?;
	let migration_password = credential(config.postgres().migration())?;
	let runtime_password = credential(config.postgres().runtime())?;
	let store = PostgresStore::connect_explicit(
		config.postgres(),
		migration_password.as_deref().map(String::as_str),
		runtime_password.as_deref().map(String::as_str),
	)
	.await
	.map_err(|_| OfflineAccountMigrationError::PostgresUnavailable)?;
	let root = config_path.parent().ok_or(OfflineAccountMigrationError::InvalidPath)?;
	let paths = DecodexRoot::new(root.to_path_buf())
		.map_err(|_| OfflineAccountMigrationError::InvalidPath)?
		.paths();
	let account_id = AccountId::new(account_id.to_owned())
		.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
	let credentials: Arc<dyn HostCredentialStore> = Arc::new(
		MacosKeychainCredentialStore::new(&paths)
			.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?,
	);
	let service =
		Arc::new(AccountService::new(store.clone(), credentials, Arc::new(OfflineRefresher)));
	let callback_ready = service
		.attest_callback_capability(CodexAccountCapabilityAttestation {
			build_identity: BUILD_IDENTITY.to_owned(),
			executable_sha256: EXECUTABLE_SHA256.to_owned(),
			schema_sha256: SCHEMA_SHA256.to_owned(),
			callback_profile_sha256: CALLBACK_SHA256.to_owned(),
			login_chatgpt_auth_tokens: true,
			refresh_callback: true,
		})
		.await
		.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?;
	if !callback_ready {
		return Err(OfflineAccountMigrationError::DestinationMismatch);
	}

	if boundary == "completed" {
		for duration in
			[AccountQuotaWindow::FIVE_HOURS_MINUTES, AccountQuotaWindow::SEVEN_DAYS_MINUTES]
		{
			let fact = AccountQuotaWindow::new(duration, 10, RESETS_AT_MICROS)
				.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?;
			service
				.observe_quota(&account_id, fact, OBSERVED_AT_MICROS)
				.await
				.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?;
		}
	}

	let initial_selection = match (boundary, service.select_initial(OBSERVED_AT_MICROS).await) {
		("unsettled", Err(failure))
			if failure.account_id.is_none()
				&& failure.recovery == AccountSelectionRecovery::ResolveCredentialOperation =>
			"refused",
		("completed", Ok(selected))
			if selected.account.account_id == account_id
				&& selected.account.revision == expected_revision =>
			"admitted",
		_ => "unexpected",
	};
	let process_spawn_admission =
		match (boundary, service.process_credential(&account_id, expected_revision).await) {
			("unsettled", Err(AccountLifecycleError::CredentialAbsent)) => "refused",
			("completed", Ok(credential)) => {
				drop(credential);
				"admitted"
			},
			_ => "unexpected",
		};

	let process_generations = ProcessGenerationControl::start(store.clone())
		.await
		.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?;
	let execution_authorization = ProcessExecutionAuthorization::load_or_create(&paths)
		.map_err(|_| OfflineAccountMigrationError::ExecutionAuthorizationUnavailable)?;
	let reset_card = ResetCardRuntime::start(
		store,
		Arc::clone(&service),
		process_generations,
		execution_authorization,
		root.to_path_buf(),
	)
	.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?;
	let descriptor = ResetCardDescriptor::new(
		ResetCardTimestamp::from_unix_seconds(2_000_000_000)
			.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?,
		ResetCardTimestamp::from_unix_seconds(2_000_003_600)
			.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?,
	)
	.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?;
	let idempotency_key = format!("xy1422-{boundary}-{}", account_id.as_str());
	let reset_card_admission = match (
		boundary,
		reset_card.prepare(&idempotency_key, &account_id, expected_revision, descriptor).await,
	) {
		("unsettled", Err(ResetCardServiceError::AccountStateRejected)) => "refused",
		("completed", Ok(_)) => "admitted",
		_ => "unexpected",
	};

	Ok(AccountMigrationAdmissionGateReport {
		schema: "decodex/account-migration-admission-gate/1",
		boundary: if boundary == "unsettled" { "unsettled" } else { "completed" },
		initial_selection,
		process_spawn_admission,
		reset_card_admission,
		process_generation_owner_composed: true,
	})
}

/// Exercise direct and logical-command cancellation through the exact manifest operation.
#[cfg(feature = "account-migration-transition-gate")]
pub async fn exercise_account_migration_recovery_for_gate(
	run_descriptor: &Path,
	phase: &str,
) -> Result<AccountMigrationRecoveryGateReport, OfflineAccountMigrationError> {
	let expected_phase = match phase {
		"prepared" => AccountOperationPhase::Prepared,
		"recovery_required" => AccountOperationPhase::RecoveryRequired,
		_ => return Err(OfflineAccountMigrationError::InvalidManifest),
	};
	let run = load_account_migration_gate_run(run_descriptor)?;
	let config_path = run.paths.root().as_path().join(".account-migration-runtime.toml");
	let config_bytes = read_private_file(&config_path, MAX_MANIFEST_BYTES)?;
	let config = DecodexConfig::parse(&config_bytes)
		.map_err(|_| OfflineAccountMigrationError::InvalidConfig)?;
	validate_configured_local_authority(&config)?;
	let manifest_path = run.paths.root().as_path().join("account-migration-manifest.json");
	let manifest = parse_manifest(&read_private_file(&manifest_path, MAX_MANIFEST_BYTES)?)?;
	if manifest.accounts.len() != 4 {
		return Err(OfflineAccountMigrationError::InvalidManifest);
	}
	for (index, account) in manifest.accounts.iter().enumerate() {
		let expected_account_id = AccountId::new(account_migration_gate_uuid(
			&run.run_id,
			&format!("account_{}", index + 1),
			"account",
		))
		.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
		if account.account_id != expected_account_id
			|| account
				.target
				.as_ref()
				.is_none_or(|target| target.writer_operation_id != account.operation_id)
		{
			return Err(OfflineAccountMigrationError::InvalidManifest);
		}
	}
	let account = manifest.accounts.first().ok_or(OfflineAccountMigrationError::InvalidManifest)?;
	let account_id = account.account_id.clone();
	let operation_id = account.operation_id.clone();
	let migration_password = credential(config.postgres().migration())?;
	let runtime_password = credential(config.postgres().runtime())?;
	let store = PostgresStore::connect_explicit(
		config.postgres(),
		migration_password.as_deref().map(String::as_str),
		runtime_password.as_deref().map(String::as_str),
	)
	.await
	.map_err(|_| OfflineAccountMigrationError::PostgresUnavailable)?;
	let credentials: Arc<dyn HostCredentialStore> = Arc::new(
		MacosKeychainCredentialStore::new(&run.paths)
			.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?,
	);
	let service = AccountService::new(store.clone(), credentials, Arc::new(OfflineRefresher));
	let before_operation = store
		.read_account_operation(&operation_id)
		.await
		.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?
		.ok_or(OfflineAccountMigrationError::DestinationMismatch)?;
	let before_account = service
		.inspect(&account_id)
		.await
		.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?
		.account;
	if before_operation.account_id != account_id
		|| before_operation.kind != AccountOperationKind::Import
		|| before_operation.phase != expected_phase
		|| before_account.revision < 1
	{
		return Err(OfflineAccountMigrationError::DestinationMismatch);
	}
	let direct_cancel_refused = matches!(
		service
			.recover_operation(
				&operation_id,
				before_account.revision,
				AccountManualRecoveryAction::CancelBeforeEffect,
			)
			.await,
		Err(AccountLifecycleError::OperationRejected(
			AccountLifecycleRejection::OperationUnsettled
		))
	);
	if !direct_cancel_refused
		|| store
			.read_account_operation(&operation_id)
			.await
			.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?
			.as_ref() != Some(&before_operation)
		|| service
			.inspect(&account_id)
			.await
			.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?
			.account != before_account
	{
		return Err(OfflineAccountMigrationError::DestinationMismatch);
	}

	let payload = CommandPayload::RecoverAccountOperation {
		operation_id: EntityId::new(operation_id.as_str().to_owned())
			.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?,
		action: AccountManualRecoveryActionDto::CancelBeforeEffect,
	};
	let request =
		serde_json::to_vec(&payload).map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
	let identity = CommandIdentity::new(format!("xy1422-{}-{phase}-cancel", run.run_id), &request)
		.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
	let lease = match store
		.reserve_account_command(
			&identity,
			AccountCommandKind::Recover,
			operation_id.as_str(),
			Some(before_account.revision),
		)
		.await
		.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?
	{
		AccountCommandReceiptClaim::Owned(lease) => lease,
		AccountCommandReceiptClaim::Replayed(_) =>
			return Err(OfflineAccountMigrationError::DestinationMismatch),
	};
	let receipt_operation_id = operation_id.clone();
	let response = service
		.recover_operation_command(
			lease,
			&operation_id,
			before_account.revision,
			AccountManualRecoveryAction::CancelBeforeEffect,
			move |result| {
				crate::application::encode_account_migration_recovery_for_gate(
					receipt_operation_id,
					result,
				)
			},
		)
		.await
		.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?;
	let logical_command_cancel_refused =
		crate::application::is_account_migration_cancel_refusal_for_gate(&response);
	let replayed = match store
		.reserve_account_command(
			&identity,
			AccountCommandKind::Recover,
			operation_id.as_str(),
			Some(before_account.revision),
		)
		.await
		.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?
	{
		AccountCommandReceiptClaim::Replayed(replayed) => replayed == response,
		AccountCommandReceiptClaim::Owned(_) => false,
	};
	let operation_unchanged = store
		.read_account_operation(&operation_id)
		.await
		.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?
		.as_ref()
		== Some(&before_operation);
	let account_unchanged = service
		.inspect(&account_id)
		.await
		.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?
		.account
		== before_account;
	if !logical_command_cancel_refused || !replayed || !operation_unchanged || !account_unchanged {
		return Err(OfflineAccountMigrationError::DestinationMismatch);
	}
	Ok(AccountMigrationRecoveryGateReport {
		schema: "decodex/account-migration-recovery-gate/1",
		phase: if expected_phase == AccountOperationPhase::Prepared {
			"prepared"
		} else {
			"recovery_required"
		},
		direct_cancel_refused,
		logical_command_cancel_refused,
		logical_command_receipt_replayed: replayed,
		operation_unchanged,
		account_unchanged,
	})
}

/// Retain the production local transport owner until the gate releases its private descriptor.
#[cfg(feature = "account-migration-transition-gate")]
pub async fn hold_account_migration_live_daemon_for_gate(
	root: &Path,
	raw_fd: RawFd,
) -> Result<AccountMigrationLiveDaemonGateReport, OfflineAccountMigrationError> {
	if raw_fd < 3 {
		return Err(OfflineAccountMigrationError::InstallerLockUnavailable);
	}
	// SAFETY: `F_GETFD` reads descriptor flags and retains no process memory pointer.
	let inherited_flags = unsafe { libc::fcntl(raw_fd, libc::F_GETFD) };
	if inherited_flags == -1 {
		return Err(OfflineAccountMigrationError::InstallerLockUnavailable);
	}
	// SAFETY: `F_GETFD` proved that this process owns the inherited descriptor.
	let mut barrier = unsafe { File::from_raw_fd(raw_fd) };
	// SAFETY: the owned descriptor remains open and the integer flags are valid.
	if unsafe { libc::fcntl(raw_fd, libc::F_SETFD, inherited_flags | libc::FD_CLOEXEC) } == -1 {
		return Err(OfflineAccountMigrationError::InstallerLockUnavailable);
	}
	let paths = DecodexRoot::new(root.to_path_buf())
		.map_err(|_| OfflineAccountMigrationError::InvalidPath)?
		.paths();
	// SAFETY: `geteuid` has no arguments and cannot fail.
	let effective_uid = unsafe { libc::geteuid() };
	let authority =
		LocalTransportAuthority::new(paths, LocalTrustPolicy::SameUid, Some(effective_uid))
			.map_err(|_| OfflineAccountMigrationError::InstallerLockUnavailable)?;
	let listener = authority
		.bind()
		.await
		.map_err(|_| OfflineAccountMigrationError::InstallerLockUnavailable)?;
	barrier
		.write_all(b"live_daemon_ready\n")
		.map_err(|_| OfflineAccountMigrationError::InstallerLockUnavailable)?;
	barrier.flush().map_err(|_| OfflineAccountMigrationError::InstallerLockUnavailable)?;
	let mut release = [0_u8; 1];
	barrier
		.read_exact(&mut release)
		.map_err(|_| OfflineAccountMigrationError::InstallerLockUnavailable)?;
	if release != [b'c'] {
		return Err(OfflineAccountMigrationError::InstallerLockUnavailable);
	}
	drop(listener);
	Ok(AccountMigrationLiveDaemonGateReport {
		schema: "decodex/account-migration-live-daemon-gate/1",
		namespace_retained: true,
	})
}

/// Normalize is installer-owned; this executor verifies all inputs and performs the finite
/// transfer.
#[allow(clippy::too_many_lines)] // Keep one closed, auditable offline ownership-transfer sequence.
pub async fn run_offline_account_migration(
	options: OfflineAccountMigrationOptions,
) -> Result<OfflineAccountMigrationReport, OfflineAccountMigrationError> {
	#[cfg(feature = "account-migration-transition-gate")]
	reject_aliased_account_migration_descriptors(
		options.installer_lock_fd,
		options.transition_gate_fd,
	)?;
	let installer_lock =
		validate_installer_namespace_lock(&options.config, options.installer_lock_fd)?;
	#[cfg(feature = "account-migration-transition-gate")]
	{
		configure_account_migration_transition_gate(options.transition_gate_fd)?;
		verify_installer_lock_cloexec(&installer_lock)?;
	}
	validate_absolute_paths(&options)?;
	let config_bytes = read_private_file(&options.config, MAX_MANIFEST_BYTES)?;
	let config = DecodexConfig::parse(&config_bytes)
		.map_err(|_| OfflineAccountMigrationError::InvalidConfig)?;
	validate_configured_local_authority(&config)?;
	let launch_agent = read_private_file(&options.launch_agent, MAX_LAUNCH_AGENT_BYTES)?;
	verify_retired_runtime_inputs(&launch_agent)?;
	let manifest_bytes = read_private_file(&options.manifest, MAX_MANIFEST_BYTES)?;
	let source_manifest = parse_source_manifest(&manifest_bytes)?;
	verify_manifest_daemon_wrapper(&source_manifest, &launch_agent)?;
	verify_sources(&source_manifest.sources)?;
	verify_credential_directory(&options.credential_directory, &source_manifest.accounts)?;
	let mut prepared_credentials = load_migration_credentials(&options, &source_manifest)?;
	let manifest =
		freeze_manifest_targets(&options.manifest, source_manifest, &prepared_credentials)?;
	#[cfg(feature = "account-migration-transition-gate")]
	account_migration_transition_checkpoint("manifest_frozen", None)?;
	verify_sources(&manifest.sources)?;
	verify_credential_directory(&options.credential_directory, &manifest.accounts)?;

	let migration_password = credential(config.postgres().migration())?;
	let runtime_password = credential(config.postgres().runtime())?;
	let account_count = u32::try_from(manifest.accounts.len())
		.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
	let intent_recorded = AccountService::migrate_account_cutover(
		config.postgres(),
		migration_password.as_deref().map(String::as_str),
		&manifest.digest,
		&manifest.raw,
		account_count,
	)
	.await
	.map_err(|error| match error {
		AccountLifecycleError::Persistence(StoreError::IdempotencyConflict) =>
			OfflineAccountMigrationError::ReceiptConflict,
		_ => OfflineAccountMigrationError::PostgresUnavailable,
	})?;
	#[cfg(feature = "account-migration-transition-gate")]
	account_migration_transition_checkpoint("cutover_committed", None)?;
	let store = PostgresStore::connect_explicit(
		config.postgres(),
		migration_password.as_deref().map(String::as_str),
		runtime_password.as_deref().map(String::as_str),
	)
	.await
	.map_err(|_| OfflineAccountMigrationError::PostgresUnavailable)?;
	#[cfg(feature = "account-migration-transition-gate")]
	account_migration_transition_checkpoint("runtime_store_connected", None)?;
	let root = options.config.parent().ok_or(OfflineAccountMigrationError::InvalidPath)?;
	let paths = DecodexRoot::new(root.to_path_buf())
		.map_err(|_| OfflineAccountMigrationError::InvalidPath)?
		.paths();
	let credentials = Arc::new(MacosKeychainCredentialStore::new(&paths).map_err(|_| {
		account_migration_error_at(
			"credential_store_open",
			OfflineAccountMigrationError::DestinationMismatch,
		)
	})?);
	#[cfg(feature = "account-migration-transition-gate")]
	account_migration_transition_checkpoint("credential_store_opened", None)?;
	let credential_store: Arc<dyn HostCredentialStore> = credentials.clone();
	let service = AccountService::new(store, credential_store, Arc::new(OfflineRefresher));
	service
		.prepare_migration_intent(&manifest.digest, &manifest.raw, account_count)
		.await
		.map_err(|_| OfflineAccountMigrationError::ReceiptConflict)?;
	#[cfg(feature = "account-migration-transition-gate")]
	account_migration_transition_checkpoint("intent_prepared", None)?;
	let execution_authorization = ProcessExecutionAuthorization::load_or_create(&paths)
		.map_err(|_| OfflineAccountMigrationError::ExecutionAuthorizationUnavailable)?;
	PostgresStore::provision_process_execution_authorization_explicit(
		config.postgres(),
		migration_password.as_deref().map(String::as_str),
		&execution_authorization,
	)
	.await
	.map_err(|_| OfflineAccountMigrationError::ExecutionAuthorizationUnavailable)?;

	let mut persisted_transitions = BTreeMap::new();
	for account in &manifest.accounts {
		let operation =
			service.read_migration_operation(&account.operation_id).await.map_err(|_| {
				account_migration_error_at(
					"operation_read",
					OfflineAccountMigrationError::DestinationMismatch,
				)
			})?;
		let transition = operation
			.as_ref()
			.map(|operation| migration_transition_from_operation(account, operation))
			.transpose()
			.map_err(|error| account_migration_error_at("operation_descriptor", error))?;
		if persisted_transitions.insert(account.account_id.clone(), transition).is_some() {
			return Err(OfflineAccountMigrationError::InvalidManifest);
		}
	}
	let initial =
		service.list().await.map_err(|_| OfflineAccountMigrationError::PostgresUnavailable)?;
	let expected_ids =
		manifest.accounts.iter().map(|account| account.account_id.clone()).collect::<BTreeSet<_>>();
	let initial_by_id = initial
		.into_iter()
		.map(|inspection| (inspection.account.account_id.clone(), inspection.account))
		.collect::<BTreeMap<_, _>>();
	if !initial_by_id.keys().all(|account_id| expected_ids.contains(account_id)) {
		return Err(account_migration_error_at(
			"unexpected_account",
			OfflineAccountMigrationError::DestinationMismatch,
		));
	}

	for account in &manifest.accounts {
		let prepared = prepared_credentials
			.remove(&account.account_id)
			.ok_or(OfflineAccountMigrationError::InvalidManifest)?;
		let target =
			account.target.as_ref().ok_or(OfflineAccountMigrationError::InvalidManifest)?;
		if &prepared.target != target || &prepared.provider != &target.provider {
			return Err(OfflineAccountMigrationError::SourceChanged);
		}
		let transition = persisted_transitions
			.remove(&account.account_id)
			.ok_or(OfflineAccountMigrationError::InvalidManifest)?;
		let transition = match transition {
			Some(transition) => Some(transition),
			None => match initial_by_id.get(&account.account_id) {
				Some(record) if record.tombstoned || record.unsettled_operation.is_some() =>
					return Err(account_migration_error_at(
						"account_classification",
						OfflineAccountMigrationError::DestinationMismatch,
					)),
				Some(record) if record.credential.is_some() => {
					verify_account_destination_binding(account, record, credentials.as_ref())
						.map_err(|error| account_migration_error_at("existing_binding", error))?;
					None
				},
				Some(record) => {
					require_credential_absent(account, credentials.as_ref()).map_err(|error| {
						account_migration_error_at("existing_credential_absence", error)
					})?;
					Some(AccountMigrationTransition::ExistingHydrate {
						revision: record.revision,
						display_label: record.label.clone(),
						enabled: record.enabled,
					})
				},
				None => {
					require_credential_absent(account, credentials.as_ref()).map_err(|error| {
						account_migration_error_at("new_credential_absence", error)
					})?;
					Some(AccountMigrationTransition::AbsentInitialize { expected_revision: None })
				},
			},
		};
		let expected_final_revision = transition
			.as_ref()
			.map(|transition| {
				expected_migration_final_revision(
					transition,
					&account.display_label,
					account.enabled,
				)
			})
			.transpose()
			.map_err(|error| account_migration_error_at("expected_final_revision", error))?;
		let record = match transition {
			Some(transition) => service
				.install_migrated_credentials(
					account.operation_id.clone(),
					account.account_id.clone(),
					transition,
					account.display_label.clone(),
					account.enabled,
					prepared.provider,
					prepared.target,
					prepared.bundle,
				)
				.await
				.map_err(|_| {
					account_migration_error_at(
						"credential_install",
						OfflineAccountMigrationError::DestinationMismatch,
					)
				})?,
			None => initial_by_id.get(&account.account_id).cloned().ok_or_else(|| {
				account_migration_error_at(
					"missing_replay_account",
					OfflineAccountMigrationError::DestinationMismatch,
				)
			})?,
		};
		match service
			.update_administration(
				&account.account_id,
				record.revision,
				Some(&account.display_label),
				Some(account.enabled),
			)
			.await
			.map_err(|_| {
				account_migration_error_at(
					"administration_update",
					OfflineAccountMigrationError::DestinationMismatch,
				)
			})? {
			AccountAdministrationOutcome::Updated { .. } => {},
			AccountAdministrationOutcome::Rejected { .. } =>
				return Err(account_migration_error_at(
					"administration_rejected",
					OfflineAccountMigrationError::DestinationMismatch,
				)),
		}
		let record = service
			.inspect(&account.account_id)
			.await
			.map_err(|_| {
				account_migration_error_at(
					"administration_readback",
					OfflineAccountMigrationError::DestinationMismatch,
				)
			})?
			.account;
		if expected_final_revision.is_some_and(|revision| record.revision != revision) {
			return Err(account_migration_error_at(
				"revision_mismatch",
				OfflineAccountMigrationError::DestinationMismatch,
			));
		}
		verify_account_destination(account, &record, credentials.as_ref())
			.map_err(|error| account_migration_error_at("destination_binding", error))?;
		#[cfg(feature = "account-migration-transition-gate")]
		account_migration_transition_checkpoint(
			"administration_applied",
			Some(&account.account_id),
		)?;
	}

	PostgresStore::replace_account_routing_for_migration_explicit(
		config.postgres(),
		migration_password.as_deref().map(String::as_str),
		&manifest.routing,
		&manifest.order,
	)
	.await
	.map_err(|_| {
		account_migration_error_at(
			"routing_replace",
			OfflineAccountMigrationError::DestinationMismatch,
		)
	})?;
	#[cfg(feature = "account-migration-transition-gate")]
	account_migration_transition_checkpoint("routing_applied", None)?;

	let (final_accounts, final_routing) = service.list_snapshot().await.map_err(|_| {
		account_migration_error_at(
			"routing_readback",
			OfflineAccountMigrationError::DestinationMismatch,
		)
	})?;
	if final_routing.mode != manifest.routing
		|| final_routing.order != manifest.order
		|| final_accounts.len() != manifest.accounts.len()
	{
		return Err(account_migration_error_at(
			"routing_projection",
			OfflineAccountMigrationError::DestinationMismatch,
		));
	}
	verify_destination_accounts(&manifest, final_accounts, credentials.as_ref())
		.map_err(|error| account_migration_error_at("destination_accounts", error))?;
	verify_sources(&manifest.sources)?;
	verify_credential_sources(&options.credential_directory, &manifest.accounts)?;

	let report = OfflineAccountMigrationReport {
		schema: "decodex/account-migration-result/1",
		outcome: "destinations_verified",
		manifest_sha256: manifest.digest,
		account_count: manifest.accounts.len(),
		account_ids: manifest
			.accounts
			.iter()
			.map(|account| account.account_id.as_str().to_owned())
			.collect(),
		intent_recorded,
		receipt_completed: false,
	};
	drop(installer_lock);
	Ok(report)
}

struct VerifiedPreparedDestination {
	manifest: ValidatedManifest,
	service: AccountService,
	routing: decodex_core::AccountRoutingControl,
	destination_accounts: Vec<Value>,
	intent_recorded: bool,
}

async fn verify_prepared_destination(
	config_path: &Path,
	manifest_path: &Path,
	config: &DecodexConfig,
	launch_agent: &[u8],
) -> Result<VerifiedPreparedDestination, OfflineAccountMigrationError> {
	let manifest_bytes = read_private_file(manifest_path, MAX_MANIFEST_BYTES)?;
	let manifest = parse_manifest(&manifest_bytes)?;
	verify_manifest_daemon_wrapper(&manifest, launch_agent)?;
	let root = config_path.parent().ok_or(OfflineAccountMigrationError::InvalidPath)?;
	let paths = DecodexRoot::new(root.to_path_buf())
		.map_err(|_| OfflineAccountMigrationError::InvalidPath)?
		.paths();
	if config_path != paths.config_file() {
		return Err(OfflineAccountMigrationError::RuntimeRetirementUnverified);
	}
	let migration_password = credential(config.postgres().migration())?;
	let runtime_password = credential(config.postgres().runtime())?;
	PostgresStore::migrate_and_provision_explicit(
		config.postgres(),
		migration_password.as_deref().map(String::as_str),
	)
	.await
	.map_err(|_| OfflineAccountMigrationError::PostgresUnavailable)?;
	let store = PostgresStore::connect_explicit(
		config.postgres(),
		migration_password.as_deref().map(String::as_str),
		runtime_password.as_deref().map(String::as_str),
	)
	.await
	.map_err(|_| OfflineAccountMigrationError::PostgresUnavailable)?;
	let credentials = Arc::new(
		MacosKeychainCredentialStore::new(&paths)
			.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?,
	);
	let credential_store: Arc<dyn HostCredentialStore> = credentials.clone();
	let service = AccountService::new(store, credential_store, Arc::new(OfflineRefresher));
	let account_count = u32::try_from(manifest.accounts.len())
		.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
	let intent_recorded = service
		.prepare_migration_intent(&manifest.digest, &manifest.raw, account_count)
		.await
		.map_err(|_| OfflineAccountMigrationError::ReceiptConflict)?;
	let (accounts, routing) = service
		.list_snapshot()
		.await
		.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?;
	verify_prepared_operation_revisions(&manifest, &service, &accounts).await?;
	if accounts.len() != manifest.accounts.len()
		|| routing.mode != manifest.routing
		|| routing.order != manifest.order
	{
		return Err(OfflineAccountMigrationError::DestinationMismatch);
	}
	let destination_accounts =
		verify_destination_accounts(&manifest, accounts, credentials.as_ref())?;
	Ok(VerifiedPreparedDestination {
		manifest,
		service,
		routing,
		destination_accounts,
		intent_recorded,
	})
}

/// Verify the frozen prepared destination without reopening retired credential sources.
pub async fn verify_prepared_offline_account_migration_destination(
	options: OfflineAccountMigrationDestinationVerifyOptions,
) -> Result<OfflineAccountMigrationReport, OfflineAccountMigrationError> {
	#[cfg(feature = "account-migration-transition-gate")]
	reject_aliased_account_migration_descriptors(
		options.installer_lock_fd,
		options.transition_gate_fd,
	)?;
	let installer_lock =
		validate_installer_namespace_lock(&options.config, options.installer_lock_fd)?;
	#[cfg(feature = "account-migration-transition-gate")]
	{
		configure_account_migration_transition_gate(options.transition_gate_fd)?;
		verify_installer_lock_cloexec(&installer_lock)?;
	}
	if [&options.config, &options.manifest, &options.launch_agent]
		.into_iter()
		.any(|path| !path.is_absolute())
	{
		return Err(OfflineAccountMigrationError::InvalidPath);
	}
	let config_bytes = read_private_file(&options.config, MAX_MANIFEST_BYTES)?;
	let config = DecodexConfig::parse(&config_bytes)
		.map_err(|_| OfflineAccountMigrationError::InvalidConfig)?;
	validate_configured_local_authority(&config)?;
	let launch_agent = read_private_file(&options.launch_agent, MAX_LAUNCH_AGENT_BYTES)?;
	verify_retired_runtime_inputs(&config_bytes)?;
	verify_retired_runtime_inputs(&launch_agent)?;
	let VerifiedPreparedDestination { manifest, intent_recorded, .. } =
		verify_prepared_destination(&options.config, &options.manifest, &config, &launch_agent)
			.await?;
	#[cfg(feature = "account-migration-transition-gate")]
	account_migration_transition_checkpoint("prepared_destination_verified", None)?;
	let account_count = manifest.accounts.len();
	let account_ids =
		manifest.accounts.iter().map(|account| account.account_id.as_str().to_owned()).collect();
	let report = OfflineAccountMigrationReport {
		schema: "decodex/account-migration-result/1",
		outcome: "destinations_verified",
		manifest_sha256: manifest.digest,
		account_count,
		account_ids,
		intent_recorded,
		receipt_completed: false,
	};
	drop(installer_lock);
	Ok(report)
}

/// Complete the singleton receipt only after the installer swaps configuration and retires staging.
pub async fn finalize_offline_account_migration(
	options: OfflineAccountMigrationFinalizeOptions,
) -> Result<OfflineAccountMigrationReport, OfflineAccountMigrationError> {
	#[cfg(feature = "account-migration-transition-gate")]
	reject_aliased_account_migration_descriptors(
		options.installer_lock_fd,
		options.transition_gate_fd,
	)?;
	let installer_lock =
		validate_installer_namespace_lock(&options.config, options.installer_lock_fd)?;
	#[cfg(feature = "account-migration-transition-gate")]
	{
		configure_account_migration_transition_gate(options.transition_gate_fd)?;
		verify_installer_lock_cloexec(&installer_lock)?;
	}
	validate_finalize_paths(&options)?;
	let config_bytes = read_private_file(&options.config, MAX_MANIFEST_BYTES)?;
	let config = DecodexConfig::parse(&config_bytes)
		.map_err(|_| OfflineAccountMigrationError::InvalidConfig)?;
	validate_configured_local_authority(&config)?;
	let launch_agent = read_private_file(&options.launch_agent, MAX_LAUNCH_AGENT_BYTES)?;
	verify_retired_runtime_inputs(&config_bytes)?;
	verify_retired_runtime_inputs(&launch_agent)?;
	let VerifiedPreparedDestination {
		manifest,
		service,
		routing,
		destination_accounts,
		intent_recorded,
	} = verify_prepared_destination(&options.config, &options.manifest, &config, &launch_agent)
		.await?;
	#[cfg(feature = "account-migration-transition-gate")]
	account_migration_transition_checkpoint("final_destination_verified", None)?;
	let expected_retired_active_sources = manifest
		.sources
		.iter()
		.filter(|source| {
			matches!(source.role.as_str(), "vnext_uuid_bridge" | "vnext_account_config")
		})
		.map(|source| PathBuf::from(&source.path))
		.collect::<Vec<_>>();
	if options.retired_active_sources != expected_retired_active_sources {
		return Err(OfflineAccountMigrationError::RuntimeRetirementUnverified);
	}
	verify_absent_exact(&options.retired_staging_config)?;
	verify_absent_exact(&options.retired_credential_directory)?;
	for path in &options.retired_active_sources {
		verify_absent_exact(path)?;
	}
	let assets = verify_installed_assets(&options.installed_assets)?;
	verify_daemon_wrapper_installed_asset(&manifest, &assets)?;

	let account_count = u32::try_from(manifest.accounts.len())
		.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
	let (routing_mode, fixed_account_id) = match &routing.mode {
		AccountSelectionMode::Balanced => ("balanced", None),
		AccountSelectionMode::Fixed(account_id) => ("fixed", Some(account_id.as_str())),
	};
	let destination_receipt = json!({
		"schema": "decodex/account-migration-destination/1",
		"keychain_verified": true,
		"postgresql_verified": true,
		"routing": {
			"mode": routing_mode,
			"fixed_account_id": fixed_account_id,
			"order": routing.order.iter().map(AccountId::as_str).collect::<Vec<_>>(),
			"revision": routing.revision,
		},
		"accounts": destination_accounts,
	});
	let final_config_path =
		options.config.to_str().ok_or(OfflineAccountMigrationError::InvalidPath)?;
	let launch_agent_path =
		options.launch_agent.to_str().ok_or(OfflineAccountMigrationError::InvalidPath)?;
	let retired_staging_config =
		options.retired_staging_config.to_str().ok_or(OfflineAccountMigrationError::InvalidPath)?;
	let retired_credential_directory = options
		.retired_credential_directory
		.to_str()
		.ok_or(OfflineAccountMigrationError::InvalidPath)?;
	let retired_active_sources = options
		.retired_active_sources
		.iter()
		.map(|path| {
			path.to_str().map(str::to_owned).ok_or(OfflineAccountMigrationError::InvalidPath)
		})
		.collect::<Result<Vec<_>, _>>()?;
	let retirement_receipt = json!({
		"schema": "decodex/account-runtime-retirement/1",
		"legacy_source_untouched": true,
		"runtime_legacy_authority_removed": true,
		"final_config_swapped": true,
		"staging_secrets_retired": true,
		"active_legacy_sources_retired": true,
		"installed_assets_verified": true,
		"daemon_wrapper_verified": true,
		"daemon_wrapper_identity_sha256": daemon_wrapper_descriptor_sha256(
			&manifest.daemon_wrapper,
		)
		.map_err(|_| OfflineAccountMigrationError::RuntimeRetirementUnverified)?,
		"final_config_path": final_config_path,
		"final_config_sha256": sha256(&config_bytes),
		"launch_agent_path": launch_agent_path,
		"launch_agent_sha256": sha256(&launch_agent),
		"retired_staging_config": retired_staging_config,
		"retired_credential_directory": retired_credential_directory,
		"retired_active_sources": retired_active_sources,
		"installed_assets": assets,
		"supervisor_profile": "postgres_and_daemon_only_v1",
	});
	let manifest_sha256 = manifest.digest.clone();
	let completed_account_count = manifest.accounts.len();
	let completed_account_ids =
		manifest.accounts.iter().map(|account| account.account_id.as_str().to_owned()).collect();
	let receipt = AccountMigrationReceipt {
		manifest_sha256: manifest_sha256.clone(),
		manifest: manifest.raw,
		destination_receipt,
		retirement_receipt,
		account_count,
	};
	service
		.record_migration_receipt(&receipt)
		.await
		.map_err(|_| OfflineAccountMigrationError::ReceiptConflict)?;
	#[cfg(feature = "account-migration-transition-gate")]
	account_migration_transition_checkpoint("receipt_completed", None)?;

	let report = OfflineAccountMigrationReport {
		schema: "decodex/account-migration-result/1",
		outcome: "verified",
		manifest_sha256,
		account_count: completed_account_count,
		account_ids: completed_account_ids,
		intent_recorded,
		receipt_completed: true,
	};
	drop(installer_lock);
	Ok(report)
}

/// Verify a completed cutover without reopening any legacy source or bridge file.
pub async fn verify_completed_offline_account_migration(
	options: OfflineAccountMigrationVerifyOptions,
) -> Result<OfflineAccountMigrationReport, OfflineAccountMigrationError> {
	#[cfg(feature = "account-migration-transition-gate")]
	reject_aliased_account_migration_descriptors(
		options.installer_lock_fd,
		options.transition_gate_fd,
	)?;
	let installer_lock =
		validate_installer_namespace_lock(&options.config, options.installer_lock_fd)?;
	#[cfg(feature = "account-migration-transition-gate")]
	{
		configure_account_migration_transition_gate(options.transition_gate_fd)?;
		verify_installer_lock_cloexec(&installer_lock)?;
	}
	validate_verify_paths(&options)?;
	let config_bytes = read_private_file(&options.config, MAX_MANIFEST_BYTES)?;
	let config = DecodexConfig::parse(&config_bytes)
		.map_err(|_| OfflineAccountMigrationError::InvalidConfig)?;
	validate_configured_local_authority(&config)?;
	let launch_agent = read_private_file(&options.launch_agent, MAX_LAUNCH_AGENT_BYTES)?;
	verify_retired_runtime_inputs(&config_bytes)?;
	verify_retired_runtime_inputs(&launch_agent)?;
	verify_absent_exact(&options.retired_staging_config)?;
	verify_absent_exact(&options.retired_credential_directory)?;
	for path in &options.retired_active_sources {
		verify_absent_exact(path)?;
	}
	let migration_password = credential(config.postgres().migration())?;
	let runtime_password = credential(config.postgres().runtime())?;
	PostgresStore::migrate_and_provision_explicit(
		config.postgres(),
		migration_password.as_deref().map(String::as_str),
	)
	.await
	.map_err(|_| OfflineAccountMigrationError::PostgresUnavailable)?;
	let receipt = PostgresStore::read_completed_account_migration_receipt_explicit(
		config.postgres(),
		migration_password.as_deref().map(String::as_str),
	)
	.await
	.map_err(|_| OfflineAccountMigrationError::PostgresUnavailable)?
	.ok_or(OfflineAccountMigrationError::ReceiptConflict)?;
	let verified = verify_completed_receipt(&receipt)?;
	verify_manifest_daemon_wrapper(&verified.manifest, &launch_agent)?;
	let current_installed_assets = verify_installed_assets(&options.installed_assets)?;
	verify_daemon_wrapper_installed_asset(&verified.manifest, &current_installed_assets)?;
	let final_config_path =
		options.config.to_str().ok_or(OfflineAccountMigrationError::InvalidPath)?;
	let launch_agent_path =
		options.launch_agent.to_str().ok_or(OfflineAccountMigrationError::InvalidPath)?;
	let retired_staging_config =
		options.retired_staging_config.to_str().ok_or(OfflineAccountMigrationError::InvalidPath)?;
	let retired_credential_directory = options
		.retired_credential_directory
		.to_str()
		.ok_or(OfflineAccountMigrationError::InvalidPath)?;
	let retired_active_sources = options
		.retired_active_sources
		.iter()
		.map(|path| {
			path.to_str().map(str::to_owned).ok_or(OfflineAccountMigrationError::InvalidPath)
		})
		.collect::<Result<Vec<_>, _>>()?;
	if verified.retirement.final_config_path != final_config_path
		|| sha256(&config_bytes) != verified.retirement.final_config_sha256
		|| verified.retirement.launch_agent_path != launch_agent_path
		|| sha256(&launch_agent) != verified.retirement.launch_agent_sha256
		|| verified.retirement.retired_staging_config != retired_staging_config
		|| verified.retirement.retired_credential_directory != retired_credential_directory
		|| verified.retirement.retired_active_sources != retired_active_sources
		|| current_installed_assets != verified.retirement.installed_assets
	{
		return Err(OfflineAccountMigrationError::RuntimeRetirementUnverified);
	}

	let store = PostgresStore::connect_explicit(
		config.postgres(),
		migration_password.as_deref().map(String::as_str),
		runtime_password.as_deref().map(String::as_str),
	)
	.await
	.map_err(|_| OfflineAccountMigrationError::PostgresUnavailable)?;
	let root = options.config.parent().ok_or(OfflineAccountMigrationError::InvalidPath)?;
	let paths = DecodexRoot::new(root.to_path_buf())
		.map_err(|_| OfflineAccountMigrationError::InvalidPath)?
		.paths();
	if options.config != paths.config_file() {
		return Err(OfflineAccountMigrationError::RuntimeRetirementUnverified);
	}
	let credentials = Arc::new(
		MacosKeychainCredentialStore::new(&paths)
			.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?,
	);
	let credential_store: Arc<dyn HostCredentialStore> = credentials.clone();
	let service = AccountService::new(store, credential_store, Arc::new(OfflineRefresher));
	let (accounts, routing) = service
		.list_snapshot()
		.await
		.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?;
	let current_destination = completed_destination_receipt(
		&verified.manifest,
		accounts,
		&routing,
		credentials.as_ref(),
	)?;
	if current_destination != verified.destination {
		return Err(OfflineAccountMigrationError::DestinationMismatch);
	}
	#[cfg(feature = "account-migration-transition-gate")]
	account_migration_transition_checkpoint("completed_verified", None)?;
	let account_ids = verified
		.manifest
		.accounts
		.iter()
		.map(|account| account.account_id.as_str().to_owned())
		.collect::<Vec<_>>();
	let account_count = account_ids.len();

	let report = OfflineAccountMigrationReport {
		schema: "decodex/account-migration-result/1",
		outcome: "verified",
		manifest_sha256: receipt.manifest_sha256,
		account_count,
		account_ids,
		intent_recorded: false,
		receipt_completed: true,
	};
	drop(installer_lock);
	Ok(report)
}

struct VerifiedCompletedReceipt {
	manifest: ValidatedManifest,
	destination: CompletedDestinationReceipt,
	retirement: CompletedRetirementReceipt,
}

fn verify_completed_receipt(
	receipt: &AccountMigrationReceipt,
) -> Result<VerifiedCompletedReceipt, OfflineAccountMigrationError> {
	let manifest_bytes = serde_json::to_vec(&receipt.manifest)
		.map_err(|_| OfflineAccountMigrationError::ReceiptConflict)?;
	let manifest = parse_manifest(&manifest_bytes)?;
	if manifest.digest != receipt.manifest_sha256
		|| manifest.accounts.len()
			!= usize::try_from(receipt.account_count)
				.map_err(|_| OfflineAccountMigrationError::ReceiptConflict)?
	{
		return Err(OfflineAccountMigrationError::ReceiptConflict);
	}
	let destination: CompletedDestinationReceipt =
		serde_json::from_value(receipt.destination_receipt.clone())
			.map_err(|_| OfflineAccountMigrationError::ReceiptConflict)?;
	let retirement: CompletedRetirementReceipt =
		serde_json::from_value(receipt.retirement_receipt.clone())
			.map_err(|_| OfflineAccountMigrationError::ReceiptConflict)?;
	let manifest_ids =
		manifest.accounts.iter().map(|account| account.account_id.clone()).collect::<BTreeSet<_>>();
	let manifest_accounts = manifest
		.accounts
		.iter()
		.map(|account| (account.account_id.clone(), account))
		.collect::<BTreeMap<_, _>>();
	let (routing_mode, fixed_account_id) = match &manifest.routing {
		AccountSelectionMode::Balanced => ("balanced", None),
		AccountSelectionMode::Fixed(account_id) => ("fixed", Some(account_id.as_str().to_owned())),
	};
	let routing_order =
		manifest.order.iter().map(|account_id| account_id.as_str().to_owned()).collect::<Vec<_>>();
	let mut destination_ids = BTreeSet::new();
	if destination.schema != "decodex/account-migration-destination/1"
		|| !destination.keychain_verified
		|| !destination.postgresql_verified
		|| destination.routing.revision < 1
		|| destination.routing.mode != routing_mode
		|| destination.routing.fixed_account_id != fixed_account_id
		|| destination.routing.order != routing_order
		|| destination.accounts.len() != manifest_ids.len()
		|| destination.accounts.iter().any(|account| {
			let Ok(account_id) = AccountId::new(account.account_id.clone()) else {
				return true;
			};
			let Some(manifest_account) = manifest_accounts.get(&account_id) else {
				return true;
			};
			let Some(target) = manifest_account.target.as_ref() else {
				return true;
			};
			!destination_ids.insert(account_id)
				|| account.revision < 1
				|| account.display_label != manifest_account.display_label
				|| account.enabled != manifest_account.enabled
				|| account.provider != "chatgpt"
				|| account.provider_account_id != target.provider.account_id()
				|| account.host_store != TARGET_HOST_STORE
				|| account.postgres_projection != TARGET_POSTGRES_PROJECTION
				|| account.store_schema_version != u32::from(target.schema_version.get())
				|| account.credential_version != target.version.get()
				|| account.fingerprint_sha256 != target.fingerprint.as_str()
				|| account.writer_operation_id != target.writer_operation_id.as_str()
				|| account.provider_account_id_sha256 != manifest_account.provider_account_id_sha256
		}) || destination_ids != manifest_ids
	{
		return Err(OfflineAccountMigrationError::ReceiptConflict);
	}
	let expected_retired_active_sources = manifest
		.sources
		.iter()
		.filter(|source| {
			matches!(source.role.as_str(), "vnext_uuid_bridge" | "vnext_account_config")
		})
		.map(|source| source.path.clone())
		.collect::<Vec<_>>();
	let retirement_paths = [
		retirement.final_config_path.as_str(),
		retirement.launch_agent_path.as_str(),
		retirement.retired_staging_config.as_str(),
		retirement.retired_credential_directory.as_str(),
	];
	let unique_retirement_path_count =
		retirement_paths.iter().copied().collect::<BTreeSet<_>>().len();
	let mut asset_names = BTreeSet::new();
	let mut asset_paths = BTreeSet::new();
	let mut daemon_wrapper_assets = retirement
		.installed_assets
		.iter()
		.filter(|asset| asset.path == manifest.daemon_wrapper.executable_path());
	let daemon_wrapper_asset_verified = daemon_wrapper_assets.next().is_some_and(|asset| {
		asset.sha256 == manifest.daemon_wrapper.executable_sha256()
			&& asset.byte_count == manifest.daemon_wrapper.executable_byte_count()
	}) && daemon_wrapper_assets.next().is_none();
	if retirement.schema != "decodex/account-runtime-retirement/1"
		|| !retirement.legacy_source_untouched
		|| !retirement.runtime_legacy_authority_removed
		|| !retirement.final_config_swapped
		|| !retirement.staging_secrets_retired
		|| !retirement.active_legacy_sources_retired
		|| !retirement.installed_assets_verified
		|| !retirement.daemon_wrapper_verified
		|| !daemon_wrapper_asset_verified
		|| daemon_wrapper_descriptor_sha256(&manifest.daemon_wrapper).ok().as_deref()
			!= Some(retirement.daemon_wrapper_identity_sha256.as_str())
		|| retirement.supervisor_profile != "postgres_and_daemon_only_v1"
		|| retirement_paths.iter().any(|path| {
			!Path::new(*path).is_absolute() || path.is_empty() || path.chars().any(char::is_control)
		}) || unique_retirement_path_count != retirement_paths.len()
		|| retirement.retired_active_sources != expected_retired_active_sources
		|| retirement.retired_active_sources.iter().any(|path| {
			!Path::new(path).is_absolute() || path.is_empty() || path.chars().any(char::is_control)
		}) || !valid_digest(&retirement.final_config_sha256)
		|| !valid_digest(&retirement.launch_agent_sha256)
		|| retirement.installed_assets.is_empty()
		|| retirement.installed_assets.len() > 16
		|| retirement.installed_assets.iter().any(|asset| {
			asset.name.is_empty()
				|| asset.name.len() > 128
				|| asset.name.chars().any(char::is_control)
				|| !Path::new(&asset.path).is_absolute()
				|| asset.path.chars().any(char::is_control)
				|| !asset_names.insert(asset.name.clone())
				|| !asset_paths.insert(asset.path.clone())
				|| !valid_digest(&asset.sha256)
				|| asset.byte_count == 0
		}) {
		return Err(OfflineAccountMigrationError::ReceiptConflict);
	}
	Ok(VerifiedCompletedReceipt { manifest, destination, retirement })
}

fn parse_source_manifest(bytes: &[u8]) -> Result<ValidatedManifest, OfflineAccountMigrationError> {
	parse_manifest_inner(bytes, false)
}

fn parse_manifest(bytes: &[u8]) -> Result<ValidatedManifest, OfflineAccountMigrationError> {
	parse_manifest_inner(bytes, true)
}

fn parse_manifest_inner(
	bytes: &[u8],
	require_targets: bool,
) -> Result<ValidatedManifest, OfflineAccountMigrationError> {
	let raw: Value =
		serde_json::from_slice(bytes).map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
	let parsed: MigrationManifest = serde_json::from_value(raw.clone())
		.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
	if parsed.schema != MANIFEST_SCHEMA
		|| parsed.quota_policy != "reset_to_unknown"
		|| parsed.usage_profile_policy != "start_empty"
		|| parsed.history_policy != "do_not_import"
		|| parsed.sources.len() != SOURCE_ROLES.len()
		|| parsed.accounts.len() > MAX_ACCOUNTS
	{
		return Err(OfflineAccountMigrationError::InvalidManifest);
	}
	let expected_decisions = decision_fingerprints(&parsed)?;
	if parsed.decision_fingerprints != expected_decisions {
		return Err(OfflineAccountMigrationError::InvalidManifest);
	}
	daemon_wrapper_descriptor_sha256(&parsed.daemon_wrapper)
		.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
	let target_count = parsed.accounts.iter().filter(|account| account.target.is_some()).count();
	if target_count != 0 && target_count != parsed.accounts.len()
		|| (require_targets && target_count != parsed.accounts.len())
	{
		return Err(OfflineAccountMigrationError::InvalidManifest);
	}
	let roles = parsed.sources.iter().map(|source| source.role.as_str()).collect::<BTreeSet<_>>();
	if roles != SOURCE_ROLES.into_iter().collect()
		|| parsed.sources.iter().any(|source| {
			!Path::new(&source.path).is_absolute()
				|| source.path.len() > 4096
				|| source.path.chars().any(char::is_control)
				|| source.present
					!= (source.byte_count.is_some()
						&& source.sha256.as_deref().is_some_and(valid_digest))
				|| (!source.present && (source.byte_count.is_some() || source.sha256.is_some()))
		}) {
		return Err(OfflineAccountMigrationError::InvalidManifest);
	}
	let mut accounts = Vec::with_capacity(parsed.accounts.len());
	for (ordinal, account) in parsed.accounts.iter().enumerate() {
		if account.source_ordinal != ordinal
			|| account.provider != "chatgpt"
			|| !valid_digest(&account.provider_account_id_sha256)
			|| !valid_digest(&account.credential_source_sha256)
			|| account.display_label.is_empty()
			|| account.display_label.len() > 128
			|| account.display_label.chars().any(char::is_control)
		{
			return Err(OfflineAccountMigrationError::InvalidManifest);
		}
		let account_id = AccountId::new(account.account_id.clone())
			.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
		let operation_id = AccountOperationId::new(account.operation_id.clone())
			.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
		let target = account
			.target
			.as_ref()
			.map(|target| parse_manifest_target(target, &account.provider_account_id_sha256))
			.transpose()?;
		accounts.push(ParsedAccount {
			account_id,
			operation_id,
			provider_account_id_sha256: account.provider_account_id_sha256.clone(),
			display_label: account.display_label.clone(),
			enabled: account.enabled,
			credential_source_sha256: account.credential_source_sha256.clone(),
			target,
		});
	}
	let universe =
		accounts.iter().map(|account| account.account_id.clone()).collect::<BTreeSet<_>>();
	let operation_ids =
		accounts.iter().map(|account| account.operation_id.clone()).collect::<BTreeSet<_>>();
	if universe.len() != accounts.len() || operation_ids.len() != accounts.len() {
		return Err(OfflineAccountMigrationError::InvalidManifest);
	}
	let (routing, raw_order) = match &parsed.routing {
		MigrationRouting::Balanced { order } => (None, order.clone()),
		MigrationRouting::Fixed { account_id, order } => (Some(account_id.clone()), order.clone()),
	};
	let order = raw_order
		.into_iter()
		.map(|value| {
			AccountId::new(value).map_err(|_| OfflineAccountMigrationError::InvalidManifest)
		})
		.collect::<Result<Vec<_>, _>>()?;
	if order.iter().cloned().collect::<BTreeSet<_>>() != universe || order.len() != universe.len() {
		return Err(OfflineAccountMigrationError::InvalidManifest);
	}
	let routing = match routing {
		Some(value) => {
			let account_id =
				AccountId::new(value).map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
			if !universe.contains(&account_id) {
				return Err(OfflineAccountMigrationError::InvalidManifest);
			}
			AccountSelectionMode::Fixed(account_id)
		},
		None => AccountSelectionMode::Balanced,
	};
	let digest = json_digest(&raw)?;
	let daemon_wrapper = parsed.daemon_wrapper.clone();
	let sources = parsed.sources.clone();
	Ok(ValidatedManifest {
		raw,
		document: parsed,
		digest,
		daemon_wrapper,
		sources,
		accounts,
		routing,
		order,
	})
}

fn parse_manifest_target(
	target: &MigrationCredentialTarget,
	provider_account_id_sha256: &str,
) -> Result<CredentialBinding, OfflineAccountMigrationError> {
	if target.host_store != TARGET_HOST_STORE
		|| target.postgres_projection != TARGET_POSTGRES_PROJECTION
		|| sha256(target.provider_account_id.as_bytes()) != provider_account_id_sha256
	{
		return Err(OfflineAccountMigrationError::InvalidManifest);
	}
	let provider =
		ProviderIdentity::new(AccountProvider::Chatgpt, target.provider_account_id.clone())
			.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
	Ok(CredentialBinding {
		schema_version: CredentialStoreSchemaVersion::new(target.store_schema_version)
			.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?,
		version: CredentialVersion::new(target.credential_version)
			.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?,
		fingerprint: CredentialFingerprint::new(target.fingerprint_sha256.clone())
			.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?,
		provider,
		writer_operation_id: AccountOperationId::new(target.writer_operation_id.clone())
			.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?,
	})
}

fn load_migration_credentials(
	options: &OfflineAccountMigrationOptions,
	manifest: &ValidatedManifest,
) -> Result<BTreeMap<AccountId, PreparedMigrationCredential>, OfflineAccountMigrationError> {
	let mut prepared = BTreeMap::new();
	for account in &manifest.accounts {
		let source =
			options.credential_directory.join(format!("{}.json", account.account_id.as_str()));
		if sha256_file(&source, MAX_CREDENTIAL_SOURCE_BYTES)? != account.credential_source_sha256 {
			return Err(OfflineAccountMigrationError::SourceChanged);
		}
		let imported = read_explicit_credential_file(
			source.to_str().ok_or(OfflineAccountMigrationError::InvalidPath)?,
		)
		.map_err(|_| OfflineAccountMigrationError::InvalidCredentialSource)?;
		if sha256(imported.provider.account_id().as_bytes()) != account.provider_account_id_sha256 {
			return Err(OfflineAccountMigrationError::InvalidCredentialSource);
		}
		let version = account
			.target
			.as_ref()
			.map_or_else(|| CredentialVersion::new(1), |target| Ok(target.version))
			.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
		let writer_operation_id = account
			.target
			.as_ref()
			.map_or(&account.operation_id, |target| &target.writer_operation_id);
		let derived = imported
			.bundle
			.binding_for(&account.account_id, writer_operation_id, version, &imported.provider)
			.map_err(|_| OfflineAccountMigrationError::InvalidCredentialSource)?;
		let target = match account.target.as_ref() {
			Some(target) if target == &derived => target.clone(),
			Some(_) => return Err(OfflineAccountMigrationError::SourceChanged),
			None => derived,
		};
		if prepared
			.insert(
				account.account_id.clone(),
				PreparedMigrationCredential {
					provider: imported.provider,
					bundle: imported.bundle,
					target,
				},
			)
			.is_some()
		{
			return Err(OfflineAccountMigrationError::InvalidManifest);
		}
	}
	Ok(prepared)
}

fn freeze_manifest_targets(
	path: &Path,
	mut manifest: ValidatedManifest,
	prepared: &BTreeMap<AccountId, PreparedMigrationCredential>,
) -> Result<ValidatedManifest, OfflineAccountMigrationError> {
	if manifest.accounts.iter().all(|account| account.target.is_some()) {
		if manifest.accounts.iter().any(|account| {
			prepared
				.get(&account.account_id)
				.zip(account.target.as_ref())
				.is_none_or(|(credential, target)| &credential.target != target)
		}) {
			return Err(OfflineAccountMigrationError::SourceChanged);
		}
		return Ok(manifest);
	}
	for account in &mut manifest.document.accounts {
		let account_id = AccountId::new(account.account_id.clone())
			.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
		let target =
			&prepared.get(&account_id).ok_or(OfflineAccountMigrationError::InvalidManifest)?.target;
		account.target = Some(MigrationCredentialTarget {
			host_store: TARGET_HOST_STORE.to_owned(),
			postgres_projection: TARGET_POSTGRES_PROJECTION.to_owned(),
			store_schema_version: target.schema_version.get(),
			credential_version: target.version.get(),
			writer_operation_id: target.writer_operation_id.as_str().to_owned(),
			fingerprint_sha256: target.fingerprint.as_str().to_owned(),
			provider_account_id: target.provider.account_id().to_owned(),
		});
	}
	manifest.document.decision_fingerprints = decision_fingerprints(&manifest.document)?;
	let raw = canonical_json_value(
		serde_json::to_value(&manifest.document)
			.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?,
	);
	let mut bytes =
		serde_json::to_vec(&raw).map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
	bytes.push(b'\n');
	write_private_manifest(path, &bytes)?;
	parse_manifest(&bytes)
}

fn write_private_manifest(path: &Path, bytes: &[u8]) -> Result<(), OfflineAccountMigrationError> {
	if bytes.is_empty()
		|| u64::try_from(bytes.len()).ok().is_none_or(|length| length > MAX_MANIFEST_BYTES)
	{
		return Err(OfflineAccountMigrationError::InvalidManifest);
	}
	let parent = path.parent().ok_or(OfflineAccountMigrationError::InvalidPath)?;
	let parent_path_metadata =
		fs::symlink_metadata(parent).map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
	let directory = OpenOptions::new()
		.read(true)
		.custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
		.open(parent)
		.map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
	let parent_metadata =
		directory.metadata().map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
	let effective_uid = unsafe { libc::geteuid() };
	if parent_path_metadata.file_type().is_symlink()
		|| !parent_metadata.is_dir()
		|| parent_metadata.uid() != effective_uid
		|| parent_metadata.permissions().mode() & 0o7777 != 0o700
		|| parent_metadata.dev() != parent_path_metadata.dev()
		|| parent_metadata.ino() != parent_path_metadata.ino()
	{
		return Err(OfflineAccountMigrationError::InvalidPath);
	}
	let current =
		fs::symlink_metadata(path).map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
	if current.file_type().is_symlink()
		|| !current.is_file()
		|| current.uid() != effective_uid
		|| current.permissions().mode() & 0o7777 != 0o600
		|| current.nlink() != 1
	{
		return Err(OfflineAccountMigrationError::InvalidPath);
	}
	let name = path
		.file_name()
		.and_then(|value| value.to_str())
		.ok_or(OfflineAccountMigrationError::InvalidPath)?;
	let candidate = parent.join(format!(".{name}.target"));
	match fs::symlink_metadata(&candidate) {
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
		Ok(metadata)
			if metadata.is_file()
				&& !metadata.file_type().is_symlink()
				&& metadata.uid() == effective_uid
				&& metadata.permissions().mode() & 0o7777 == 0o600
				&& metadata.nlink() == 1 =>
			fs::remove_file(&candidate).map_err(|_| OfflineAccountMigrationError::InvalidPath)?,
		_ => return Err(OfflineAccountMigrationError::InvalidPath),
	}
	let mut candidate_created = false;
	let result = (|| {
		let mut file = OpenOptions::new()
			.write(true)
			.create_new(true)
			.mode(0o600)
			.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
			.open(&candidate)
			.map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
		candidate_created = true;
		file.write_all(bytes).map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
		file.sync_all().map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
		drop(file);
		fs::rename(&candidate, path).map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
		candidate_created = false;
		directory.sync_all().map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
		let readback = read_private_file(path, MAX_MANIFEST_BYTES)?;
		if readback.as_slice() != bytes {
			return Err(OfflineAccountMigrationError::SourceChanged);
		}
		Ok(())
	})();
	if candidate_created {
		if fs::remove_file(candidate).is_err() {
			return Err(OfflineAccountMigrationError::InvalidPath);
		}
	}
	result
}

fn decision_fingerprints(
	manifest: &MigrationManifest,
) -> Result<MigrationDecisionFingerprints, OfflineAccountMigrationError> {
	let credentials = manifest
		.accounts
		.iter()
		.map(|account| match account.target.as_ref() {
			Some(target) => json!({
				"account_id": account.account_id.as_str(),
				"credential_source_sha256": account.credential_source_sha256.as_str(),
				"target": target,
			}),
			None => json!({
				"account_id": account.account_id.as_str(),
				"credential_source_sha256": account.credential_source_sha256.as_str(),
			}),
		})
		.collect::<Vec<_>>();
	let labels = manifest
		.accounts
		.iter()
		.map(|account| {
			json!({
				"account_id": account.account_id.as_str(),
				"display_label": account.display_label.as_str(),
			})
		})
		.collect::<Vec<_>>();
	let enabled = manifest
		.accounts
		.iter()
		.map(|account| {
			json!({
				"account_id": account.account_id.as_str(),
				"enabled": account.enabled,
			})
		})
		.collect::<Vec<_>>();
	let provider = manifest
		.accounts
		.iter()
		.map(|account| {
			json!({
				"account_id": account.account_id.as_str(),
				"provider": account.provider.as_str(),
				"provider_account_id_sha256": account.provider_account_id_sha256.as_str(),
			})
		})
		.collect::<Vec<_>>();
	let routing = match &manifest.routing {
		MigrationRouting::Balanced { order } => json!({"mode": "balanced", "order": order}),
		MigrationRouting::Fixed { account_id, order } =>
			json!({"mode": "fixed", "account_id": account_id, "order": order}),
	};
	Ok(MigrationDecisionFingerprints {
		credentials_sha256: json_digest(&credentials)?,
		labels_sha256: json_digest(&labels)?,
		enabled_sha256: json_digest(&enabled)?,
		routing_sha256: json_digest(&routing)?,
		provider_sha256: json_digest(&provider)?,
		quota_sha256: json_digest(&json!({"policy": manifest.quota_policy.as_str()}))?,
		usage_profile_sha256: json_digest(
			&json!({"policy": manifest.usage_profile_policy.as_str()}),
		)?,
		history_sha256: json_digest(&json!({"policy": manifest.history_policy.as_str()}))?,
	})
}

fn json_digest(value: &impl Serialize) -> Result<String, OfflineAccountMigrationError> {
	let value =
		serde_json::to_value(value).map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
	serde_json::to_vec(&canonical_json_value(value))
		.map(|bytes| sha256(&bytes))
		.map_err(|_| OfflineAccountMigrationError::InvalidManifest)
}

fn canonical_json_value(value: Value) -> Value {
	match value {
		Value::Array(values) =>
			Value::Array(values.into_iter().map(canonical_json_value).collect()),
		Value::Object(values) => {
			let sorted = values
				.into_iter()
				.map(|(key, value)| (key, canonical_json_value(value)))
				.collect::<BTreeMap<_, _>>();
			Value::Object(sorted.into_iter().collect())
		},
		value => value,
	}
}

fn migration_transition_from_operation(
	expected: &ParsedAccount,
	operation: &AccountOperation,
) -> Result<AccountMigrationTransition, OfflineAccountMigrationError> {
	let target = expected.target.as_ref().ok_or(OfflineAccountMigrationError::InvalidManifest)?;
	if operation.operation_id != expected.operation_id
		|| operation.account_id != expected.account_id
		|| operation.kind != AccountOperationKind::Import
		|| operation.expected.is_some()
		|| operation.target.as_ref() != Some(target)
	{
		return Err(OfflineAccountMigrationError::DestinationMismatch);
	}
	match operation.expected_account_revision {
		None if operation.requested_display_label.as_deref()
			== Some(expected.display_label.as_str())
			&& operation.requested_enabled == Some(expected.enabled) =>
			Ok(AccountMigrationTransition::AbsentInitialize { expected_revision: None }),
		Some(revision) if revision > 0 => {
			let display_label = operation
				.requested_display_label
				.clone()
				.filter(|label| {
					!label.is_empty() && label.len() <= 128 && !label.chars().any(char::is_control)
				})
				.ok_or(OfflineAccountMigrationError::DestinationMismatch)?;
			let enabled = operation
				.requested_enabled
				.ok_or(OfflineAccountMigrationError::DestinationMismatch)?;
			Ok(AccountMigrationTransition::ExistingHydrate { revision, display_label, enabled })
		},
		_ => Err(OfflineAccountMigrationError::DestinationMismatch),
	}
}

fn expected_migration_final_revision(
	transition: &AccountMigrationTransition,
	desired_label: &str,
	desired_enabled: bool,
) -> Result<i64, OfflineAccountMigrationError> {
	match transition {
		AccountMigrationTransition::AbsentInitialize { expected_revision: None } => Ok(2),
		AccountMigrationTransition::AbsentInitialize { expected_revision: Some(_) } =>
			Err(OfflineAccountMigrationError::DestinationMismatch),
		AccountMigrationTransition::ExistingHydrate { revision, display_label, enabled } =>
			revision
				.checked_add(if display_label != desired_label || *enabled != desired_enabled {
					2
				} else {
					1
				})
				.ok_or(OfflineAccountMigrationError::DestinationMismatch),
	}
}

async fn verify_prepared_operation_revisions(
	manifest: &ValidatedManifest,
	service: &AccountService,
	accounts: &[crate::account_service::AccountInspection],
) -> Result<(), OfflineAccountMigrationError> {
	let records = accounts
		.iter()
		.map(|inspection| (&inspection.account.account_id, &inspection.account))
		.collect::<BTreeMap<_, _>>();
	for account in &manifest.accounts {
		let operation = service
			.read_migration_operation(&account.operation_id)
			.await
			.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?;
		let Some(operation) = operation else {
			continue;
		};
		if operation.phase != AccountOperationPhase::Committed {
			return Err(OfflineAccountMigrationError::DestinationMismatch);
		}
		let transition = migration_transition_from_operation(account, &operation)?;
		let expected_revision = expected_migration_final_revision(
			&transition,
			&account.display_label,
			account.enabled,
		)?;
		if records.get(&account.account_id).map(|record| record.revision) != Some(expected_revision)
		{
			return Err(OfflineAccountMigrationError::DestinationMismatch);
		}
	}
	Ok(())
}

fn require_credential_absent(
	expected: &ParsedAccount,
	credentials: &dyn HostCredentialStore,
) -> Result<(), OfflineAccountMigrationError> {
	let target = expected.target.as_ref().ok_or(OfflineAccountMigrationError::InvalidManifest)?;
	match credentials.read_exact(&expected.account_id, target) {
		Err(CredentialStoreError::NotFound) => Ok(()),
		_ => Err(OfflineAccountMigrationError::DestinationMismatch),
	}
}

fn verify_account_destination(
	expected: &ParsedAccount,
	record: &decodex_core::AccountRecord,
	credentials: &dyn HostCredentialStore,
) -> Result<(), OfflineAccountMigrationError> {
	if record.account_id != expected.account_id
		|| record.label != expected.display_label
		|| record.enabled != expected.enabled
		|| record.tombstoned
		|| record.unsettled_operation.is_some()
	{
		return Err(OfflineAccountMigrationError::DestinationMismatch);
	}
	verify_account_destination_binding(expected, record, credentials)
}

fn verify_account_destination_binding(
	expected: &ParsedAccount,
	record: &decodex_core::AccountRecord,
	credentials: &dyn HostCredentialStore,
) -> Result<(), OfflineAccountMigrationError> {
	let binding =
		record.credential.as_ref().ok_or(OfflineAccountMigrationError::DestinationMismatch)?;
	let target = expected.target.as_ref().ok_or(OfflineAccountMigrationError::InvalidManifest)?;
	if record.account_id != expected.account_id
		|| record.tombstoned
		|| record.unsettled_operation.is_some()
		|| binding != target
		|| sha256(binding.provider.account_id().as_bytes()) != expected.provider_account_id_sha256
	{
		return Err(OfflineAccountMigrationError::DestinationMismatch);
	}
	credentials
		.read_exact(&record.account_id, binding)
		.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)?;
	Ok(())
}

fn verify_destination_accounts(
	manifest: &ValidatedManifest,
	accounts: Vec<crate::account_service::AccountInspection>,
	credentials: &dyn HostCredentialStore,
) -> Result<Vec<Value>, OfflineAccountMigrationError> {
	let expected = manifest
		.accounts
		.iter()
		.map(|account| (account.account_id.clone(), account))
		.collect::<BTreeMap<_, _>>();
	let mut verified = Vec::with_capacity(accounts.len());
	for inspection in accounts {
		let record = inspection.account;
		let account = expected
			.get(&record.account_id)
			.ok_or(OfflineAccountMigrationError::DestinationMismatch)?;
		verify_account_destination(account, &record, credentials)?;
		let binding =
			record.credential.as_ref().ok_or(OfflineAccountMigrationError::DestinationMismatch)?;
		verified.push(json!({
			"account_id": record.account_id.as_str(),
			"display_label": record.label.as_str(),
			"enabled": record.enabled,
			"revision": record.revision,
			"provider": "chatgpt",
			"provider_account_id": binding.provider.account_id(),
			"host_store": TARGET_HOST_STORE,
			"postgres_projection": TARGET_POSTGRES_PROJECTION,
			"store_schema_version": binding.schema_version.get(),
			"credential_version": binding.version.get(),
			"fingerprint_sha256": binding.fingerprint.as_str(),
			"writer_operation_id": binding.writer_operation_id.as_str(),
			"provider_account_id_sha256": account.provider_account_id_sha256,
		}));
	}
	Ok(verified)
}

fn completed_destination_receipt(
	manifest: &ValidatedManifest,
	accounts: Vec<crate::account_service::AccountInspection>,
	routing: &decodex_core::AccountRoutingControl,
	credentials: &dyn HostCredentialStore,
) -> Result<CompletedDestinationReceipt, OfflineAccountMigrationError> {
	let accounts = verify_destination_accounts(manifest, accounts, credentials)?;
	let (mode, fixed_account_id) = match &routing.mode {
		AccountSelectionMode::Balanced => ("balanced", None),
		AccountSelectionMode::Fixed(account_id) => ("fixed", Some(account_id.as_str())),
	};
	serde_json::from_value(json!({
		"schema": "decodex/account-migration-destination/1",
		"keychain_verified": true,
		"postgresql_verified": true,
		"routing": {
			"mode": mode,
			"fixed_account_id": fixed_account_id,
			"order": routing.order.iter().map(AccountId::as_str).collect::<Vec<_>>(),
			"revision": routing.revision,
		},
		"accounts": accounts,
	}))
	.map_err(|_| OfflineAccountMigrationError::DestinationMismatch)
}

fn verify_credential_sources(
	directory: &Path,
	accounts: &[ParsedAccount],
) -> Result<(), OfflineAccountMigrationError> {
	verify_credential_directory(directory, accounts)?;
	for account in accounts {
		let source = directory.join(format!("{}.json", account.account_id.as_str()));
		if sha256_file(&source, MAX_CREDENTIAL_SOURCE_BYTES)? != account.credential_source_sha256 {
			return Err(OfflineAccountMigrationError::SourceChanged);
		}
	}
	Ok(())
}

fn verify_sources(sources: &[MigrationSource]) -> Result<(), OfflineAccountMigrationError> {
	for source in sources {
		let path = Path::new(&source.path);
		verify_migration_source_parents(path)?;
		if source.present {
			let bytes = read_private_file(path, MAX_SOURCE_BYTES)?;
			let digest = sha256(&bytes);
			if Some(
				u64::try_from(bytes.len())
					.map_err(|_| OfflineAccountMigrationError::SourceChanged)?,
			) != source.byte_count
				|| source.sha256.as_deref() != Some(digest.as_str())
			{
				return Err(OfflineAccountMigrationError::SourceChanged);
			}
		} else if fs::symlink_metadata(path).is_ok() {
			return Err(OfflineAccountMigrationError::SourceChanged);
		}
	}
	Ok(())
}

#[cfg(feature = "account-migration-transition-gate")]
pub(crate) fn verify_gate_credential_source(
	path: &Path,
) -> Result<(), OfflineAccountMigrationError> {
	verify_migration_source_parents(path)?;
	let _ = read_private_file(path, MAX_CREDENTIAL_SOURCE_BYTES)?;
	Ok(())
}

#[cfg(feature = "account-migration-transition-gate")]
pub(crate) fn read_account_migration_gate_file(
	path: &Path,
	max_bytes: u64,
) -> Result<Zeroizing<Vec<u8>>, OfflineAccountMigrationError> {
	verify_migration_source_parents(path)?;
	read_private_file(path, max_bytes)
}

#[cfg(feature = "account-migration-transition-gate")]
pub(crate) fn load_account_migration_gate_run(
	path: &Path,
) -> Result<AccountMigrationGateRun, OfflineAccountMigrationError> {
	let login_home = account_migration_gate_login_home()?;
	let fixture_root = path.parent().ok_or(OfflineAccountMigrationError::InvalidPath)?;
	let derived_run_id = fixture_root
		.file_name()
		.and_then(OsStr::to_str)
		.and_then(|name| name.strip_prefix(".xy1422-"))
		.ok_or(OfflineAccountMigrationError::InvalidPath)?;
	if path.file_name() != Some(OsStr::new(ACCOUNT_MIGRATION_GATE_RUN_FILE))
		|| fixture_root.parent() != Some(login_home.as_path())
		|| derived_run_id.len() != 16
		|| !derived_run_id
			.bytes()
			.all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
	{
		return Err(OfflineAccountMigrationError::InvalidPath);
	}
	let bytes = read_account_migration_gate_file(path, 4096)?;
	let descriptor: AccountMigrationGateRunDescriptor = serde_json::from_slice(&bytes)
		.map_err(|_| OfflineAccountMigrationError::InvalidManifest)?;
	if descriptor.schema != ACCOUNT_MIGRATION_GATE_RUN_SCHEMA || descriptor.run_id != derived_run_id
	{
		return Err(OfflineAccountMigrationError::InvalidManifest);
	}
	let fixture_root = fixture_root.to_path_buf();
	let root = fixture_root.join(".decodex");
	if root == login_home.join(".decodex") || !root.starts_with(&fixture_root) {
		return Err(OfflineAccountMigrationError::InvalidPath);
	}
	// SAFETY: `geteuid` has no arguments and cannot fail.
	let effective_uid = unsafe { libc::geteuid() };
	for directory in [&fixture_root, &root] {
		let metadata = fs::symlink_metadata(directory)
			.map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
		if metadata.file_type().is_symlink()
			|| !metadata.is_dir()
			|| metadata.uid() != effective_uid
			|| metadata.permissions().mode() & 0o7777 != 0o700
		{
			return Err(OfflineAccountMigrationError::InvalidPath);
		}
	}
	let paths =
		DecodexRoot::new(root).map_err(|_| OfflineAccountMigrationError::InvalidPath)?.paths();
	Ok(AccountMigrationGateRun { run_id: descriptor.run_id, fixture_root, paths })
}

#[cfg(feature = "account-migration-transition-gate")]
pub(crate) fn account_migration_gate_uuid(run_id: &str, slot: &str, purpose: &str) -> String {
	let mut digest = Sha256::new();
	digest.update(b"decodex-account-migration-gate-v1\0");
	digest.update(run_id.as_bytes());
	digest.update(b"\0");
	digest.update(slot.as_bytes());
	digest.update(b"\0");
	digest.update(purpose.as_bytes());
	let digest = digest.finalize();
	let mut value = [0_u8; 16];
	value.copy_from_slice(&digest[..16]);
	value[6] = (value[6] & 0x0f) | 0x40;
	value[8] = (value[8] & 0x3f) | 0x80;
	format!(
		"{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
		value[0],
		value[1],
		value[2],
		value[3],
		value[4],
		value[5],
		value[6],
		value[7],
		value[8],
		value[9],
		value[10],
		value[11],
		value[12],
		value[13],
		value[14],
		value[15],
	)
}

#[cfg(feature = "account-migration-transition-gate")]
pub(crate) fn account_migration_gate_login_home() -> Result<PathBuf, OfflineAccountMigrationError> {
	// SAFETY: `geteuid` has no arguments and cannot fail.
	real_login_home(unsafe { libc::geteuid() })
}

#[cfg(feature = "account-migration-transition-gate")]
pub(crate) fn account_migration_gate_manifest_bindings(
	path: &Path,
	expected_account_ids: &[AccountId],
) -> Result<Vec<(AccountId, CredentialBinding)>, OfflineAccountMigrationError> {
	let bytes = read_private_file(path, MAX_MANIFEST_BYTES)?;
	let manifest = parse_manifest(&bytes)?;
	if manifest.accounts.len() != expected_account_ids.len() {
		return Err(OfflineAccountMigrationError::InvalidManifest);
	}
	manifest
		.accounts
		.into_iter()
		.zip(expected_account_ids)
		.map(|(account, expected_account_id)| {
			if &account.account_id != expected_account_id {
				return Err(OfflineAccountMigrationError::InvalidManifest);
			}
			let target = account.target.ok_or(OfflineAccountMigrationError::InvalidManifest)?;
			Ok((account.account_id, target))
		})
		.collect()
}

fn verify_credential_directory(
	directory: &Path,
	accounts: &[ParsedAccount],
) -> Result<(), OfflineAccountMigrationError> {
	let effective_uid = verify_path_below_real_login_home(directory)?;
	verify_private_ancestor_chain(directory, effective_uid)?;
	let metadata = fs::symlink_metadata(directory)
		.map_err(|_| OfflineAccountMigrationError::InvalidCredentialSource)?;
	if metadata.file_type().is_symlink()
		|| !metadata.is_dir()
		|| metadata.uid() != effective_uid
		|| metadata.permissions().mode() & 0o7777 != 0o700
	{
		return Err(OfflineAccountMigrationError::InvalidCredentialSource);
	}
	let expected = accounts
		.iter()
		.map(|account| format!("{}.json", account.account_id.as_str()))
		.collect::<BTreeSet<_>>();
	let actual = fs::read_dir(directory)
		.map_err(|_| OfflineAccountMigrationError::InvalidCredentialSource)?
		.map(|entry| {
			entry
				.map_err(|_| OfflineAccountMigrationError::InvalidCredentialSource)?
				.file_name()
				.into_string()
				.map_err(|_| OfflineAccountMigrationError::InvalidCredentialSource)
		})
		.collect::<Result<BTreeSet<_>, _>>()?;
	if actual != expected {
		return Err(OfflineAccountMigrationError::InvalidCredentialSource);
	}
	Ok(())
}

fn verify_migration_source_parents(path: &Path) -> Result<(), OfflineAccountMigrationError> {
	let effective_uid = verify_path_below_real_login_home(path)?;
	let direct_parent = path.parent().ok_or(OfflineAccountMigrationError::InvalidPath)?;
	match fs::symlink_metadata(direct_parent) {
		Ok(metadata)
			if !metadata.file_type().is_symlink()
				&& metadata.is_dir()
				&& metadata.uid() == effective_uid
				&& metadata.permissions().mode() & 0o7777 == 0o700 => {},
		_ => return Err(OfflineAccountMigrationError::InvalidPath),
	}
	verify_private_ancestor_chain(direct_parent, effective_uid)
}

fn verify_private_ancestor_chain(
	private_boundary: &Path,
	effective_uid: u32,
) -> Result<(), OfflineAccountMigrationError> {
	let mut parent = private_boundary.parent().ok_or(OfflineAccountMigrationError::InvalidPath)?;
	loop {
		let metadata =
			fs::symlink_metadata(parent).map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
		let mode = metadata.permissions().mode();
		if metadata.file_type().is_symlink()
			|| !metadata.is_dir()
			|| (metadata.uid() != 0 && metadata.uid() != effective_uid)
			|| mode & 0o022 != 0
		{
			return Err(OfflineAccountMigrationError::InvalidPath);
		}
		let Some(next) = parent.parent() else {
			break;
		};
		parent = next;
	}
	Ok(())
}

fn verify_path_below_real_login_home(path: &Path) -> Result<u32, OfflineAccountMigrationError> {
	if !path.is_absolute()
		|| path
			.components()
			.any(|component| matches!(component, Component::CurDir | Component::ParentDir))
	{
		return Err(OfflineAccountMigrationError::InvalidPath);
	}
	// SAFETY: `geteuid` has no arguments and cannot fail.
	let effective_uid = unsafe { libc::geteuid() };
	let home = real_login_home(effective_uid)?;
	let relative =
		path.strip_prefix(&home).map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
	if relative.as_os_str().is_empty() {
		return Err(OfflineAccountMigrationError::InvalidPath);
	}
	Ok(effective_uid)
}

fn real_login_home(effective_uid: u32) -> Result<PathBuf, OfflineAccountMigrationError> {
	const INITIAL_BUFFER_BYTES: usize = 16 * 1024;
	const MAX_BUFFER_BYTES: usize = 1024 * 1024;

	let mut capacity = INITIAL_BUFFER_BYTES;
	loop {
		// SAFETY: a zeroed `passwd` is valid output storage for `getpwuid_r`.
		let mut entry = unsafe { std::mem::zeroed::<libc::passwd>() };
		let mut result = std::ptr::null_mut();
		let mut buffer = vec![0_u8; capacity];
		// SAFETY: all output pointers refer to live storage for the duration of this call.
		let status = unsafe {
			libc::getpwuid_r(
				effective_uid,
				&mut entry,
				buffer.as_mut_ptr().cast(),
				buffer.len(),
				&mut result,
			)
		};
		if status == libc::ERANGE && capacity < MAX_BUFFER_BYTES {
			capacity = (capacity * 2).min(MAX_BUFFER_BYTES);
			continue;
		}
		if status != 0 || result.is_null() || entry.pw_dir.is_null() {
			return Err(OfflineAccountMigrationError::InvalidPath);
		}
		// SAFETY: successful `getpwuid_r` returned a non-null NUL-terminated `pw_dir`.
		let bytes = unsafe { CStr::from_ptr(entry.pw_dir) }.to_bytes();
		if bytes.is_empty() {
			return Err(OfflineAccountMigrationError::InvalidPath);
		}
		let home = PathBuf::from(OsStr::from_bytes(bytes));
		if !home.is_absolute()
			|| home
				.components()
				.any(|component| matches!(component, Component::CurDir | Component::ParentDir))
		{
			return Err(OfflineAccountMigrationError::InvalidPath);
		}
		return Ok(home);
	}
}

fn verify_retired_runtime_inputs(bytes: &[u8]) -> Result<(), OfflineAccountMigrationError> {
	let lower = bytes.iter().map(u8::to_ascii_lowercase).collect::<Vec<_>>();
	for forbidden in [
		b"--legacy-accounts".as_slice(),
		b"--legacy-mapping".as_slice(),
		b"decodex_reset_card_slot_".as_slice(),
		b":8192".as_slice(),
	] {
		if lower.windows(forbidden.len()).any(|window| window == forbidden) {
			return Err(OfflineAccountMigrationError::RuntimeRetirementUnverified);
		}
	}
	Ok(())
}

fn validate_installer_namespace_lock(
	config: &Path,
	raw_fd: RawFd,
) -> Result<File, OfflineAccountMigrationError> {
	let root = config.parent().ok_or(OfflineAccountMigrationError::InstallerLockUnavailable)?;
	let paths = DecodexRoot::new(root.to_path_buf())
		.map_err(|_| OfflineAccountMigrationError::InstallerLockUnavailable)?
		.paths();
	// SAFETY: `geteuid` has no arguments and cannot fail.
	let effective_uid = unsafe { libc::geteuid() };
	LocalTransportAuthority::new(paths, LocalTrustPolicy::SameUid, Some(effective_uid))
		.and_then(|authority| authority.validate_installer_namespace_lock_fd(raw_fd))
		.map_err(|_| OfflineAccountMigrationError::InstallerLockUnavailable)
}

fn validate_configured_local_authority(
	config: &DecodexConfig,
) -> Result<(), OfflineAccountMigrationError> {
	let effective_uid = unsafe { libc::geteuid() };
	match config.active_profile() {
		ServerProfile::Local(profile)
			if profile.policy() == LocalTrustPolicy::SameUid
				&& profile.service_owner_uid() == Some(effective_uid) =>
			Ok(()),
		_ => Err(OfflineAccountMigrationError::InvalidConfig),
	}
}

fn validate_absolute_paths(
	options: &OfflineAccountMigrationOptions,
) -> Result<(), OfflineAccountMigrationError> {
	if [&options.config, &options.manifest, &options.credential_directory, &options.launch_agent]
		.into_iter()
		.any(|path| !path.is_absolute())
	{
		return Err(OfflineAccountMigrationError::InvalidPath);
	}
	Ok(())
}

fn validate_finalize_paths(
	options: &OfflineAccountMigrationFinalizeOptions,
) -> Result<(), OfflineAccountMigrationError> {
	let fixed = [
		&options.config,
		&options.manifest,
		&options.launch_agent,
		&options.retired_staging_config,
		&options.retired_credential_directory,
	];
	if fixed.iter().copied().any(|path| !path.is_absolute())
		|| options.retired_active_sources.len() > 4
		|| options.installed_assets.is_empty()
		|| options.installed_assets.len() > 16
		|| options
			.retired_active_sources
			.iter()
			.chain(options.installed_assets.iter())
			.any(|path| !path.is_absolute())
	{
		return Err(OfflineAccountMigrationError::InvalidPath);
	}
	let unique = fixed
		.iter()
		.copied()
		.chain(options.retired_active_sources.iter())
		.chain(options.installed_assets.iter())
		.collect::<BTreeSet<_>>();
	if unique.len()
		!= fixed.len() + options.retired_active_sources.len() + options.installed_assets.len()
	{
		return Err(OfflineAccountMigrationError::InvalidPath);
	}
	Ok(())
}

fn validate_verify_paths(
	options: &OfflineAccountMigrationVerifyOptions,
) -> Result<(), OfflineAccountMigrationError> {
	let fixed = [
		&options.config,
		&options.launch_agent,
		&options.retired_staging_config,
		&options.retired_credential_directory,
	];
	if fixed.iter().copied().any(|path| !path.is_absolute())
		|| options.retired_active_sources.len() > 4
		|| options.installed_assets.is_empty()
		|| options.installed_assets.len() > 16
		|| options
			.retired_active_sources
			.iter()
			.chain(options.installed_assets.iter())
			.any(|path| !path.is_absolute())
	{
		return Err(OfflineAccountMigrationError::InvalidPath);
	}
	let unique = fixed
		.iter()
		.copied()
		.chain(options.retired_active_sources.iter())
		.chain(options.installed_assets.iter())
		.collect::<BTreeSet<_>>();
	if unique.len()
		!= fixed.len() + options.retired_active_sources.len() + options.installed_assets.len()
	{
		return Err(OfflineAccountMigrationError::InvalidPath);
	}
	Ok(())
}

fn verify_absent_exact(path: &Path) -> Result<(), OfflineAccountMigrationError> {
	match fs::symlink_metadata(path) {
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
		_ => Err(OfflineAccountMigrationError::RuntimeRetirementUnverified),
	}
}

fn verify_manifest_daemon_wrapper(
	manifest: &ValidatedManifest,
	launch_agent: &[u8],
) -> Result<(), OfflineAccountMigrationError> {
	verify_current_daemon_wrapper(&manifest.daemon_wrapper)
		.and_then(|_| verify_launch_agent_daemon_wrapper(launch_agent, &manifest.daemon_wrapper))
		.map_err(|_| OfflineAccountMigrationError::RuntimeRetirementUnverified)
}

fn verify_daemon_wrapper_installed_asset(
	manifest: &ValidatedManifest,
	assets: &[CompletedInstalledAsset],
) -> Result<(), OfflineAccountMigrationError> {
	let matches = assets
		.iter()
		.filter(|asset| asset.path == manifest.daemon_wrapper.executable_path())
		.collect::<Vec<_>>();
	if matches.len() == 1
		&& matches[0].sha256 == manifest.daemon_wrapper.executable_sha256()
		&& matches[0].byte_count == manifest.daemon_wrapper.executable_byte_count()
	{
		return Ok(());
	}
	Err(OfflineAccountMigrationError::RuntimeRetirementUnverified)
}

fn verify_installed_assets(
	paths: &[PathBuf],
) -> Result<Vec<CompletedInstalledAsset>, OfflineAccountMigrationError> {
	let effective_uid = unsafe { libc::geteuid() };
	let mut names = BTreeSet::new();
	let mut verified = Vec::with_capacity(paths.len());
	for path in paths {
		let metadata = fs::symlink_metadata(path)
			.map_err(|_| OfflineAccountMigrationError::RuntimeRetirementUnverified)?;
		let name = path
			.file_name()
			.and_then(|value| value.to_str())
			.filter(|value| !value.is_empty() && value.len() <= 128)
			.ok_or(OfflineAccountMigrationError::RuntimeRetirementUnverified)?;
		let path_text =
			path.to_str().ok_or(OfflineAccountMigrationError::RuntimeRetirementUnverified)?;
		if !names.insert(name.to_owned())
			|| metadata.file_type().is_symlink()
			|| !metadata.is_file()
			|| (metadata.uid() != effective_uid && metadata.uid() != 0)
			|| metadata.permissions().mode() & 0o022 != 0
			|| metadata.permissions().mode() & 0o111 == 0
			|| metadata.nlink() != 1
			|| metadata.len() == 0
			|| metadata.len() > MAX_INSTALLED_ASSET_BYTES
		{
			return Err(OfflineAccountMigrationError::RuntimeRetirementUnverified);
		}
		let mut options = OpenOptions::new();
		options.read(true).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
		let mut file = options
			.open(path)
			.map_err(|_| OfflineAccountMigrationError::RuntimeRetirementUnverified)?;
		let actual = file
			.metadata()
			.map_err(|_| OfflineAccountMigrationError::RuntimeRetirementUnverified)?;
		if actual.dev() != metadata.dev() || actual.ino() != metadata.ino() {
			return Err(OfflineAccountMigrationError::RuntimeRetirementUnverified);
		}
		let mut digest = Sha256::new();
		let mut bytes_read = 0_u64;
		let mut buffer = [0_u8; 64 * 1024];
		loop {
			let count = file
				.read(&mut buffer)
				.map_err(|_| OfflineAccountMigrationError::RuntimeRetirementUnverified)?;
			if count == 0 {
				break;
			}
			bytes_read = bytes_read
				.checked_add(u64::try_from(count).unwrap_or(u64::MAX))
				.ok_or(OfflineAccountMigrationError::RuntimeRetirementUnverified)?;
			if bytes_read > MAX_INSTALLED_ASSET_BYTES {
				return Err(OfflineAccountMigrationError::RuntimeRetirementUnverified);
			}
			digest.update(&buffer[..count]);
		}
		if bytes_read != metadata.len() {
			return Err(OfflineAccountMigrationError::RuntimeRetirementUnverified);
		}
		let digest = digest.finalize();
		verified.push(CompletedInstalledAsset {
			name: name.to_owned(),
			path: path_text.to_owned(),
			sha256: digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
			byte_count: bytes_read,
		});
	}
	Ok(verified)
}

fn credential(
	identity: &PostgresIdentityConfig,
) -> Result<Option<Zeroizing<String>>, OfflineAccountMigrationError> {
	match identity.credential_env_var() {
		Some(name) => env::var(name)
			.ok()
			.filter(|value| !value.is_empty())
			.map(|value| Some(Zeroizing::new(value)))
			.ok_or(OfflineAccountMigrationError::PostgresUnavailable),
		None => Ok(None),
	}
}

fn read_private_file(
	path: &Path,
	maximum_bytes: u64,
) -> Result<Zeroizing<Vec<u8>>, OfflineAccountMigrationError> {
	let metadata =
		fs::symlink_metadata(path).map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
	let effective_uid = unsafe { libc::geteuid() };
	if metadata.file_type().is_symlink()
		|| !metadata.is_file()
		|| metadata.uid() != effective_uid
		|| metadata.permissions().mode() & 0o7777 != 0o600
		|| metadata.nlink() != 1
		|| metadata.len() == 0
		|| metadata.len() > maximum_bytes
	{
		return Err(OfflineAccountMigrationError::InvalidPath);
	}
	let file = OpenOptions::new()
		.read(true)
		.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
		.open(path)
		.map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
	let actual = file.metadata().map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
	if !actual.is_file()
		|| actual.uid() != effective_uid
		|| actual.permissions().mode() & 0o7777 != 0o600
		|| actual.nlink() != 1
		|| actual.dev() != metadata.dev()
		|| actual.ino() != metadata.ino()
		|| actual.len() != metadata.len()
	{
		return Err(OfflineAccountMigrationError::SourceChanged);
	}
	read_bounded(file, maximum_bytes)
}

fn read_bounded(
	file: File,
	maximum_bytes: u64,
) -> Result<Zeroizing<Vec<u8>>, OfflineAccountMigrationError> {
	let mut bytes = Zeroizing::new(Vec::new());
	let mut reader: Take<File> = file.take(maximum_bytes + 1);
	reader.read_to_end(&mut bytes).map_err(|_| OfflineAccountMigrationError::InvalidPath)?;
	if bytes.is_empty()
		|| u64::try_from(bytes.len()).ok().is_none_or(|length| length > maximum_bytes)
	{
		return Err(OfflineAccountMigrationError::InvalidPath);
	}
	Ok(bytes)
}

fn sha256_file(path: &Path, maximum_bytes: u64) -> Result<String, OfflineAccountMigrationError> {
	read_private_file(path, maximum_bytes).map(|bytes| sha256(&bytes))
}

fn sha256(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn valid_digest(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Closed operator failure. It never contains a path, provider identity, or credential value.
#[derive(Debug)]
pub enum OfflineAccountMigrationError {
	/// The required inherited namespace-lock descriptor shape is unavailable.
	InstallerLockUnavailable,
	/// An input or destination path did not satisfy local authority rules.
	InvalidPath,
	/// Repository configuration is missing or invalid.
	InvalidConfig,
	/// The normalized migration manifest is invalid.
	InvalidManifest,
	/// A source changed after manifest validation.
	SourceChanged,
	/// A credential source is unsafe or malformed.
	InvalidCredentialSource,
	/// PostgreSQL authority could not complete the transfer.
	PostgresUnavailable,
	/// Exact destination verification failed.
	DestinationMismatch,
	/// Legacy normal-runtime authority was not proved retired.
	RuntimeRetirementUnverified,
	/// Process execution authorization could not be proved closed.
	ExecutionAuthorizationUnavailable,
	/// An existing durable migration receipt conflicts with this manifest.
	ReceiptConflict,
}
impl Error for OfflineAccountMigrationError {}
impl Display for OfflineAccountMigrationError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InstallerLockUnavailable =>
				"offline account migration inherited namespace lock is unavailable",
			Self::InvalidPath => "offline account migration path is unsafe",
			Self::InvalidConfig => "offline account migration configuration is invalid",
			Self::InvalidManifest => "offline account migration manifest is invalid",
			Self::SourceChanged => "offline account migration source changed",
			Self::InvalidCredentialSource => "offline account credential source is invalid",
			Self::PostgresUnavailable => "offline account migration PostgreSQL is unavailable",
			Self::DestinationMismatch =>
				"offline account migration destination verification failed",
			Self::RuntimeRetirementUnverified => "normal runtime retirement is not verified",
			Self::ExecutionAuthorizationUnavailable =>
				"ProcessGeneration execution authorization is unavailable",
			Self::ReceiptConflict => "offline account migration receipt conflicts",
		})
	}
}
