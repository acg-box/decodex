//! One-shot, value-suppressing transfer from the retired redb account vault.

#[cfg(test)] use tempfile as _;

#[cfg(unix)] use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::{
	collections::BTreeSet,
	error::Error,
	fmt::{Display, Formatter},
	fs::{File, OpenOptions},
	io::{Read as _, stdin},
	path::{Path, PathBuf},
};

use clap::Parser;
use decodex_core::{
	AccountId, AccountLifecycleReadiness, AccountOperationId, AccountProvider,
	AccountQuotaDisposition, AccountQuotaObservationError, AccountQuotaWindow,
	AccountQuotaWindowObservation, AccountRecord, AccountRoutingControl, AccountSelectionMode,
	AccountState, CredentialBinding, CredentialFingerprint, CredentialStoreSchemaVersion,
	CredentialVersion, DecodexRoot, ProviderIdentity,
};
use decodex_database::{
	CredentialKey, CredentialRecord, LocalAccountTransfer, LocalAccountTransferBatch,
	LocalAccountTransferOutcome, SqliteStore,
};
use redb::{ReadOnlyDatabase, ReadableDatabase as _, ReadableTable as _, TableDefinition};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
const MAX_VAULT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CREDENTIAL_RECORD_BYTES: usize = 1024 * 1024;
const FINGERPRINT_DOMAIN: &[u8] = b"decodex-host-credential-store-v1\0";
const TRANSFER_DIGEST_DOMAIN: &[u8] = b"decodex-local-account-transfer-v1\0";
const CREDENTIALS: TableDefinition<&str, &[u8]> = TableDefinition::new("account_credentials_v1");

#[derive(Parser)]
#[command(about, version)]
struct Cli {
	/// Exact Decodex root whose fixed retired vault and SQLite target are used.
	#[arg(long)]
	root: PathBuf,
}

#[derive(Clone, Copy, Debug)]
enum TransferFailure {
	InvalidManifest,
	UnsafeSource,
	SourceUnavailable,
	SourceMismatch,
	TargetRefused,
	VerificationFailed,
}

impl Display for TransferFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidManifest => "account snapshot is invalid",
			Self::UnsafeSource => "retired credential source is unsafe",
			Self::SourceUnavailable => "retired credential source is unavailable",
			Self::SourceMismatch => "account snapshot and retired credential source differ",
			Self::TargetRefused => "SQLite account transfer was refused",
			Self::VerificationFailed => "SQLite account transfer verification failed",
		})
	}
}

impl Error for TransferFailure {}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CliEnvelope {
	schema: String,
	command: String,
	outcome: String,
	result: AvailableAccounts,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AvailableAccounts {
	outcome: String,
	data: AccountSnapshot,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AccountSnapshot {
	accounts: Vec<AccountInput>,
	routing: RoutingInput,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AccountInput {
	account_id: String,
	alias: String,
	enabled: bool,
	account_revision: u64,
	observed_state: ObservedStateInput,
	lifecycle_readiness: LifecycleReadinessInput,
	credential_binding: Option<CredentialBindingInput>,
	#[serde(default)]
	unsettled_operation: Option<serde_json::Value>,
	five_hour_quota: QuotaInput,
	seven_day_quota: QuotaInput,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ObservedStateInput {
	Unavailable,
	Unknown,
	Available,
	Depleted,
	AuthFailed,
	PluginUnready,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum LifecycleReadinessInput {
	Ready,
	CredentialAbsent,
	StoreUnavailable,
	StoreMismatch,
	ProviderMismatch,
	OperationUnsettled,
	CallbackCapabilityUnready,
	Tombstoned,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CredentialBindingInput {
	schema_version: u16,
	version: u64,
	fingerprint_sha256: String,
	provider: String,
	provider_account_id: String,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct QuotaInput {
	duration_minutes: u32,
	observed_at_unix_micros: Option<i64>,
	result: QuotaStateInput,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(tag = "state", content = "data", rename_all = "snake_case", deny_unknown_fields)]
enum QuotaStateInput {
	Unknown,
	Current { used_percent: u8, resets_at_unix_micros: i64 },
	Error { error: QuotaErrorInput },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum QuotaErrorInput {
	ProviderUnavailable,
	ProtocolUnavailable,
	AccountMismatch,
	UnsupportedWindow,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RoutingInput {
	revision: u64,
	mode: RoutingModeInput,
	order: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum RoutingModeInput {
	Balanced,
	Fixed { account_id: String },
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct PersistedCredentialV1 {
	schema_version: u16,
	account_id: String,
	credential_version: u64,
	writer_operation_id: String,
	provider: String,
	provider_account_id: String,
	access_token: String,
	refresh_token: String,
	id_token: Option<String>,
	plan_type: Option<String>,
	provider_email: String,
	token_type: String,
	access_token_expires_at_unix_micros: i64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
	let cli = Cli::parse();
	let root = DecodexRoot::new(cli.root).map_err(|_| TransferFailure::TargetRefused)?;
	let manifest = read_manifest()?;
	validate_envelope(&manifest)?;
	let paths = root.paths();
	let records = read_vault(&paths.credential_vault_file(), &manifest.result.data.accounts)?;
	let digest = transfer_digest(&manifest.result.data, &records)?;
	let (accounts, routing) = build_batch(manifest.result.data, records)?;
	let expected_ids =
		routing.order.iter().map(|value| value.as_str().to_owned()).collect::<Vec<_>>();
	let expected_routing = routing.clone();
	let expected_keys =
		accounts.iter().map(|value| value.credential.key.clone()).collect::<Vec<_>>();
	let store = SqliteStore::open(&paths).map_err(|_| TransferFailure::TargetRefused)?;
	let outcome = store
		.import_local_accounts(LocalAccountTransferBatch {
			source_sha256: digest,
			accounts,
			routing,
		})
		.map_err(|_| TransferFailure::TargetRefused)?;
	store.revalidate().await.map_err(|_| TransferFailure::VerificationFailed)?;
	verify_target(&store, &expected_ids, &expected_routing, &expected_keys).await?;
	store.close();
	let (outcome_text, account_count) = match outcome {
		LocalAccountTransferOutcome::Imported { account_count } => ("imported", account_count),
		LocalAccountTransferOutcome::Replayed { account_count } => ("replayed", account_count),
	};
	println!(
		"{}",
		serde_json::to_string(&serde_json::json!({
			"schema": "decodex/local-account-transfer/1",
			"outcome": outcome_text,
			"account_count": account_count,
			"source_vault_retained": true,
		}))?
	);
	Ok(())
}

fn read_manifest() -> Result<CliEnvelope, TransferFailure> {
	let mut bytes = Zeroizing::new(Vec::new());
	stdin()
		.take(MAX_MANIFEST_BYTES + 1)
		.read_to_end(&mut bytes)
		.map_err(|_| TransferFailure::InvalidManifest)?;
	if bytes.is_empty()
		|| u64::try_from(bytes.len()).ok().is_none_or(|len| len > MAX_MANIFEST_BYTES)
	{
		return Err(TransferFailure::InvalidManifest);
	}
	serde_json::from_slice(&bytes).map_err(|_| TransferFailure::InvalidManifest)
}

fn validate_envelope(envelope: &CliEnvelope) -> Result<(), TransferFailure> {
	if envelope.schema != "decodex/cli-account/1"
		|| envelope.command != "list"
		|| envelope.outcome != "success"
		|| envelope.result.outcome != "available"
		|| envelope.result.data.accounts.is_empty()
		|| envelope.result.data.accounts.len() > 512
	{
		return Err(TransferFailure::InvalidManifest);
	}
	Ok(())
}

fn read_vault(
	path: &Path,
	accounts: &[AccountInput],
) -> Result<Vec<CredentialRecord>, TransferFailure> {
	let guard = open_source_guard(path)?;
	let before = guard.metadata().map_err(|_| TransferFailure::UnsafeSource)?;
	let database = ReadOnlyDatabase::open(path).map_err(|_| TransferFailure::SourceUnavailable)?;
	let after = path.metadata().map_err(|_| TransferFailure::UnsafeSource)?;
	#[cfg(unix)]
	if before.dev() != after.dev() || before.ino() != after.ino() || after.nlink() != 1 {
		return Err(TransferFailure::UnsafeSource);
	}
	let transaction = database.begin_read().map_err(|_| TransferFailure::SourceUnavailable)?;
	let table =
		transaction.open_table(CREDENTIALS).map_err(|_| TransferFailure::SourceUnavailable)?;
	let source_ids = table
		.iter()
		.map_err(|_| TransferFailure::SourceUnavailable)?
		.map(|entry| {
			entry
				.map(|(key, _)| key.value().to_owned())
				.map_err(|_| TransferFailure::SourceUnavailable)
		})
		.collect::<Result<BTreeSet<_>, _>>()?;
	let expected_ids =
		accounts.iter().map(|value| value.account_id.clone()).collect::<BTreeSet<_>>();
	if source_ids != expected_ids || expected_ids.len() != accounts.len() {
		return Err(TransferFailure::SourceMismatch);
	}
	let mut records = Vec::with_capacity(accounts.len());
	for account in accounts {
		let value = table
			.get(account.account_id.as_str())
			.map_err(|_| TransferFailure::SourceUnavailable)?
			.ok_or(TransferFailure::SourceMismatch)?;
		let payload = Zeroizing::new(value.value().to_vec());
		records.push(validate_credential(account, payload)?);
	}
	drop((table, transaction, database, guard));
	Ok(records)
}

fn open_source_guard(path: &Path) -> Result<File, TransferFailure> {
	let mut options = OpenOptions::new();
	options.read(true);
	#[cfg(unix)]
	options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
	let file = options.open(path).map_err(|_| TransferFailure::SourceUnavailable)?;
	let metadata = file.metadata().map_err(|_| TransferFailure::UnsafeSource)?;
	#[cfg(unix)]
	let unsafe_authority = metadata.uid() != unsafe { libc::geteuid() }
		|| metadata.mode() & 0o077 != 0
		|| metadata.nlink() != 1;
	#[cfg(not(unix))]
	let unsafe_authority = false;
	if !metadata.is_file()
		|| unsafe_authority
		|| metadata.len() == 0
		|| metadata.len() > MAX_VAULT_BYTES
	{
		return Err(TransferFailure::UnsafeSource);
	}
	Ok(file)
}

fn validate_credential(
	account: &AccountInput,
	payload: Zeroizing<Vec<u8>>,
) -> Result<CredentialRecord, TransferFailure> {
	if payload.is_empty() || payload.len() > MAX_CREDENTIAL_RECORD_BYTES {
		return Err(TransferFailure::SourceMismatch);
	}
	let persisted: PersistedCredentialV1 =
		serde_json::from_slice(&payload).map_err(|_| TransferFailure::SourceMismatch)?;
	let declared = account.credential_binding.as_ref().ok_or(TransferFailure::SourceMismatch)?;
	let fingerprint = credential_fingerprint(&payload);
	if persisted.schema_version != 1
		|| persisted.account_id != account.account_id
		|| persisted.credential_version != declared.version
		|| persisted.provider != "chatgpt"
		|| declared.provider != "chatgpt"
		|| persisted.provider_account_id != declared.provider_account_id
		|| persisted.schema_version != declared.schema_version
		|| fingerprint != declared.fingerprint_sha256
		|| persisted.access_token.is_empty()
		|| persisted.refresh_token.is_empty()
		|| persisted.provider_email.is_empty()
		|| persisted.provider_email.len() > 320
		|| persisted.provider_email.chars().any(char::is_control)
		|| !persisted.token_type.eq_ignore_ascii_case("bearer")
		|| persisted.access_token_expires_at_unix_micros <= 0
	{
		return Err(TransferFailure::SourceMismatch);
	}
	AccountOperationId::new(persisted.writer_operation_id.clone())
		.map_err(|_| TransferFailure::SourceMismatch)?;
	Ok(CredentialRecord {
		key: CredentialKey {
			account_id: persisted.account_id.clone(),
			schema_version: persisted.schema_version,
			credential_version: persisted.credential_version,
			fingerprint,
			writer_operation_id: persisted.writer_operation_id.clone(),
			provider: persisted.provider.clone(),
			provider_account_id: persisted.provider_account_id.clone(),
		},
		payload,
	})
}

fn credential_fingerprint(bytes: &[u8]) -> String {
	let mut digest = Sha256::new();
	digest.update(FINGERPRINT_DOMAIN);
	digest.update(bytes);
	hex_digest(digest.finalize().as_slice())
}

fn transfer_digest(
	snapshot: &AccountSnapshot,
	records: &[CredentialRecord],
) -> Result<String, TransferFailure> {
	let canonical = serde_json::to_vec(snapshot).map_err(|_| TransferFailure::InvalidManifest)?;
	let mut digest = Sha256::new();
	digest.update(TRANSFER_DIGEST_DOMAIN);
	digest.update(canonical);
	for record in records {
		for value in [
			record.key.account_id.as_str(),
			record.key.fingerprint.as_str(),
			record.key.writer_operation_id.as_str(),
			record.key.provider.as_str(),
			record.key.provider_account_id.as_str(),
		] {
			digest.update(value.as_bytes());
			digest.update([0]);
		}
	}
	Ok(hex_digest(digest.finalize().as_slice()))
}

fn hex_digest(bytes: &[u8]) -> String {
	bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn build_batch(
	snapshot: AccountSnapshot,
	records: Vec<CredentialRecord>,
) -> Result<(Vec<LocalAccountTransfer>, AccountRoutingControl), TransferFailure> {
	if snapshot.accounts.len() != records.len() {
		return Err(TransferFailure::SourceMismatch);
	}
	let accounts = snapshot
		.accounts
		.into_iter()
		.zip(records)
		.map(|(input, credential)| build_account(input, credential))
		.collect::<Result<Vec<_>, _>>()?;
	let revision = i64::try_from(snapshot.routing.revision)
		.ok()
		.filter(|value| *value > 0)
		.ok_or(TransferFailure::InvalidManifest)?;
	let order = snapshot
		.routing
		.order
		.into_iter()
		.map(|value| AccountId::new(value).map_err(|_| TransferFailure::InvalidManifest))
		.collect::<Result<Vec<_>, _>>()?;
	let mode = match snapshot.routing.mode {
		RoutingModeInput::Balanced => AccountSelectionMode::Balanced,
		RoutingModeInput::Fixed { account_id } => AccountSelectionMode::Fixed(
			AccountId::new(account_id).map_err(|_| TransferFailure::InvalidManifest)?,
		),
	};
	Ok((accounts, AccountRoutingControl { revision, mode, order }))
}

fn build_account(
	input: AccountInput,
	credential: CredentialRecord,
) -> Result<LocalAccountTransfer, TransferFailure> {
	if !matches!(input.lifecycle_readiness, LifecycleReadinessInput::Ready)
		|| input.unsettled_operation.is_some()
	{
		return Err(TransferFailure::SourceMismatch);
	}
	let account_id =
		AccountId::new(input.account_id).map_err(|_| TransferFailure::InvalidManifest)?;
	let operation_id = AccountOperationId::new(credential.key.writer_operation_id.clone())
		.map_err(|_| TransferFailure::SourceMismatch)?;
	let provider =
		ProviderIdentity::new(AccountProvider::Chatgpt, credential.key.provider_account_id.clone())
			.map_err(|_| TransferFailure::SourceMismatch)?;
	let binding = CredentialBinding {
		schema_version: CredentialStoreSchemaVersion::new(credential.key.schema_version)
			.map_err(|_| TransferFailure::SourceMismatch)?,
		version: CredentialVersion::new(credential.key.credential_version)
			.map_err(|_| TransferFailure::SourceMismatch)?,
		fingerprint: CredentialFingerprint::new(credential.key.fingerprint.clone())
			.map_err(|_| TransferFailure::SourceMismatch)?,
		provider,
		writer_operation_id: operation_id,
	};
	let revision = i64::try_from(input.account_revision)
		.ok()
		.filter(|value| *value > 0)
		.ok_or(TransferFailure::InvalidManifest)?;
	Ok(LocalAccountTransfer {
		account: AccountRecord {
			account_id,
			label: input.alias,
			enabled: input.enabled,
			revision,
			observed_state: observed_state(input.observed_state),
			lifecycle_readiness: AccountLifecycleReadiness::Ready,
			credential: Some(binding),
			unsettled_operation: None,
			five_hour_quota: quota(input.five_hour_quota)?,
			seven_day_quota: quota(input.seven_day_quota)?,
			tombstoned: false,
		},
		credential,
	})
}

const fn observed_state(value: ObservedStateInput) -> AccountState {
	match value {
		ObservedStateInput::Unavailable => AccountState::Unavailable,
		ObservedStateInput::Unknown => AccountState::Unknown,
		ObservedStateInput::Available => AccountState::Available,
		ObservedStateInput::Depleted => AccountState::Depleted,
		ObservedStateInput::AuthFailed => AccountState::AuthFailed,
		ObservedStateInput::PluginUnready => AccountState::PluginUnready,
	}
}

fn quota(input: QuotaInput) -> Result<AccountQuotaWindowObservation, TransferFailure> {
	if !matches!(
		input.duration_minutes,
		AccountQuotaWindow::FIVE_HOURS_MINUTES | AccountQuotaWindow::SEVEN_DAYS_MINUTES
	) {
		return Err(TransferFailure::InvalidManifest);
	}
	let disposition = match input.result {
		QuotaStateInput::Unknown => {
			if input.observed_at_unix_micros.is_some() {
				return Err(TransferFailure::InvalidManifest);
			}
			AccountQuotaDisposition::Unknown
		},
		QuotaStateInput::Current { used_percent, resets_at_unix_micros } => {
			let observed_at = input
				.observed_at_unix_micros
				.filter(|value| *value > 0)
				.ok_or(TransferFailure::InvalidManifest)?;
			if resets_at_unix_micros <= observed_at {
				return Err(TransferFailure::InvalidManifest);
			}
			AccountQuotaDisposition::Current(
				AccountQuotaWindow::new(
					input.duration_minutes,
					used_percent,
					resets_at_unix_micros,
				)
				.map_err(|_| TransferFailure::InvalidManifest)?,
			)
		},
		QuotaStateInput::Error { error } => {
			if input.observed_at_unix_micros.is_none_or(|value| value <= 0) {
				return Err(TransferFailure::InvalidManifest);
			}
			AccountQuotaDisposition::Error(match error {
				QuotaErrorInput::ProviderUnavailable =>
					AccountQuotaObservationError::ProviderUnavailable,
				QuotaErrorInput::ProtocolUnavailable =>
					AccountQuotaObservationError::ProtocolUnavailable,
				QuotaErrorInput::AccountMismatch => AccountQuotaObservationError::AccountMismatch,
				QuotaErrorInput::UnsupportedWindow =>
					AccountQuotaObservationError::UnsupportedWindow,
			})
		},
	};
	Ok(AccountQuotaWindowObservation {
		duration_minutes: input.duration_minutes,
		observed_at_unix_micros: input.observed_at_unix_micros,
		disposition,
	})
}

async fn verify_target(
	store: &SqliteStore,
	expected_ids: &[String],
	expected_routing: &AccountRoutingControl,
	expected_keys: &[CredentialKey],
) -> Result<(), TransferFailure> {
	let (accounts, routing) = store
		.read_account_registry_snapshot(512)
		.await
		.map_err(|_| TransferFailure::VerificationFailed)?;
	let actual_ids = accounts.iter().map(|value| value.account_id.as_str()).collect::<Vec<_>>();
	if actual_ids != expected_ids.iter().map(String::as_str).collect::<Vec<_>>()
		|| routing != *expected_routing
		|| expected_keys.len() != accounts.len()
	{
		return Err(TransferFailure::VerificationFailed);
	}
	for key in expected_keys {
		let actual = store
			.read_credential(&key.account_id)
			.map_err(|_| TransferFailure::VerificationFailed)?;
		if actual.key != *key
			|| credential_fingerprint(actual.payload.as_slice()) != key.fingerprint
		{
			return Err(TransferFailure::VerificationFailed);
		}
	}
	Ok(())
}
