//! Sole daemon coordinator for durable account state and credential effects.

use std::{
	collections::HashMap,
	error::Error,
	fmt::{Debug, Display, Formatter},
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, Ordering},
	},
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use decodex_core::{
	AccountId, AccountLifecycleReadiness, AccountOperation, AccountOperationId,
	AccountOperationKind, AccountOperationPhase, AccountProvider, AccountRecord,
	AccountSelectionMode, AccountSelectionRecovery, CredentialBinding, CredentialVersion,
	ProcessGenerationAccountBinding, ProcessGenerationId, ProcessGenerationState, ProviderIdentity,
};
use decodex_database::{
	AccountAdministrationOutcome, AccountCommandReceiptLease, AccountLifecycleMutationOutcome,
	AccountOperationPreparation, AccountStoreObservation, CodexAccountCapabilityAttestation,
	RoutingControlOutcome, SqliteStore, StoreError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{
	account_import::{
		CredentialImportError, ImportedCredential, decode_chatgpt_identity,
		read_explicit_credential_file, read_explicit_shared_codex_credential_file,
		read_shared_codex_credential,
	},
	auth_projection::{
		CodexAuthProjectionError, SharedCodexAuthIdentity, project_shared_codex_auth,
		read_shared_codex_auth_identity, shared_codex_auth_matches,
	},
	host_credentials::{
		CredentialSecretBundle, CredentialStoreError, HostCredentialStore, StoredCredential,
	},
};

const REFRESH_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const CHATGPT_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const MAX_ACCOUNT_READ: u16 = 512;
const PROVIDER_REFRESH_OUTCOME_UNKNOWN: &str = "provider_refresh_outcome_unknown";
const ACCOUNT_ALIAS_DOMAIN: &[u8] = b"decodex/account-alias/v2\0";
const CODEX_AUTH_PROJECTION_DOMAIN: &[u8] = b"decodex/codex-auth-projection/v1\0";
const ACCOUNT_ALIAS_WORDS: [&str; 44] = [
	"Alex", "Avery", "Bailey", "Blake", "Casey", "Charlie", "Clara", "Dana", "Drew", "Eden",
	"Elliot", "Emery", "Evan", "Finley", "Harper", "Hayden", "Iris", "Jamie", "Jordan", "Kai",
	"Kendall", "Lane", "Liam", "Logan", "Mason", "Maya", "Mia", "Morgan", "Noah", "Nora", "Owen",
	"Paige", "Parker", "Quinn", "Reese", "Remy", "Riley", "Rowan", "Sage", "Sasha", "Sidney",
	"Taylor", "Theo", "Val",
];

/// Derive the stable public account alias from the canonical credential-negative provider binding.
pub(crate) fn stable_account_alias(provider: &ProviderIdentity) -> String {
	let provider_kind = match provider.provider() {
		AccountProvider::Chatgpt => b"chatgpt".as_slice(),
	};
	let digest = Sha256::new()
		.chain_update(ACCOUNT_ALIAS_DOMAIN)
		.chain_update(provider_kind)
		.chain_update(b"\0")
		.chain_update(provider.account_id().as_bytes())
		.finalize();
	let selector = u64::from_be_bytes([
		digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
	]);
	let word_count =
		u64::try_from(ACCOUNT_ALIAS_WORDS.len()).expect("account alias word count fits u64");
	let index =
		usize::try_from(selector % word_count).expect("account alias word index fits usize");
	ACCOUNT_ALIAS_WORDS[index].to_owned()
}

fn codex_auth_projection_digest(account: &AccountRecord, binding: &CredentialBinding) -> String {
	let digest = Sha256::new()
		.chain_update(CODEX_AUTH_PROJECTION_DOMAIN)
		.chain_update(account.account_id.as_str().as_bytes())
		.chain_update(b"\0")
		.chain_update((account.revision as u64).to_be_bytes())
		.chain_update(binding.version.get().to_be_bytes())
		.chain_update(binding.fingerprint.as_str().as_bytes())
		.finalize();
	let mut encoded = String::with_capacity(64);
	for byte in digest {
		use std::fmt::Write as _;
		write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
	}
	encoded
}

pub(crate) struct CredentialRefreshResult {
	returned_provider: ProviderIdentity,
	bundle: CredentialSecretBundle,
}

/// Credential-negative readback of the normal shared Codex auth projection.
pub(crate) enum CodexAuthProjectionInspection {
	Current { account_id: AccountId, account_revision: i64, projection_digest: String },
	Unmanaged,
	Unavailable,
}

/// Provider refresh boundary. Implementations receive secrets only in short-lived memory.
pub(crate) trait CredentialRefreshPort: Send + Sync {
	/// Exchange one exact refresh token for a replacement identity and complete bundle.
	fn refresh(
		&self,
		current: &CredentialSecretBundle,
	) -> Result<CredentialRefreshResult, CredentialRefreshError>;
}

/// Exact OpenAI OAuth refresh adapter used by the Mac daemon.
pub(crate) struct OpenAiCredentialRefresher {
	client: reqwest::blocking::Client,
}
impl OpenAiCredentialRefresher {
	/// Construct a bounded client without ambient credential configuration.
	pub(crate) fn new() -> Result<Self, CredentialRefreshError> {
		let client = reqwest::blocking::Client::builder()
			.timeout(Duration::from_secs(10))
			.user_agent("decodexd")
			.build()
			.map_err(|_| CredentialRefreshError::Unavailable)?;
		Ok(Self { client })
	}
}
impl CredentialRefreshPort for OpenAiCredentialRefresher {
	fn refresh(
		&self,
		current: &CredentialSecretBundle,
	) -> Result<CredentialRefreshResult, CredentialRefreshError> {
		let request = RefreshRequest {
			client_id: CHATGPT_OAUTH_CLIENT_ID,
			grant_type: "refresh_token",
			refresh_token: current.refresh_token(),
		};
		let response = self
			.client
			.post(REFRESH_ENDPOINT)
			.json(&request)
			.send()
			.map_err(|_| CredentialRefreshError::Ambiguous)?;
		let status = response.status();
		if !status.is_success() {
			return if status.is_client_error() {
				Err(CredentialRefreshError::Rejected)
			} else {
				Err(CredentialRefreshError::Ambiguous)
			};
		}
		let refreshed: RefreshResponse =
			response.json().map_err(|_| CredentialRefreshError::Ambiguous)?;
		let observed_at = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map_err(|_| CredentialRefreshError::Ambiguous)?;
		let observed_at_micros = i64::try_from(observed_at.as_micros())
			.map_err(|_| CredentialRefreshError::Ambiguous)?;

		credential_refresh_result(current, refreshed, observed_at_micros)
	}
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct RefreshRequest<'a> {
	client_id: &'static str,
	grant_type: &'static str,
	refresh_token: &'a str,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct RefreshResponse {
	id_token: Option<String>,
	access_token: Option<String>,
	refresh_token: Option<String>,
	token_type: Option<String>,
	expires_in: Option<u64>,
}

fn credential_refresh_result(
	current: &CredentialSecretBundle,
	mut refreshed: RefreshResponse,
	observed_at_micros: i64,
) -> Result<CredentialRefreshResult, CredentialRefreshError> {
	let access_token = refreshed
		.access_token
		.take()
		.filter(|value| !value.is_empty())
		.ok_or(CredentialRefreshError::Ambiguous)?;
	let refresh_token =
		refreshed.refresh_token.take().unwrap_or_else(|| current.refresh_token().to_owned());
	let id_token = refreshed
		.id_token
		.take()
		.filter(|value| !value.is_empty())
		.ok_or(CredentialRefreshError::Ambiguous)?;
	let identity =
		decode_chatgpt_identity(&id_token).map_err(|_| CredentialRefreshError::Ambiguous)?;
	let token_type = refreshed.token_type.take().ok_or(CredentialRefreshError::Ambiguous)?;
	let expires_in = refreshed.expires_in.ok_or(CredentialRefreshError::Ambiguous)?;
	let lifetime_micros = i64::try_from(expires_in)
		.ok()
		.and_then(|seconds| seconds.checked_mul(1_000_000))
		.ok_or(CredentialRefreshError::Ambiguous)?;
	let expires_at_micros =
		observed_at_micros.checked_add(lifetime_micros).ok_or(CredentialRefreshError::Ambiguous)?;
	let bundle = CredentialSecretBundle::chatgpt(
		access_token,
		refresh_token,
		Some(id_token),
		identity.plan_type,
		identity.provider_email,
		token_type,
		expires_at_micros,
	)
	.map_err(|_| CredentialRefreshError::Ambiguous)?;

	Ok(CredentialRefreshResult { returned_provider: identity.provider, bundle })
}

/// Provider refresh result classification without response bodies or tokens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialRefreshError {
	/// The provider adapter could not complete before an effect.
	Unavailable,
	/// The provider rejected the refresh request.
	Rejected,
	/// The provider effect outcome cannot be proved.
	Ambiguous,
}
impl Error for CredentialRefreshError {}
impl Display for CredentialRefreshError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::Unavailable => "credential refresh adapter unavailable",
			Self::Rejected => "credential refresh rejected",
			Self::Ambiguous => "credential refresh outcome ambiguous",
		})
	}
}

/// Account projection paired with its current exact host-store readiness.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountInspection {
	/// Current credential-negative account projection.
	pub account: AccountRecord,
	/// Current exact host-store readiness.
	pub readiness: AccountLifecycleReadiness,
}

/// Exact selected account for one initial process generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountSelectionResult {
	/// Exact selected account projection.
	pub account: AccountRecord,
	/// Exact attested refresh-callback profile digest.
	pub callback_profile_sha256: String,
}

/// One short-lived secret projection paired with the immutable non-secret process binding.
pub struct AccountProcessCredential {
	/// Short-lived exact host-store read.
	pub stored: StoredCredential,
	/// Immutable credential-negative process binding.
	pub binding: ProcessGenerationAccountBinding,
	pub(crate) launch_guard: OwnedMutexGuard<()>,
}
impl Debug for AccountProcessCredential {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("AccountProcessCredential")
			.field("stored", &"[REDACTED]")
			.field("binding", &self.binding)
			.finish()
	}
}

/// Short-lived credential projection for the direct provider backend API.
///
/// This projection intentionally has no Codex executable, callback, or process-generation
/// requirement.  The account lock remains held until the provider request owner drops it, so one
/// account cannot refresh or rotate credentials concurrently with an API observation.
pub(crate) struct AccountApiCredential {
	/// Short-lived exact host-store read.
	pub(crate) stored: StoredCredential,
	/// Exact credential-negative provider binding.
	pub(crate) binding: CredentialBinding,
	/// Registry revision read together with the binding.
	pub(crate) account_revision: i64,
	/// Per-account lifecycle lock retained across the bounded provider request.
	pub(crate) _launch_guard: OwnedMutexGuard<()>,
}
impl Debug for AccountApiCredential {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("AccountApiCredential")
			.field("stored", &"[REDACTED]")
			.field("binding", &self.binding)
			.field("account_revision", &self.account_revision)
			.finish()
	}
}

/// Typed explicit recovery when initial selection cannot proceed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountSelectionFailure {
	/// Account that requires recovery, when one can be selected.
	pub account_id: Option<AccountId>,
	/// Stable explicit recovery action.
	pub recovery: AccountSelectionRecovery,
}

/// Secret response used only to answer one Codex refresh callback or login projection.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct ChatgptTokenProjection {
	access_token: String,
	#[zeroize(skip)]
	provider_account_id: String,
	#[zeroize(skip)]
	plan_type: Option<String>,
	#[zeroize(skip)]
	binding: CredentialBinding,
}
impl ChatgptTokenProjection {
	/// Borrow the short-lived access token.
	pub fn access_token(&self) -> &str {
		&self.access_token
	}

	/// Borrow the non-secret provider account identity.
	pub fn provider_account_id(&self) -> &str {
		&self.provider_account_id
	}

	/// Borrow the optional non-secret plan hint.
	pub fn plan_type(&self) -> Option<&str> {
		self.plan_type.as_deref()
	}

	/// Borrow the exact credential-negative binding.
	pub fn binding(&self) -> &CredentialBinding {
		&self.binding
	}
}
impl Debug for ChatgptTokenProjection {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ChatgptTokenProjection")
			.field("provider_account_id", &self.provider_account_id)
			.field("plan_type", &self.plan_type)
			.field("binding", &self.binding)
			.field("access_token", &"[REDACTED]")
			.finish()
	}
}

/// Startup reconciliation summary. Manual items remain admission-blocking.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StartupAccountReconciliation {
	/// Operations proved applied and committed.
	pub committed: u32,
	/// Operations proved effect-free and cancelled.
	pub cancelled: u32,
	/// Account and operation pairs that require explicit recovery.
	pub manual_recovery: Vec<(AccountId, AccountOperationId)>,
}

/// Explicit action accepted for one unsettled credential operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountManualRecoveryAction {
	/// Re-read exact account and credential state and settle only proved effects.
	ReconcileExactStoreState,
	/// Cancel only when the external effect is proved absent.
	CancelBeforeEffect,
}

/// Closed disposition of one explicit manual recovery request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountManualRecoveryOutcome {
	/// The exact external effect was proved and committed.
	Committed,
	/// The operation was proved effect-free and cancelled.
	Cancelled,
	/// Exact state still requires operator recovery.
	StillRequiresRecovery,
}

struct ReauthenticationCommandInput<'a> {
	operation_id: AccountOperationId,
	account_id: &'a AccountId,
	expected_account_revision: i64,
	source_descriptor: &'a str,
	account: &'a AccountRecord,
}

/// Sole account lifecycle coordinator in `decodexd`.
pub struct AccountService {
	store: SqliteStore,
	credentials: Arc<dyn HostCredentialStore>,
	refresher: Arc<dyn CredentialRefreshPort>,
	account_locks: Mutex<HashMap<AccountId, Arc<AsyncMutex<()>>>>,
	callback_ready: AtomicBool,
	callback_profile_sha256: Mutex<Option<String>>,
}
impl AccountService {
	/// Assemble one coordinator from its three narrow infrastructure ports.
	pub(crate) fn new(
		store: SqliteStore,
		credentials: Arc<dyn HostCredentialStore>,
		refresher: Arc<dyn CredentialRefreshPort>,
	) -> Self {
		Self {
			store,
			credentials,
			refresher,
			account_locks: Mutex::new(HashMap::new()),
			callback_ready: AtomicBool::new(false),
			callback_profile_sha256: Mutex::new(None),
		}
	}

	fn set_callback_capability(&self, profile_sha256: String, ready: bool) {
		if let Ok(mut profile) = self.callback_profile_sha256.lock() {
			*profile = ready.then_some(profile_sha256);
			self.callback_ready.store(ready, Ordering::Release);
		} else {
			self.callback_ready.store(false, Ordering::Release);
		}
	}

	/// Persist and activate callback evidence generated from the user's current Codex executable.
	/// Incompatible runtime capabilities stay closed.
	pub async fn attest_callback_capability(
		&self,
		attestation: CodexAccountCapabilityAttestation,
	) -> Result<bool, AccountLifecycleError> {
		let profile = attestation.callback_profile_sha256.clone();
		let ready = self.store.attest_codex_account_capability(&attestation).await?;
		self.set_callback_capability(profile, ready);
		Ok(ready)
	}

	/// Permit only the bootstrap-owned live proof to project the exact attested profile while the
	/// durable capability remains closed. No transport or background worker is live at this point.
	/// List account registry state with current exact host-store readiness.
	pub async fn list(&self) -> Result<Vec<AccountInspection>, AccountLifecycleError> {
		let accounts = self.store.read_account_registry(None, MAX_ACCOUNT_READ).await?;
		Ok(accounts
			.into_iter()
			.map(|account| AccountInspection { readiness: account.lifecycle_readiness, account })
			.collect())
	}

	/// Inspect one account without exposing credential material.
	pub async fn inspect(
		&self,
		account_id: &AccountId,
	) -> Result<AccountInspection, AccountLifecycleError> {
		let account = self.load_account(account_id).await?;
		let readiness = account.lifecycle_readiness;
		Ok(AccountInspection { account, readiness })
	}

	/// Read only the credential-negative deterministic routing controls.
	pub async fn routing_control(
		&self,
	) -> Result<decodex_core::AccountRoutingControl, AccountLifecycleError> {
		Ok(self.store.read_account_routing_control().await?)
	}

	/// Match the safe shared Codex auth identity against current credential-negative bindings.
	pub(crate) async fn codex_auth_projection(&self) -> CodexAuthProjectionInspection {
		let provider_account_id = match read_shared_codex_auth_identity() {
			Ok(SharedCodexAuthIdentity::Chatgpt { provider_account_id }) => provider_account_id,
			Ok(SharedCodexAuthIdentity::Unmanaged) => {
				return CodexAuthProjectionInspection::Unmanaged;
			},
			Err(_) => return CodexAuthProjectionInspection::Unavailable,
		};
		let Ok(accounts) = self.store.read_account_registry(None, MAX_ACCOUNT_READ).await else {
			return CodexAuthProjectionInspection::Unavailable;
		};
		let mut matching = accounts.into_iter().filter(|account| {
			!account.tombstoned
				&& account.credential.as_ref().is_some_and(|binding| {
					binding.provider.provider() == AccountProvider::Chatgpt
						&& binding.provider.account_id() == provider_account_id
				})
		});
		let Some(candidate) = matching.next() else {
			return CodexAuthProjectionInspection::Unmanaged;
		};
		if matching.next().is_some() {
			return CodexAuthProjectionInspection::Unavailable;
		}
		let account_id = candidate.account_id;
		let Ok(lock) = self.lock_for(&account_id) else {
			return CodexAuthProjectionInspection::Unavailable;
		};
		let _guard = lock.lock().await;
		let Ok(account) = self.load_account(&account_id).await else {
			return CodexAuthProjectionInspection::Unavailable;
		};
		if account.tombstoned || account.revision <= 0 {
			return CodexAuthProjectionInspection::Unavailable;
		}
		let Some(binding) = account.credential.as_ref() else {
			return CodexAuthProjectionInspection::Unmanaged;
		};
		if binding.provider.provider() != AccountProvider::Chatgpt
			|| binding.provider.account_id() != provider_account_id
		{
			return CodexAuthProjectionInspection::Unmanaged;
		}
		let Ok(stored) = self.credentials.read_exact(&account.account_id, binding) else {
			return CodexAuthProjectionInspection::Unavailable;
		};
		let Some(id_token) = stored.bundle().id_token() else {
			return CodexAuthProjectionInspection::Unmanaged;
		};
		let Ok(identity) = decode_chatgpt_identity(id_token) else {
			return CodexAuthProjectionInspection::Unmanaged;
		};
		if identity.provider != binding.provider {
			return CodexAuthProjectionInspection::Unmanaged;
		}
		match shared_codex_auth_matches(stored.bundle(), binding.provider.account_id()) {
			Ok(true) => {},
			Ok(false) => return CodexAuthProjectionInspection::Unmanaged,
			Err(_) => return CodexAuthProjectionInspection::Unavailable,
		};
		CodexAuthProjectionInspection::Current {
			account_id: account.account_id.clone(),
			account_revision: account.revision,
			projection_digest: codex_auth_projection_digest(&account, binding),
		}
	}

	/// Project and durably complete one exact logical command while the account writer is held.
	pub(crate) async fn use_account_in_codex_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		account_id: &AccountId,
		expected_revision: i64,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<(i64, String), AccountLifecycleError>) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let lock = self.lock_for(account_id)?;
		let _guard = lock.lock().await;
		let result = use_in_codex_receipt_result(
			self.use_account_in_codex_locked(account_id, expected_revision).await,
		)?;
		let response = build_response(result)?;
		self.store.complete_account_command(lease, &response).await?;
		Ok(response)
	}

	async fn use_account_in_codex_locked(
		&self,
		account_id: &AccountId,
		expected_revision: i64,
	) -> Result<(i64, String), UseInCodexProjectionError> {
		let account =
			self.load_account(account_id).await.map_err(UseInCodexProjectionError::Rejected)?;
		let binding = projection_binding(&account, expected_revision)
			.map_err(UseInCodexProjectionError::Rejected)?
			.clone();
		let stored = self
			.credentials
			.read_exact(account_id, &binding)
			.map_err(AccountLifecycleError::from)
			.map_err(UseInCodexProjectionError::Rejected)?;
		let id_token = stored
			.bundle()
			.id_token()
			.ok_or(AccountLifecycleError::CredentialAbsent)
			.map_err(UseInCodexProjectionError::Rejected)?;
		let identity = decode_chatgpt_identity(id_token)
			.map_err(AccountLifecycleError::from)
			.map_err(UseInCodexProjectionError::Rejected)?;
		if identity.provider != binding.provider {
			return Err(UseInCodexProjectionError::Rejected(
				AccountLifecycleError::ProviderMismatch,
			));
		}
		let latest =
			self.load_account(account_id).await.map_err(UseInCodexProjectionError::Rejected)?;
		if latest.revision != account.revision
			|| latest.enabled != account.enabled
			|| latest.lifecycle_readiness != account.lifecycle_readiness
			|| latest.tombstoned != account.tombstoned
			|| latest.credential.as_ref() != Some(&binding)
		{
			return Err(UseInCodexProjectionError::Rejected(AccountLifecycleError::StaleAccount));
		}
		match project_shared_codex_auth(stored.bundle(), binding.provider.account_id()) {
			Ok(()) => {},
			Err(CodexAuthProjectionError::OutcomeUnknown) => {
				return Err(UseInCodexProjectionError::OutcomeUnknown);
			},
			Err(error) => return Err(UseInCodexProjectionError::Rejected(projection_error(error))),
		}
		Ok((account.revision, codex_auth_projection_digest(&account, &binding)))
	}

	/// Enroll through the logical-command journal and commit the terminal registry projection with
	/// its exact public result in one product-store transaction.
	pub(crate) async fn enroll_from_shared_codex_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: AccountOperationId,
		account_id: AccountId,
		enabled: bool,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<&AccountRecord, AccountLifecycleError>) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let imported = match read_shared_codex_credential() {
			Ok(imported) => imported,
			Err(error) => {
				return self
					.complete_account_command_error(lease, error.into(), build_response)
					.await;
			},
		};
		let alias = stable_account_alias(&imported.provider);
		self.install_credentials_command(
			lease,
			operation_id,
			account_id,
			AccountOperationKind::Enroll,
			alias,
			enabled,
			imported.provider,
			imported.bundle,
			build_response,
		)
		.await
	}

	/// Import through the logical-command journal and atomically retain the final registry result.
	#[allow(clippy::too_many_arguments)] // The journal, operation, account, input, and response owner are independent authority inputs.
	pub(crate) async fn import_credential_file_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: AccountOperationId,
		account_id: AccountId,
		enabled: bool,
		source_descriptor: &str,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<&AccountRecord, AccountLifecycleError>) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let imported = match read_explicit_credential_file(source_descriptor) {
			Ok(imported) => imported,
			Err(error) => {
				return self
					.complete_account_command_error(lease, error.into(), build_response)
					.await;
			},
		};
		let alias = stable_account_alias(&imported.provider);
		self.install_credentials_command(
			lease,
			operation_id,
			account_id,
			AccountOperationKind::Import,
			alias,
			enabled,
			imported.provider,
			imported.bundle,
			build_response,
		)
		.await
	}

	#[allow(clippy::too_many_arguments)]
	async fn install_credentials_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: AccountOperationId,
		account_id: AccountId,
		kind: AccountOperationKind,
		alias: String,
		enabled: bool,
		provider: ProviderIdentity,
		bundle: CredentialSecretBundle,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<&AccountRecord, AccountLifecycleError>) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let lock = self.lock_for(&account_id)?;
		let _guard = lock.lock().await;
		let target = bundle.binding_for(
			&account_id,
			&operation_id,
			CredentialVersion::new(1).map_err(|_| AccountLifecycleError::InvalidOperation)?,
			&provider,
		)?;
		let preparation = AccountOperationPreparation {
			operation_id: operation_id.clone(),
			account_id,
			kind,
			display_label: Some(alias),
			enabled: Some(enabled),
			expected_account_revision: None,
			expected: None,
			target: Some(target.clone()),
			provider,
		};
		let phase = match self.store.prepare_account_operation(&preparation).await? {
			AccountLifecycleMutationOutcome::Applied(mutation)
			| AccountLifecycleMutationOutcome::Replayed(mutation) => mutation.phase,
			AccountLifecycleMutationOutcome::Rejected { rejection, .. } => {
				return self
					.complete_account_command_error(
						lease,
						AccountLifecycleError::OperationRejected(rejection),
						build_response,
					)
					.await;
			},
		};
		if phase == AccountOperationPhase::Prepared {
			if let Err(error) = self.credentials.create(&preparation.account_id, &target, bundle)
				&& (error != CredentialStoreError::AlreadyExists
					|| self.credentials.read_exact(&preparation.account_id, &target).is_err())
			{
				let ambiguous = store_effect_may_be_ambiguous(error);
				let terminal = if ambiguous {
					AccountOperationPhase::RecoveryRequired
				} else {
					AccountOperationPhase::Cancelled
				};
				return self
					.complete_account_operation_error(
						lease,
						&operation_id,
						AccountOperationPhase::Prepared,
						terminal,
						ambiguous.then_some("credential_create_failed"),
						error.into(),
						build_response,
					)
					.await;
			}
			accepted_phase(
				self.store
					.advance_account_operation(
						&operation_id,
						AccountOperationPhase::Prepared,
						AccountOperationPhase::StoreApplied,
						None,
					)
					.await?,
			)?;
		}
		match phase {
			AccountOperationPhase::Prepared
			| AccountOperationPhase::StoreApplied
			| AccountOperationPhase::Committed =>
				self.complete_account_operation_success(
					lease,
					&operation_id,
					AccountOperationPhase::StoreApplied,
					AccountOperationPhase::Committed,
					build_response,
				)
				.await,
			AccountOperationPhase::Cancelled =>
				self.complete_account_operation_error(
					lease,
					&operation_id,
					AccountOperationPhase::Cancelled,
					AccountOperationPhase::Cancelled,
					None,
					AccountLifecycleError::InvalidOperation,
					build_response,
				)
				.await,
			AccountOperationPhase::RecoveryRequired
			| AccountOperationPhase::ProviderEffectPending =>
				self.complete_account_operation_error(
					lease,
					&operation_id,
					phase,
					phase,
					None,
					AccountLifecycleError::NotReady(AccountLifecycleReadiness::OperationUnsettled),
					build_response,
				)
				.await,
		}
	}

	/// Refresh one exact account through either proactive or exact generation-bound authority.
	pub async fn refresh(
		&self,
		operation_id: AccountOperationId,
		account_id: &AccountId,
		expected_account_revision: Option<i64>,
		callback_generation: Option<(&ProcessGenerationId, &ProcessGenerationAccountBinding)>,
		previous_provider_account_id: Option<&str>,
	) -> Result<ChatgptTokenProjection, AccountLifecycleError> {
		let lock = self.lock_for(account_id)?;
		let _guard = lock.lock().await;
		self.refresh_while_locked(
			operation_id,
			account_id,
			expected_account_revision,
			callback_generation,
			previous_provider_account_id,
			false,
		)
		.await
	}

	#[allow(clippy::too_many_lines)] // Keep the generation-bound refresh state machine auditable as one sequence.
	async fn refresh_while_locked(
		&self,
		operation_id: AccountOperationId,
		account_id: &AccountId,
		expected_account_revision: Option<i64>,
		callback_generation: Option<(&ProcessGenerationId, &ProcessGenerationAccountBinding)>,
		previous_provider_account_id: Option<&str>,
		allow_disabled: bool,
	) -> Result<ChatgptTokenProjection, AccountLifecycleError> {
		if let Some((generation_id, process_binding)) = callback_generation {
			self.require_active_callback_generation(account_id, generation_id, process_binding)
				.await?;
		}
		let account = self.load_account(account_id).await?;
		let current = account.credential.as_ref().ok_or(AccountLifecycleError::CredentialAbsent)?;
		if current.writer_operation_id == operation_id {
			return self.project_refresh_result(account_id, current, callback_generation).await;
		}
		if previous_provider_account_id.is_some_and(|value| value != current.provider.account_id())
		{
			return Err(AccountLifecycleError::ProviderMismatch);
		}
		if let Some((_, process_binding)) = callback_generation
			&& &process_binding.credential != current
		{
			return Err(AccountLifecycleError::StaleAccount);
		}
		if let Some(operation) = self.store.read_account_operation(&operation_id).await? {
			self.require_operation_identity(&operation, account_id, AccountOperationKind::Refresh)?;
			return match self.reconcile_operation(&operation).await? {
				ReconciliationDisposition::Committed => {
					let latest = self.load_account(account_id).await?;
					self.project_refresh_result(
						account_id,
						latest
							.credential
							.as_ref()
							.ok_or(AccountLifecycleError::CredentialAbsent)?,
						callback_generation,
					)
					.await
				},
				ReconciliationDisposition::Cancelled =>
					Err(AccountLifecycleError::InvalidOperation),
				ReconciliationDisposition::Manual => Err(AccountLifecycleError::NotReady(
					AccountLifecycleReadiness::OperationUnsettled,
				)),
			};
		}
		if expected_account_revision.is_some_and(|expected| expected != account.revision) {
			return Err(AccountLifecycleError::StaleAccount);
		}
		let stored = if callback_generation.is_some() {
			self.read_exact_for_bound_callback(&account).await?
		} else if allow_disabled {
			self.read_exact_for_existing_work(&account).await?
		} else {
			self.read_exact_for_admission(&account).await?
		};
		let preparation = AccountOperationPreparation {
			operation_id: operation_id.clone(),
			account_id: account_id.clone(),
			kind: AccountOperationKind::Refresh,
			display_label: None,
			enabled: None,
			expected_account_revision: Some(expected_account_revision.unwrap_or(account.revision)),
			expected: Some(current.clone()),
			target: None,
			provider: current.provider.clone(),
		};
		let phase = accepted_phase(self.store.prepare_account_operation(&preparation).await?)?;
		if phase == AccountOperationPhase::Committed {
			let latest = self.load_account(account_id).await?;
			return self
				.project_refresh_result(
					account_id,
					latest.credential.as_ref().ok_or(AccountLifecycleError::CredentialAbsent)?,
					callback_generation,
				)
				.await;
		}
		if phase == AccountOperationPhase::Prepared {
			accepted_phase(
				self.store
					.advance_account_operation(
						&operation_id,
						AccountOperationPhase::Prepared,
						AccountOperationPhase::ProviderEffectPending,
						None,
					)
					.await?,
			)?;
		}
		let refresher = Arc::clone(&self.refresher);
		let refreshed =
			match tokio::task::spawn_blocking(move || refresher.refresh(stored.bundle())).await {
				Ok(refreshed) => refreshed,
				Err(_) => {
					self.recover_or_cancel(
						&operation_id,
						AccountOperationPhase::ProviderEffectPending,
						"provider_refresh_ambiguous",
						true,
					)
					.await?;
					return Err(AccountLifecycleError::Refresh(CredentialRefreshError::Ambiguous));
				},
			};
		let refreshed = match refreshed {
			Ok(refreshed) => refreshed,
			Err(CredentialRefreshError::Rejected) => {
				self.recover_or_cancel(
					&operation_id,
					AccountOperationPhase::ProviderEffectPending,
					"provider_refresh_rejected",
					false,
				)
				.await?;
				return Err(AccountLifecycleError::Refresh(CredentialRefreshError::Rejected));
			},
			Err(error) => {
				self.recover_or_cancel(
					&operation_id,
					AccountOperationPhase::ProviderEffectPending,
					"provider_refresh_ambiguous",
					true,
				)
				.await?;
				return Err(AccountLifecycleError::Refresh(error));
			},
		};
		let target =
			match refreshed_credential_target(current, account_id, &operation_id, &refreshed) {
				Ok(target) => target,
				Err(AccountLifecycleError::ProviderMismatch) => {
					self.recover_or_cancel(
						&operation_id,
						AccountOperationPhase::ProviderEffectPending,
						PROVIDER_REFRESH_OUTCOME_UNKNOWN,
						true,
					)
					.await?;
					return Err(AccountLifecycleError::ProviderMismatch);
				},
				Err(error) => return Err(error),
			};
		accepted_phase(self.store.set_account_operation_target(&operation_id, &target).await?)?;
		let projection_bundle = refreshed.bundle.clone();
		if let Err(error) =
			self.credentials.compare_and_swap_rotate(account_id, current, &target, refreshed.bundle)
		{
			self.recover_or_cancel(
				&operation_id,
				AccountOperationPhase::ProviderEffectPending,
				"credential_rotate_failed",
				true,
			)
			.await?;
			return Err(error.into());
		}
		accepted_phase(
			self.store
				.advance_account_operation(
					&operation_id,
					AccountOperationPhase::ProviderEffectPending,
					AccountOperationPhase::StoreApplied,
					None,
				)
				.await?,
		)?;
		self.commit_store_applied(&operation_id).await?;
		if let Some((generation_id, process_binding)) = callback_generation {
			self.require_active_callback_generation(account_id, generation_id, process_binding)
				.await?;
		}
		Ok(projection(&target, &projection_bundle))
	}

	/// Replace one exact account credential from a private Codex auth file.
	#[allow(clippy::too_many_arguments)] // The receipt, operation, account fence, private source, and response owner are independent inputs.
	pub(crate) async fn reauthenticate_from_credential_file_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: AccountOperationId,
		account_id: &AccountId,
		expected_account_revision: i64,
		source_descriptor: &str,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<&AccountRecord, AccountLifecycleError>) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let lock = self.lock_for(account_id)?;
		let _guard = lock.lock().await;
		let account = match self.load_account(account_id).await {
			Ok(account) => account,
			Err(error) => {
				return self.complete_account_command_error(lease, error, build_response).await;
			},
		};
		if let Some(operation) = self.store.read_account_operation(&operation_id).await? {
			return self
				.complete_reauthentication_replay(
					lease,
					&operation_id,
					account_id,
					expected_account_revision,
					&operation,
					build_response,
				)
				.await;
		}
		let input = ReauthenticationCommandInput {
			operation_id,
			account_id,
			expected_account_revision,
			source_descriptor,
			account: &account,
		};
		self.continue_reauthentication_command(lease, input, build_response).await
	}

	async fn complete_reauthentication_replay<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: &AccountOperationId,
		account_id: &AccountId,
		expected_account_revision: i64,
		operation: &AccountOperation,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<&AccountRecord, AccountLifecycleError>) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let disposition = match self.reauthentication_replay_disposition(
			operation,
			account_id,
			expected_account_revision,
		) {
			Ok(disposition) => disposition,
			Err(error) => {
				return self.complete_account_command_error(lease, error, build_response).await;
			},
		};
		match disposition {
			ReauthenticationReplayDisposition::Complete => {
				if !matches!(
					operation.phase,
					AccountOperationPhase::StoreApplied | AccountOperationPhase::Committed
				) {
					accepted_phase(
						self.store
							.advance_account_operation(
								operation_id,
								operation.phase,
								AccountOperationPhase::StoreApplied,
								None,
							)
							.await?,
					)?;
				}
				self.complete_account_operation_success(
					lease,
					operation_id,
					AccountOperationPhase::StoreApplied,
					AccountOperationPhase::Committed,
					build_response,
				)
				.await
			},
			ReauthenticationReplayDisposition::Cancel =>
				self.complete_account_operation_error(
					lease,
					operation_id,
					operation.phase,
					AccountOperationPhase::Cancelled,
					None,
					AccountLifecycleError::InvalidOperation,
					build_response,
				)
				.await,
			ReauthenticationReplayDisposition::Recover =>
				self.complete_account_operation_error(
					lease,
					operation_id,
					operation.phase,
					AccountOperationPhase::RecoveryRequired,
					Some("credential_reauthentication_reconciliation"),
					AccountLifecycleError::NotReady(AccountLifecycleReadiness::OperationUnsettled),
					build_response,
				)
				.await,
		}
	}

	fn reauthentication_replay_disposition(
		&self,
		operation: &AccountOperation,
		account_id: &AccountId,
		expected_account_revision: i64,
	) -> Result<ReauthenticationReplayDisposition, AccountLifecycleError> {
		self.require_operation_identity(operation, account_id, AccountOperationKind::Refresh)?;
		if operation.expected_account_revision != Some(expected_account_revision) {
			return Err(AccountLifecycleError::StaleAccount);
		}
		Ok(classify_reauthentication_replay(
			operation.phase,
			operation.expected.as_ref(),
			operation.target.as_ref(),
			|binding| self.credentials.read_exact(account_id, binding).map(|_| ()),
		))
	}

	async fn continue_reauthentication_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		input: ReauthenticationCommandInput<'_>,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<&AccountRecord, AccountLifecycleError>) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let ReauthenticationCommandInput {
			operation_id,
			account_id,
			expected_account_revision,
			source_descriptor,
			account,
		} = input;
		let (current, imported, target) = match self.reauthentication_material(
			account,
			expected_account_revision,
			account_id,
			&operation_id,
			source_descriptor,
		) {
			Ok(material) => material,
			Err(error) => {
				return self.complete_account_command_error(lease, error, build_response).await;
			},
		};
		let preparation = AccountOperationPreparation {
			operation_id: operation_id.clone(),
			account_id: account_id.clone(),
			kind: AccountOperationKind::Refresh,
			display_label: None,
			enabled: None,
			expected_account_revision: Some(expected_account_revision),
			expected: Some(current.clone()),
			target: Some(target.clone()),
			provider: imported.provider.clone(),
		};
		let phase = match self.store.prepare_account_operation(&preparation).await? {
			AccountLifecycleMutationOutcome::Applied(mutation)
			| AccountLifecycleMutationOutcome::Replayed(mutation) => mutation.phase,
			AccountLifecycleMutationOutcome::Rejected { rejection, .. } => {
				return self
					.complete_account_command_error(
						lease,
						AccountLifecycleError::OperationRejected(rejection),
						build_response,
					)
					.await;
			},
		};
		if phase != AccountOperationPhase::Prepared {
			return self
				.complete_account_command_error(
					lease,
					AccountLifecycleError::InvalidOperation,
					build_response,
				)
				.await;
		}
		let rotation = self.credentials.compare_and_swap_rotate(
			account_id,
			&current,
			&target,
			imported.bundle,
		);
		let target_is_exact =
			rotation.is_err() && self.credentials.read_exact(account_id, &target).is_ok();
		if let Err(error) = resolve_reauthentication_store_effect(rotation, target_is_exact) {
			let ambiguous = store_effect_may_be_ambiguous(error);
			return self
				.complete_account_operation_error(
					lease,
					&operation_id,
					AccountOperationPhase::Prepared,
					if ambiguous {
						AccountOperationPhase::RecoveryRequired
					} else {
						AccountOperationPhase::Cancelled
					},
					ambiguous.then_some("credential_reauthentication_reconciliation"),
					error.into(),
					build_response,
				)
				.await;
		}
		accepted_phase(
			self.store
				.advance_account_operation(
					&operation_id,
					AccountOperationPhase::Prepared,
					AccountOperationPhase::StoreApplied,
					None,
				)
				.await?,
		)?;
		self.complete_account_operation_success(
			lease,
			&operation_id,
			AccountOperationPhase::StoreApplied,
			AccountOperationPhase::Committed,
			build_response,
		)
		.await
	}

	fn reauthentication_material(
		&self,
		account: &AccountRecord,
		expected_account_revision: i64,
		account_id: &AccountId,
		operation_id: &AccountOperationId,
		source_descriptor: &str,
	) -> Result<(CredentialBinding, ImportedCredential, CredentialBinding), AccountLifecycleError>
	{
		let current = reauthentication_current(account, expected_account_revision)?;
		let imported = read_explicit_shared_codex_credential_file(source_descriptor)?;
		let target = reauthentication_target(&current, account_id, operation_id, &imported)?;
		self.credentials.read_exact(account_id, &current)?;
		Ok((current, imported, target))
	}

	/// Refresh one account and atomically commit the exact final registry result with its logical
	/// command receipt.
	#[allow(clippy::too_many_lines)] // Keep the journaled refresh and recovery transitions in one auditable sequence.
	pub(crate) async fn refresh_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: AccountOperationId,
		account_id: &AccountId,
		expected_account_revision: i64,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<&AccountRecord, AccountLifecycleError>) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let lock = self.lock_for(account_id)?;
		let _guard = lock.lock().await;
		let mut build_response = Some(build_response);
		let account = match self.load_account(account_id).await {
			Ok(account) => account,
			Err(error) => {
				return self
					.complete_account_command_error(
						lease,
						error,
						build_response.take().expect("builder is retained"),
					)
					.await;
			},
		};
		let current = match account.credential.clone() {
			Some(current) => current,
			None => {
				return self
					.complete_account_command_error(
						lease,
						AccountLifecycleError::CredentialAbsent,
						build_response.take().expect("builder is retained"),
					)
					.await;
			},
		};
		if let Some(operation) = self.store.read_account_operation(&operation_id).await? {
			if let Err(error) = self.require_operation_identity(
				&operation,
				account_id,
				AccountOperationKind::Refresh,
			) {
				return self
					.complete_account_command_error(
						lease,
						error,
						build_response.take().expect("builder is retained"),
					)
					.await;
			}
			match operation.phase {
				AccountOperationPhase::Committed | AccountOperationPhase::StoreApplied => {
					return self
						.complete_account_operation_success(
							lease,
							&operation_id,
							AccountOperationPhase::StoreApplied,
							AccountOperationPhase::Committed,
							build_response.take().expect("builder is retained"),
						)
						.await;
				},
				AccountOperationPhase::Cancelled => {
					return self
						.complete_account_operation_error(
							lease,
							&operation_id,
							operation.phase,
							operation.phase,
							None,
							AccountLifecycleError::InvalidOperation,
							build_response.take().expect("builder is retained"),
						)
						.await;
				},
				AccountOperationPhase::RecoveryRequired => {
					return self
						.complete_account_operation_error(
							lease,
							&operation_id,
							operation.phase,
							operation.phase,
							None,
							AccountLifecycleError::NotReady(
								AccountLifecycleReadiness::OperationUnsettled,
							),
							build_response.take().expect("builder is retained"),
						)
						.await;
				},
				AccountOperationPhase::Prepared => {
					return self
						.complete_account_operation_error(
							lease,
							&operation_id,
							operation.phase,
							AccountOperationPhase::Cancelled,
							None,
							AccountLifecycleError::InvalidOperation,
							build_response.take().expect("builder is retained"),
						)
						.await;
				},
				AccountOperationPhase::ProviderEffectPending => {
					let Some(target) = operation.target.as_ref() else {
						return self
							.complete_account_operation_error(
								lease,
								&operation_id,
								operation.phase,
								AccountOperationPhase::RecoveryRequired,
								Some(PROVIDER_REFRESH_OUTCOME_UNKNOWN),
								AccountLifecycleError::NotReady(
									AccountLifecycleReadiness::OperationUnsettled,
								),
								build_response.take().expect("builder is retained"),
							)
							.await;
					};
					if self.credentials.read_exact(account_id, target).is_err() {
						return self
							.complete_account_operation_error(
								lease,
								&operation_id,
								operation.phase,
								AccountOperationPhase::RecoveryRequired,
								Some("credential_refresh_reconciliation"),
								AccountLifecycleError::NotReady(
									AccountLifecycleReadiness::OperationUnsettled,
								),
								build_response.take().expect("builder is retained"),
							)
							.await;
					}
					accepted_phase(
						self.store
							.advance_account_operation(
								&operation_id,
								operation.phase,
								AccountOperationPhase::StoreApplied,
								None,
							)
							.await?,
					)?;
					return self
						.complete_account_operation_success(
							lease,
							&operation_id,
							AccountOperationPhase::StoreApplied,
							AccountOperationPhase::Committed,
							build_response.take().expect("builder is retained"),
						)
						.await;
				},
			}
		}
		if account.revision != expected_account_revision {
			return self
				.complete_account_command_error(
					lease,
					AccountLifecycleError::StaleAccount,
					build_response.take().expect("builder is retained"),
				)
				.await;
		}
		let stored = match self.read_exact_for_admission(&account).await {
			Ok(stored) => stored,
			Err(error) => {
				return self
					.complete_account_command_error(
						lease,
						error,
						build_response.take().expect("builder is retained"),
					)
					.await;
			},
		};
		let now_unix_micros = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.ok()
			.and_then(|duration| i64::try_from(duration.as_micros()).ok());
		let shared_refresh = now_unix_micros.and_then(|now_unix_micros| {
			matching_shared_refresh(
				&current,
				stored.bundle(),
				now_unix_micros,
				read_shared_codex_credential(),
			)
		});
		let preparation = AccountOperationPreparation {
			operation_id: operation_id.clone(),
			account_id: account_id.clone(),
			kind: AccountOperationKind::Refresh,
			display_label: None,
			enabled: None,
			expected_account_revision: Some(expected_account_revision),
			expected: Some(current.clone()),
			target: None,
			provider: current.provider.clone(),
		};
		match self.store.prepare_account_operation(&preparation).await? {
			AccountLifecycleMutationOutcome::Applied(mutation)
			| AccountLifecycleMutationOutcome::Replayed(mutation)
				if mutation.phase == AccountOperationPhase::Prepared => {},
			AccountLifecycleMutationOutcome::Rejected { rejection, .. } => {
				return self
					.complete_account_command_error(
						lease,
						AccountLifecycleError::OperationRejected(rejection),
						build_response.take().expect("builder is retained"),
					)
					.await;
			},
			_ => {
				return self
					.complete_account_command_error(
						lease,
						AccountLifecycleError::InvalidOperation,
						build_response.take().expect("builder is retained"),
					)
					.await;
			},
		}
		accepted_phase(
			self.store
				.advance_account_operation(
					&operation_id,
					AccountOperationPhase::Prepared,
					AccountOperationPhase::ProviderEffectPending,
					None,
				)
				.await?,
		)?;
		let refreshed = match shared_refresh {
			Some(refreshed) => Ok(refreshed),
			None => {
				let refresher = Arc::clone(&self.refresher);
				match tokio::task::spawn_blocking(move || refresher.refresh(stored.bundle())).await
				{
					Ok(refreshed) => refreshed,
					Err(_) => {
						return self
							.complete_account_operation_error(
								lease,
								&operation_id,
								AccountOperationPhase::ProviderEffectPending,
								AccountOperationPhase::RecoveryRequired,
								Some("provider_refresh_ambiguous"),
								AccountLifecycleError::Refresh(CredentialRefreshError::Ambiguous),
								build_response.take().expect("builder is retained"),
							)
							.await;
					},
				}
			},
		};
		let refreshed = match refreshed {
			Ok(refreshed) => refreshed,
			Err(error @ CredentialRefreshError::Rejected) => {
				return self
					.complete_account_operation_error(
						lease,
						&operation_id,
						AccountOperationPhase::ProviderEffectPending,
						AccountOperationPhase::Cancelled,
						None,
						AccountLifecycleError::Refresh(error),
						build_response.take().expect("builder is retained"),
					)
					.await;
			},
			Err(error) => {
				return self
					.complete_account_operation_error(
						lease,
						&operation_id,
						AccountOperationPhase::ProviderEffectPending,
						AccountOperationPhase::RecoveryRequired,
						Some("provider_refresh_ambiguous"),
						AccountLifecycleError::Refresh(error),
						build_response.take().expect("builder is retained"),
					)
					.await;
			},
		};
		let target =
			match refreshed_credential_target(&current, account_id, &operation_id, &refreshed) {
				Ok(target) => target,
				Err(AccountLifecycleError::ProviderMismatch) => {
					return self
						.complete_account_operation_error(
							lease,
							&operation_id,
							AccountOperationPhase::ProviderEffectPending,
							AccountOperationPhase::RecoveryRequired,
							Some(PROVIDER_REFRESH_OUTCOME_UNKNOWN),
							AccountLifecycleError::ProviderMismatch,
							build_response.take().expect("builder is retained"),
						)
						.await;
				},
				Err(error) => return Err(error),
			};
		accepted_phase(self.store.set_account_operation_target(&operation_id, &target).await?)?;
		if let Err(error) = self.credentials.compare_and_swap_rotate(
			account_id,
			&current,
			&target,
			refreshed.bundle,
		) {
			return self
				.complete_account_operation_error(
					lease,
					&operation_id,
					AccountOperationPhase::ProviderEffectPending,
					AccountOperationPhase::RecoveryRequired,
					Some("credential_rotate_failed"),
					error.into(),
					build_response.take().expect("builder is retained"),
				)
				.await;
		}
		accepted_phase(
			self.store
				.advance_account_operation(
					&operation_id,
					AccountOperationPhase::ProviderEffectPending,
					AccountOperationPhase::StoreApplied,
					None,
				)
				.await?,
		)?;
		self.complete_account_operation_success(
			lease,
			&operation_id,
			AccountOperationPhase::StoreApplied,
			AccountOperationPhase::Committed,
			build_response.take().expect("builder is retained"),
		)
		.await
	}

	async fn project_refresh_result(
		&self,
		account_id: &AccountId,
		binding: &CredentialBinding,
		callback_generation: Option<(&ProcessGenerationId, &ProcessGenerationAccountBinding)>,
	) -> Result<ChatgptTokenProjection, AccountLifecycleError> {
		let projection = self.project_exact(account_id, binding)?;
		if let Some((generation_id, process_binding)) = callback_generation {
			self.require_active_callback_generation(account_id, generation_id, process_binding)
				.await?;
		}
		Ok(projection)
	}

	async fn require_active_callback_generation(
		&self,
		account_id: &AccountId,
		generation_id: &ProcessGenerationId,
		expected: &ProcessGenerationAccountBinding,
	) -> Result<(), AccountLifecycleError> {
		let generation = self
			.store
			.read_bound_process_generations(Some(account_id), false, 256)
			.await?
			.into_iter()
			.find(|candidate| candidate.generation.generation_id == *generation_id)
			.ok_or(AccountLifecycleError::StaleAccount)?;
		if !matches!(
			generation.generation.state,
			ProcessGenerationState::Starting | ProcessGenerationState::Ready
		) || generation.account_binding.as_ref() != Some(expected)
		{
			return Err(AccountLifecycleError::StaleAccount);
		}
		Ok(())
	}

	/// Delete an exact credential bundle, then tombstone its account projection.
	pub async fn logout(
		&self,
		operation_id: AccountOperationId,
		account_id: &AccountId,
		expected_revision: i64,
	) -> Result<AccountRecord, AccountLifecycleError> {
		let lock = self.lock_for(account_id)?;
		let _guard = lock.lock().await;
		if let Some(operation) = self.store.read_account_operation(&operation_id).await? {
			self.require_operation_identity(&operation, account_id, AccountOperationKind::Logout)?;
			return match self.reconcile_operation(&operation).await? {
				ReconciliationDisposition::Committed => self.load_account(account_id).await,
				ReconciliationDisposition::Cancelled =>
					Err(AccountLifecycleError::InvalidOperation),
				ReconciliationDisposition::Manual => Err(AccountLifecycleError::NotReady(
					AccountLifecycleReadiness::OperationUnsettled,
				)),
			};
		}
		let account = self.load_account(account_id).await?;
		let expected = account.credential.clone().ok_or(AccountLifecycleError::CredentialAbsent)?;
		let preparation = AccountOperationPreparation {
			operation_id: operation_id.clone(),
			account_id: account_id.clone(),
			kind: AccountOperationKind::Logout,
			display_label: None,
			enabled: None,
			expected_account_revision: Some(expected_revision),
			expected: Some(expected.clone()),
			target: None,
			provider: expected.provider.clone(),
		};
		let phase = accepted_phase(self.store.prepare_account_operation(&preparation).await?)?;
		if phase == AccountOperationPhase::Prepared {
			match self.credentials.delete(account_id, &expected) {
				Ok(()) | Err(CredentialStoreError::NotFound) => {},
				Err(error) => {
					self.recover_or_cancel(
						&operation_id,
						AccountOperationPhase::Prepared,
						"credential_delete_failed",
						true,
					)
					.await?;
					return Err(error.into());
				},
			}
			accepted_phase(
				self.store
					.advance_account_operation(
						&operation_id,
						AccountOperationPhase::Prepared,
						AccountOperationPhase::StoreApplied,
						None,
					)
					.await?,
			)?;
		}
		self.commit_store_applied(&operation_id).await?;
		self.load_account(account_id).await
	}

	/// Delete one exact host bundle and atomically commit the tombstone with its command result.
	#[allow(clippy::too_many_lines)] // Keep the journaled delete and tombstone transitions in one auditable sequence.
	pub(crate) async fn logout_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: AccountOperationId,
		account_id: &AccountId,
		expected_revision: i64,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<&AccountRecord, AccountLifecycleError>) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let lock = self.lock_for(account_id)?;
		let _guard = lock.lock().await;
		let mut build_response = Some(build_response);
		let mut phase = None;
		let mut expected = None;
		if let Some(operation) = self.store.read_account_operation(&operation_id).await? {
			if let Err(error) = self.require_operation_identity(
				&operation,
				account_id,
				AccountOperationKind::Logout,
			) {
				return self
					.complete_account_command_error(
						lease,
						error,
						build_response.take().expect("builder is retained"),
					)
					.await;
			}
			phase = Some(operation.phase);
			expected = operation.expected;
		}
		if phase.is_none() {
			let account = match self.load_account(account_id).await {
				Ok(account) => account,
				Err(error) => {
					return self
						.complete_account_command_error(
							lease,
							error,
							build_response.take().expect("builder is retained"),
						)
						.await;
				},
			};
			let credential = match account.credential.clone() {
				Some(credential) => credential,
				None => {
					return self
						.complete_account_command_error(
							lease,
							AccountLifecycleError::CredentialAbsent,
							build_response.take().expect("builder is retained"),
						)
						.await;
				},
			};
			let preparation = AccountOperationPreparation {
				operation_id: operation_id.clone(),
				account_id: account_id.clone(),
				kind: AccountOperationKind::Logout,
				display_label: None,
				enabled: None,
				expected_account_revision: Some(expected_revision),
				expected: Some(credential.clone()),
				target: None,
				provider: credential.provider.clone(),
			};
			match self.store.prepare_account_operation(&preparation).await? {
				AccountLifecycleMutationOutcome::Applied(mutation)
				| AccountLifecycleMutationOutcome::Replayed(mutation) => phase = Some(mutation.phase),
				AccountLifecycleMutationOutcome::Rejected { rejection, .. } => {
					return self
						.complete_account_command_error(
							lease,
							AccountLifecycleError::OperationRejected(rejection),
							build_response.take().expect("builder is retained"),
						)
						.await;
				},
			}
			expected = Some(credential);
		}
		let phase = phase.expect("logout preparation or replay has one phase");
		match phase {
			AccountOperationPhase::Committed | AccountOperationPhase::StoreApplied => {
				return self
					.complete_account_operation_success(
						lease,
						&operation_id,
						AccountOperationPhase::StoreApplied,
						AccountOperationPhase::Committed,
						build_response.take().expect("builder is retained"),
					)
					.await;
			},
			AccountOperationPhase::Cancelled => {
				return self
					.complete_account_operation_error(
						lease,
						&operation_id,
						phase,
						phase,
						None,
						AccountLifecycleError::InvalidOperation,
						build_response.take().expect("builder is retained"),
					)
					.await;
			},
			AccountOperationPhase::RecoveryRequired
			| AccountOperationPhase::ProviderEffectPending => {
				return self
					.complete_account_operation_error(
						lease,
						&operation_id,
						phase,
						phase,
						None,
						AccountLifecycleError::NotReady(
							AccountLifecycleReadiness::OperationUnsettled,
						),
						build_response.take().expect("builder is retained"),
					)
					.await;
			},
			AccountOperationPhase::Prepared => {},
		}
		let Some(expected) = expected else {
			return self
				.complete_account_operation_error(
					lease,
					&operation_id,
					AccountOperationPhase::Prepared,
					AccountOperationPhase::RecoveryRequired,
					Some("credential_logout_reconciliation"),
					AccountLifecycleError::InvalidOperation,
					build_response.take().expect("builder is retained"),
				)
				.await;
		};
		if let Err(error) = self.credentials.delete(account_id, &expected)
			&& error != CredentialStoreError::NotFound
		{
			return self
				.complete_account_operation_error(
					lease,
					&operation_id,
					AccountOperationPhase::Prepared,
					AccountOperationPhase::RecoveryRequired,
					Some("credential_delete_failed"),
					error.into(),
					build_response.take().expect("builder is retained"),
				)
				.await;
		}
		accepted_phase(
			self.store
				.advance_account_operation(
					&operation_id,
					AccountOperationPhase::Prepared,
					AccountOperationPhase::StoreApplied,
					None,
				)
				.await?,
		)?;
		self.complete_account_operation_success(
			lease,
			&operation_id,
			AccountOperationPhase::StoreApplied,
			AccountOperationPhase::Committed,
			build_response.take().expect("builder is retained"),
		)
		.await
	}

	/// Apply one enablement command and its durable public result in one PG transaction.
	pub async fn set_account_enabled_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		account_id: &AccountId,
		expected_revision: i64,
		enabled: bool,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(
				&AccountAdministrationOutcome,
				Option<&AccountRecord>,
			) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let lock = self.lock_for(account_id)?;
		let _guard = lock.lock().await;
		Ok(self
			.store
			.set_account_enabled_command(
				lease,
				account_id,
				expected_revision,
				enabled,
				build_response,
			)
			.await?)
	}

	/// Select one fixed account under independent routing and account revision guards.
	pub async fn set_fixed_selection(
		&self,
		expected_routing_revision: i64,
		account_id: &AccountId,
		expected_account_revision: i64,
	) -> Result<RoutingControlOutcome, AccountLifecycleError> {
		Ok(self
			.store
			.set_fixed_account_selection(
				expected_routing_revision,
				account_id,
				expected_account_revision,
			)
			.await?)
	}

	/// Apply one fixed-selection command and its durable public result in one PG transaction.
	pub async fn set_fixed_selection_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		expected_routing_revision: i64,
		account_id: &AccountId,
		expected_account_revision: i64,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(&RoutingControlOutcome) -> Result<Value, StoreError> + Send + 'static,
	{
		Ok(self
			.store
			.set_fixed_account_selection_command(
				lease,
				expected_routing_revision,
				account_id,
				expected_account_revision,
				build_response,
			)
			.await?)
	}

	/// Select balanced routing while preserving the complete account order.
	pub async fn set_balanced_selection(
		&self,
		expected_routing_revision: i64,
	) -> Result<RoutingControlOutcome, AccountLifecycleError> {
		Ok(self.store.set_balanced_account_selection(expected_routing_revision).await?)
	}

	/// Apply one balanced-selection command and its durable result in one PG transaction.
	pub async fn set_balanced_selection_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		expected_routing_revision: i64,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(&RoutingControlOutcome) -> Result<Value, StoreError> + Send + 'static,
	{
		Ok(self
			.store
			.set_balanced_account_selection_command(
				lease,
				expected_routing_revision,
				build_response,
			)
			.await?)
	}

	/// Replace the complete order while preserving selection mode and fixed target.
	pub async fn set_account_order(
		&self,
		expected_routing_revision: i64,
		order: &[AccountId],
	) -> Result<RoutingControlOutcome, AccountLifecycleError> {
		Ok(self.store.set_account_order(expected_routing_revision, order).await?)
	}

	/// Apply one account-order command and its durable result in one PG transaction.
	pub async fn set_account_order_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		expected_routing_revision: i64,
		order: &[AccountId],
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(&RoutingControlOutcome) -> Result<Value, StoreError> + Send + 'static,
	{
		Ok(self
			.store
			.set_account_order_command(lease, expected_routing_revision, order, build_response)
			.await?)
	}

	async fn complete_account_command_error<F>(
		&self,
		lease: AccountCommandReceiptLease,
		error: AccountLifecycleError,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<&AccountRecord, AccountLifecycleError>) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let response = build_response(Err(error))?;
		self.store.complete_account_command(lease, &response).await?;
		Ok(response)
	}

	async fn complete_account_operation_success<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: &AccountOperationId,
		expected: AccountOperationPhase,
		target: AccountOperationPhase,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<&AccountRecord, AccountLifecycleError>) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		Ok(self
			.store
			.complete_account_operation_command(
				lease,
				operation_id,
				expected,
				target,
				None,
				move |outcome, _, account| {
					let result = match outcome {
						AccountLifecycleMutationOutcome::Applied(mutation)
						| AccountLifecycleMutationOutcome::Replayed(mutation)
							if mutation.phase == target =>
							account.ok_or(AccountLifecycleError::AccountMissing),
						AccountLifecycleMutationOutcome::Rejected { rejection, .. } =>
							Err(AccountLifecycleError::OperationRejected(*rejection)),
						_ => Err(AccountLifecycleError::InvalidOperation),
					};
					build_response(result)
				},
			)
			.await?)
	}

	#[allow(clippy::too_many_arguments)]
	async fn complete_account_operation_error<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: &AccountOperationId,
		expected: AccountOperationPhase,
		target: AccountOperationPhase,
		recovery_code: Option<&str>,
		error: AccountLifecycleError,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<&AccountRecord, AccountLifecycleError>) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		Ok(self
			.store
			.complete_account_operation_command(
				lease,
				operation_id,
				expected,
				target,
				recovery_code,
				move |outcome, _, _| {
					let error = match outcome {
						AccountLifecycleMutationOutcome::Applied(mutation)
						| AccountLifecycleMutationOutcome::Replayed(mutation)
							if mutation.phase == target =>
							error,
						AccountLifecycleMutationOutcome::Rejected { rejection, .. } =>
							AccountLifecycleError::OperationRejected(*rejection),
						_ => AccountLifecycleError::InvalidOperation,
					};
					build_response(Err(error))
				},
			)
			.await?)
	}

	async fn complete_recovery_command_error<F>(
		&self,
		lease: AccountCommandReceiptLease,
		error: AccountLifecycleError,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(
				Result<(AccountManualRecoveryOutcome, &AccountRecord), AccountLifecycleError>,
			) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let response = build_response(Err(error))?;
		self.store.complete_account_command(lease, &response).await?;
		Ok(response)
	}

	#[allow(clippy::too_many_arguments)]
	async fn complete_recovery_operation_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: &AccountOperationId,
		expected: AccountOperationPhase,
		target: AccountOperationPhase,
		recovery_code: Option<&str>,
		result: AccountManualRecoveryOutcome,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(
				Result<(AccountManualRecoveryOutcome, &AccountRecord), AccountLifecycleError>,
			) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		Ok(self
			.store
			.complete_account_operation_command(
				lease,
				operation_id,
				expected,
				target,
				recovery_code,
				move |outcome, _, account| {
					let value = match outcome {
						AccountLifecycleMutationOutcome::Applied(mutation)
						| AccountLifecycleMutationOutcome::Replayed(mutation)
							if mutation.phase == target =>
							account
								.ok_or(AccountLifecycleError::AccountMissing)
								.map(|account| (result, account)),
						AccountLifecycleMutationOutcome::Rejected { rejection, .. } =>
							Err(AccountLifecycleError::OperationRejected(*rejection)),
						_ => Err(AccountLifecycleError::InvalidOperation),
					};
					build_response(value)
				},
			)
			.await?)
	}

	/// Persist one accepted 300-minute or 10080-minute quota fact.
	pub async fn observe_quota(
		&self,
		account_id: &AccountId,
		fact: decodex_core::AccountQuotaWindow,
		observed_at_unix_micros: i64,
	) -> Result<(), AccountLifecycleError> {
		Ok(self.store.observe_account_quota(account_id, fact, observed_at_unix_micros).await?)
	}

	/// Persist one bounded row-scoped quota observation error for both list and Reset Card reads.
	pub async fn observe_quota_error(
		&self,
		account_id: &AccountId,
		duration_minutes: u32,
		error: decodex_core::AccountQuotaObservationError,
		observed_at_unix_micros: i64,
	) -> Result<(), AccountLifecycleError> {
		Ok(self
			.store
			.observe_account_quota_error(
				account_id,
				duration_minutes,
				error,
				observed_at_unix_micros,
			)
			.await?)
	}

	/// Select one initial account. This never creates automatic same-thread fallback or wake work.
	pub async fn select_initial(
		&self,
		now_unix_micros: i64,
	) -> Result<AccountSelectionResult, AccountSelectionFailure> {
		let callback_profile_sha256 = self
			.callback_profile()
			.map_err(|recovery| AccountSelectionFailure { account_id: None, recovery })?;
		let (accounts, control) =
			self.store.read_account_registry_snapshot(MAX_ACCOUNT_READ).await.map_err(|_| {
				AccountSelectionFailure {
					account_id: None,
					recovery: AccountSelectionRecovery::ResolveCredentialOperation,
				}
			})?;
		let by_id = accounts
			.into_iter()
			.map(|account| (account.account_id.clone(), account))
			.collect::<HashMap<_, _>>();
		match &control.mode {
			AccountSelectionMode::Fixed(account_id) => {
				let account = by_id.get(account_id).ok_or(AccountSelectionFailure {
					account_id: Some(account_id.clone()),
					recovery: AccountSelectionRecovery::ConfigureFixedAccount,
				})?;
				self.selection_candidate(account, now_unix_micros).map_err(|recovery| {
					AccountSelectionFailure { account_id: Some(account_id.clone()), recovery }
				})?;
				Ok(AccountSelectionResult { account: account.clone(), callback_profile_sha256 })
			},
			AccountSelectionMode::Balanced => {
				let mut first_failure = None;
				for account_id in &control.order {
					let account = by_id.get(account_id).ok_or(AccountSelectionFailure {
						account_id: Some(account_id.clone()),
						recovery: AccountSelectionRecovery::ResolveCredentialOperation,
					})?;
					match self.selection_candidate(account, now_unix_micros) {
						Ok(_) => {
							return Ok(AccountSelectionResult {
								account: account.clone(),
								callback_profile_sha256,
							});
						},
						Err(recovery) if first_failure.is_none() => {
							first_failure = Some((account.account_id.clone(), recovery));
						},
						Err(_) => {},
					}
				}
				let (account_id, recovery) = first_failure.map_or(
					(None, AccountSelectionRecovery::EnrollCredentials),
					|(account_id, recovery)| (Some(account_id), recovery),
				);
				Err(AccountSelectionFailure { account_id, recovery })
			},
		}
	}

	/// Reconcile every admission-blocking operation before serving account work.
	pub async fn reconcile_startup(
		&self,
	) -> Result<StartupAccountReconciliation, AccountLifecycleError> {
		let operations = self.store.read_unsettled_account_operations(MAX_ACCOUNT_READ).await?;
		let mut summary = StartupAccountReconciliation::default();
		for operation in operations {
			let lock = self.lock_for(&operation.account_id)?;
			let _guard = lock.lock().await;
			match self.reconcile_operation(&operation).await? {
				ReconciliationDisposition::Committed => summary.committed += 1,
				ReconciliationDisposition::Cancelled => summary.cancelled += 1,
				ReconciliationDisposition::Manual => summary
					.manual_recovery
					.push((operation.account_id.clone(), operation.operation_id.clone())),
			}
		}
		for account in self.store.read_account_registry(None, MAX_ACCOUNT_READ).await? {
			if let Some(binding) = account.credential.as_ref() {
				self.observe_exact_store(&account, binding).await?;
			}
		}
		Ok(summary)
	}

	/// Apply one typed manual recovery action after re-reading the exact host-store state.
	pub async fn recover_operation(
		&self,
		operation_id: &AccountOperationId,
		expected_account_revision: i64,
		action: AccountManualRecoveryAction,
	) -> Result<(AccountManualRecoveryOutcome, AccountRecord), AccountLifecycleError> {
		let operation = self
			.store
			.read_account_operation(operation_id)
			.await?
			.ok_or(AccountLifecycleError::InvalidOperation)?;
		let lock = self.lock_for(&operation.account_id)?;
		let _guard = lock.lock().await;
		let operation = self
			.store
			.read_account_operation(operation_id)
			.await?
			.ok_or(AccountLifecycleError::InvalidOperation)?;
		let replayed = match (operation.phase, action) {
			(
				AccountOperationPhase::Committed,
				AccountManualRecoveryAction::ReconcileExactStoreState,
			) => Some(AccountManualRecoveryOutcome::Committed),
			(AccountOperationPhase::Cancelled, AccountManualRecoveryAction::CancelBeforeEffect) =>
				Some(AccountManualRecoveryOutcome::Cancelled),
			(AccountOperationPhase::Committed | AccountOperationPhase::Cancelled, _) => {
				return Err(AccountLifecycleError::InvalidOperation);
			},
			_ => None,
		};
		if let Some(outcome) = replayed {
			let account = self.load_account(&operation.account_id).await?;
			return Ok((outcome, account));
		}
		if self.load_account(&operation.account_id).await?.revision != expected_account_revision {
			return Err(AccountLifecycleError::StaleAccount);
		}
		let outcome = match action {
			AccountManualRecoveryAction::ReconcileExactStoreState =>
				self.recover_from_exact_store(&operation).await?,
			AccountManualRecoveryAction::CancelBeforeEffect =>
				self.cancel_proven_before_effect(&operation).await?,
		};
		let account = self.load_account(&operation.account_id).await?;
		Ok((outcome, account))
	}

	/// Apply one manual recovery command and commit its phase/result projection atomically.
	#[allow(clippy::too_many_lines)] // Keep every finite manual-recovery branch in one auditable sequence.
	pub(crate) async fn recover_operation_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: &AccountOperationId,
		expected_account_revision: i64,
		action: AccountManualRecoveryAction,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(
				Result<(AccountManualRecoveryOutcome, &AccountRecord), AccountLifecycleError>,
			) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let Some(initial) = self.store.read_account_operation(operation_id).await? else {
			return self
				.complete_recovery_command_error(
					lease,
					AccountLifecycleError::InvalidOperation,
					build_response,
				)
				.await;
		};
		let lock = self.lock_for(&initial.account_id)?;
		let _guard = lock.lock().await;
		let mut build_response = Some(build_response);
		let Some(operation) = self.store.read_account_operation(operation_id).await? else {
			return self
				.complete_recovery_command_error(
					lease,
					AccountLifecycleError::InvalidOperation,
					build_response.take().expect("builder is retained"),
				)
				.await;
		};
		let terminal_replay = match (operation.phase, action) {
			(
				AccountOperationPhase::Committed,
				AccountManualRecoveryAction::ReconcileExactStoreState,
			) => Some(AccountManualRecoveryOutcome::Committed),
			(AccountOperationPhase::Cancelled, AccountManualRecoveryAction::CancelBeforeEffect) =>
				Some(AccountManualRecoveryOutcome::Cancelled),
			(AccountOperationPhase::Committed | AccountOperationPhase::Cancelled, _) => {
				return self
					.complete_recovery_command_error(
						lease,
						AccountLifecycleError::InvalidOperation,
						build_response.take().expect("builder is retained"),
					)
					.await;
			},
			_ => None,
		};
		if let Some(outcome) = terminal_replay {
			return self
				.complete_recovery_operation_command(
					lease,
					operation_id,
					operation.phase,
					operation.phase,
					None,
					outcome,
					build_response.take().expect("builder is retained"),
				)
				.await;
		}
		let account = self.load_account(&operation.account_id).await?;
		if account.revision != expected_account_revision {
			return self
				.complete_recovery_command_error(
					lease,
					AccountLifecycleError::StaleAccount,
					build_response.take().expect("builder is retained"),
				)
				.await;
		}
		if action == AccountManualRecoveryAction::CancelBeforeEffect {
			let proven = match (operation.phase, operation.kind) {
				(
					AccountOperationPhase::Prepared,
					AccountOperationKind::Enroll | AccountOperationKind::Import,
				)
				| (
					AccountOperationPhase::RecoveryRequired,
					AccountOperationKind::Enroll | AccountOperationKind::Import,
				) => operation.target.as_ref().is_some_and(|target| {
					matches!(
						self.credentials.read_exact(&operation.account_id, target),
						Err(CredentialStoreError::NotFound)
					)
				}),
				(
					AccountOperationPhase::Prepared,
					AccountOperationKind::Refresh | AccountOperationKind::Logout,
				) => operation.expected.as_ref().is_some_and(|expected| {
					self.credentials.read_exact(&operation.account_id, expected).is_ok()
				}),
				(AccountOperationPhase::RecoveryRequired, AccountOperationKind::Logout) =>
					operation.expected.as_ref().is_some_and(|expected| {
						self.credentials.read_exact(&operation.account_id, expected).is_ok()
					}),
				_ => false,
			};
			let (target, outcome) = if proven {
				(AccountOperationPhase::Cancelled, AccountManualRecoveryOutcome::Cancelled)
			} else {
				(operation.phase, AccountManualRecoveryOutcome::StillRequiresRecovery)
			};
			return self
				.complete_recovery_operation_command(
					lease,
					operation_id,
					operation.phase,
					target,
					None,
					outcome,
					build_response.take().expect("builder is retained"),
				)
				.await;
		}
		if operation.phase == AccountOperationPhase::StoreApplied {
			return self
				.complete_recovery_operation_command(
					lease,
					operation_id,
					operation.phase,
					AccountOperationPhase::Committed,
					None,
					AccountManualRecoveryOutcome::Committed,
					build_response.take().expect("builder is retained"),
				)
				.await;
		}
		let proven_applied = match operation.kind {
			AccountOperationKind::Enroll
			| AccountOperationKind::Import
			| AccountOperationKind::Refresh => operation.target.as_ref().is_some_and(|target| {
				self.credentials.read_exact(&operation.account_id, target).is_ok()
			}),
			AccountOperationKind::Logout => operation.expected.as_ref().is_some_and(|expected| {
				matches!(
					self.credentials.read_exact(&operation.account_id, expected),
					Err(CredentialStoreError::NotFound)
				)
			}),
		};
		if proven_applied {
			accepted_phase(
				self.store
					.advance_account_operation(
						operation_id,
						operation.phase,
						AccountOperationPhase::StoreApplied,
						None,
					)
					.await?,
			)?;
			return self
				.complete_recovery_operation_command(
					lease,
					operation_id,
					AccountOperationPhase::StoreApplied,
					AccountOperationPhase::Committed,
					None,
					AccountManualRecoveryOutcome::Committed,
					build_response.take().expect("builder is retained"),
				)
				.await;
		}
		if operation.kind == AccountOperationKind::Logout {
			let Some(expected) = operation.expected.as_ref() else {
				return self
					.complete_recovery_command_error(
						lease,
						AccountLifecycleError::InvalidOperation,
						build_response.take().expect("builder is retained"),
					)
					.await;
			};
			match self.credentials.delete(&operation.account_id, expected) {
				Ok(()) | Err(CredentialStoreError::NotFound) => {
					accepted_phase(
						self.store
							.advance_account_operation(
								operation_id,
								operation.phase,
								AccountOperationPhase::StoreApplied,
								None,
							)
							.await?,
					)?;
					return self
						.complete_recovery_operation_command(
							lease,
							operation_id,
							AccountOperationPhase::StoreApplied,
							AccountOperationPhase::Committed,
							None,
							AccountManualRecoveryOutcome::Committed,
							build_response.take().expect("builder is retained"),
						)
						.await;
				},
				Err(_) => {},
			}
		}
		if matches!(
			(operation.kind, operation.phase),
			(
				AccountOperationKind::Enroll | AccountOperationKind::Import,
				AccountOperationPhase::Prepared
			) | (AccountOperationKind::Refresh, AccountOperationPhase::Prepared)
		) {
			return self
				.complete_recovery_operation_command(
					lease,
					operation_id,
					operation.phase,
					AccountOperationPhase::Cancelled,
					None,
					AccountManualRecoveryOutcome::Cancelled,
					build_response.take().expect("builder is retained"),
				)
				.await;
		}
		let recovery_code = match operation.kind {
			AccountOperationKind::Enroll | AccountOperationKind::Import =>
				"credential_import_reconciliation",
			AccountOperationKind::Refresh if operation.target.is_none() =>
				PROVIDER_REFRESH_OUTCOME_UNKNOWN,
			AccountOperationKind::Refresh => "credential_refresh_reconciliation",
			AccountOperationKind::Logout => "credential_logout_reconciliation",
		};
		let target = if operation.phase == AccountOperationPhase::RecoveryRequired {
			operation.phase
		} else {
			AccountOperationPhase::RecoveryRequired
		};
		self.complete_recovery_operation_command(
			lease,
			operation_id,
			operation.phase,
			target,
			(target == AccountOperationPhase::RecoveryRequired
				&& operation.phase != AccountOperationPhase::RecoveryRequired)
				.then_some(recovery_code),
			AccountManualRecoveryOutcome::StillRequiresRecovery,
			build_response.take().expect("builder is retained"),
		)
		.await
	}

	/// Exact secret projection for process launch after registry and store agreement.
	pub async fn credential_for_launch(
		&self,
		account_id: &AccountId,
		expected_revision: i64,
		expected: &CredentialBinding,
	) -> Result<StoredCredential, AccountLifecycleError> {
		let account = self.load_account(account_id).await?;
		if account.revision != expected_revision || account.credential.as_ref() != Some(expected) {
			return Err(AccountLifecycleError::StaleAccount);
		}
		self.read_exact_for_admission(&account).await
	}

	/// Acquire one exact, account-ready process projection without exposing secret payloads.
	pub async fn process_credential(
		&self,
		account_id: &AccountId,
		expected_revision: i64,
	) -> Result<AccountProcessCredential, AccountLifecycleError> {
		let launch_guard = self.lock_for(account_id)?.lock_owned().await;
		let account = self.load_account(account_id).await?;
		if account.revision != expected_revision {
			return Err(AccountLifecycleError::StaleAccount);
		}
		let credential =
			account.credential.clone().ok_or(AccountLifecycleError::CredentialAbsent)?;
		let stored = self.read_exact_for_admission(&account).await?;
		let callback_profile = self.callback_profile().map_err(|_| {
			AccountLifecycleError::NotReady(AccountLifecycleReadiness::CallbackCapabilityUnready)
		})?;
		let binding =
			ProcessGenerationAccountBinding::new(account.revision, credential, callback_profile)
				.map_err(|_| AccountLifecycleError::InvalidOperation)?;
		Ok(AccountProcessCredential { stored, binding, launch_guard })
	}

	/// Acquire one exact credential projection for the direct provider backend API.
	///
	/// API health and usage observation are allowed to run when the optional Codex app-server
	/// callback capability is absent.  This is the boundary that prevents provider account health
	/// from inheriting an executable/protocol-version gate.
	pub(crate) async fn api_credential_for_observation(
		&self,
		account_id: &AccountId,
		minimum_validity: Duration,
	) -> Result<AccountApiCredential, AccountLifecycleError> {
		let launch_guard = self.lock_for(account_id)?.lock_owned().await;
		let mut account = self.load_account(account_id).await?;
		let mut stored = self.read_exact_for_api(&account).await?;
		let now_unix_micros = current_unix_micros()?;
		if access_token_needs_refresh(
			stored.bundle().access_token_expires_at_unix_micros(),
			now_unix_micros,
			minimum_validity,
		)? {
			let operation_id = AccountOperationId::generate()
				.map_err(|_| AccountLifecycleError::InvalidOperation)?;
			self.refresh_while_locked(
				operation_id,
				account_id,
				Some(account.revision),
				None,
				None,
				true,
			)
			.await?;
			account = self.load_account(account_id).await?;
			stored = self.read_exact_for_api(&account).await?;
			let refreshed_at_unix_micros = current_unix_micros()?;
			require_refreshed_access_token_for_observation(
				stored.bundle().access_token_expires_at_unix_micros(),
				refreshed_at_unix_micros,
				minimum_validity,
			)?;
		}
		let binding = account.credential.clone().ok_or(AccountLifecycleError::CredentialAbsent)?;
		Ok(AccountApiCredential {
			stored,
			binding,
			account_revision: account.revision,
			_launch_guard: launch_guard,
		})
	}

	fn selection_candidate(
		&self,
		account: &AccountRecord,
		now_unix_micros: i64,
	) -> Result<(u8, u8), AccountSelectionRecovery> {
		if !account.enabled {
			return Err(AccountSelectionRecovery::EnableAccount);
		}
		match account.lifecycle_readiness {
			AccountLifecycleReadiness::Ready => {},
			AccountLifecycleReadiness::CredentialAbsent => {
				return Err(AccountSelectionRecovery::EnrollCredentials);
			},
			AccountLifecycleReadiness::OperationUnsettled => {
				return Err(AccountSelectionRecovery::ResolveCredentialOperation);
			},
			AccountLifecycleReadiness::CallbackCapabilityUnready => {
				return Err(AccountSelectionRecovery::UpgradeCodex);
			},
			AccountLifecycleReadiness::ProviderMismatch => {
				return Err(AccountSelectionRecovery::RestoreProviderAgreement);
			},
			_ => return Err(AccountSelectionRecovery::RepairCredentialStore),
		}
		let five = account
			.five_hour_quota
			.current()
			.filter(|fact| fact.resets_at_unix_micros > now_unix_micros)
			.ok_or(AccountSelectionRecovery::RefreshQuota)?;
		let seven = account
			.seven_day_quota
			.current()
			.filter(|fact| fact.resets_at_unix_micros > now_unix_micros)
			.ok_or(AccountSelectionRecovery::RefreshQuota)?;
		if five.used_percent >= 100 || seven.used_percent >= 100 {
			return Err(AccountSelectionRecovery::RefreshQuota);
		}
		Ok((five.used_percent.max(seven.used_percent), five.used_percent))
	}

	async fn read_exact_for_admission(
		&self,
		account: &AccountRecord,
	) -> Result<StoredCredential, AccountLifecycleError> {
		self.read_exact_with_gate(account, true).await
	}

	async fn read_exact_for_existing_work(
		&self,
		account: &AccountRecord,
	) -> Result<StoredCredential, AccountLifecycleError> {
		self.read_exact_with_gate(account, false).await
	}

	async fn read_exact_for_api(
		&self,
		account: &AccountRecord,
	) -> Result<StoredCredential, AccountLifecycleError> {
		if account.tombstoned {
			return Err(AccountLifecycleError::NotReady(AccountLifecycleReadiness::Tombstoned));
		}
		if account.unsettled_operation.is_some() {
			return Err(AccountLifecycleError::NotReady(
				AccountLifecycleReadiness::OperationUnsettled,
			));
		}
		let binding = account
			.credential
			.as_ref()
			.ok_or(AccountLifecycleError::NotReady(AccountLifecycleReadiness::CredentialAbsent))?;
		match self.credentials.read_exact(&account.account_id, binding) {
			Ok(stored) => {
				self.store
					.observe_account_store(
						&account.account_id,
						account.revision,
						binding,
						AccountStoreObservation::Exact,
					)
					.await?;
				Ok(stored)
			},
			Err(error) => {
				let (observation, readiness) = store_error_observation(error);
				self.store
					.observe_account_store(
						&account.account_id,
						account.revision,
						binding,
						observation,
					)
					.await?;
				Err(AccountLifecycleError::NotReady(readiness))
			},
		}
	}

	async fn read_exact_for_bound_callback(
		&self,
		account: &AccountRecord,
	) -> Result<StoredCredential, AccountLifecycleError> {
		if account.tombstoned {
			return Err(AccountLifecycleError::NotReady(AccountLifecycleReadiness::Tombstoned));
		}
		let binding = account.credential.as_ref().ok_or(AccountLifecycleError::CredentialAbsent)?;
		match self.credentials.read_exact(&account.account_id, binding) {
			Ok(stored) => {
				self.store
					.observe_account_store(
						&account.account_id,
						account.revision,
						binding,
						AccountStoreObservation::Exact,
					)
					.await?;
				Ok(stored)
			},
			Err(error) => {
				let (observation, readiness) = store_error_observation(error);
				self.store
					.observe_account_store(
						&account.account_id,
						account.revision,
						binding,
						observation,
					)
					.await?;
				Err(AccountLifecycleError::NotReady(readiness))
			},
		}
	}

	async fn read_exact_with_gate(
		&self,
		account: &AccountRecord,
		require_enabled: bool,
	) -> Result<StoredCredential, AccountLifecycleError> {
		if account.tombstoned {
			return Err(AccountLifecycleError::NotReady(AccountLifecycleReadiness::Tombstoned));
		}
		if require_enabled && !account.enabled {
			return Err(AccountLifecycleError::AccountDisabled);
		}
		if account.unsettled_operation.is_some() {
			return Err(AccountLifecycleError::NotReady(
				AccountLifecycleReadiness::OperationUnsettled,
			));
		}
		if !self.callback_ready.load(Ordering::Acquire) {
			return Err(AccountLifecycleError::NotReady(
				AccountLifecycleReadiness::CallbackCapabilityUnready,
			));
		}
		let binding = account
			.credential
			.as_ref()
			.ok_or(AccountLifecycleError::NotReady(AccountLifecycleReadiness::CredentialAbsent))?;
		match self.credentials.read_exact(&account.account_id, binding) {
			Ok(stored) => {
				self.store
					.observe_account_store(
						&account.account_id,
						account.revision,
						binding,
						AccountStoreObservation::Exact,
					)
					.await?;
				Ok(stored)
			},
			Err(error) => {
				let (observation, readiness) = store_error_observation(error);
				self.store
					.observe_account_store(
						&account.account_id,
						account.revision,
						binding,
						observation,
					)
					.await?;
				Err(AccountLifecycleError::NotReady(readiness))
			},
		}
	}

	async fn observe_exact_store(
		&self,
		account: &AccountRecord,
		binding: &CredentialBinding,
	) -> Result<(), AccountLifecycleError> {
		let observation = match self.credentials.read_exact(&account.account_id, binding) {
			Ok(_) => AccountStoreObservation::Exact,
			Err(error) => store_error_observation(error).0,
		};
		self.store
			.observe_account_store(&account.account_id, account.revision, binding, observation)
			.await?;
		Ok(())
	}

	async fn recover_from_exact_store(
		&self,
		operation: &AccountOperation,
	) -> Result<AccountManualRecoveryOutcome, AccountLifecycleError> {
		if operation.phase != AccountOperationPhase::RecoveryRequired {
			return Ok(match self.reconcile_operation(operation).await? {
				ReconciliationDisposition::Committed => AccountManualRecoveryOutcome::Committed,
				ReconciliationDisposition::Cancelled => AccountManualRecoveryOutcome::Cancelled,
				ReconciliationDisposition::Manual =>
					AccountManualRecoveryOutcome::StillRequiresRecovery,
			});
		}
		let proven_applied = match operation.kind {
			AccountOperationKind::Enroll
			| AccountOperationKind::Import
			| AccountOperationKind::Refresh => operation.target.as_ref().is_some_and(|target| {
				self.credentials.read_exact(&operation.account_id, target).is_ok()
			}),
			AccountOperationKind::Logout => operation.expected.as_ref().is_some_and(|expected| {
				matches!(
					self.credentials.read_exact(&operation.account_id, expected),
					Err(CredentialStoreError::NotFound)
				)
			}),
		};
		if !proven_applied {
			return Ok(AccountManualRecoveryOutcome::StillRequiresRecovery);
		}
		accepted_phase(
			self.store
				.advance_account_operation(
					&operation.operation_id,
					AccountOperationPhase::RecoveryRequired,
					AccountOperationPhase::StoreApplied,
					None,
				)
				.await?,
		)?;
		self.commit_store_applied(&operation.operation_id).await?;
		Ok(AccountManualRecoveryOutcome::Committed)
	}

	fn require_operation_identity(
		&self,
		operation: &AccountOperation,
		account_id: &AccountId,
		kind: AccountOperationKind,
	) -> Result<(), AccountLifecycleError> {
		if operation.account_id != *account_id || operation.kind != kind {
			return Err(AccountLifecycleError::InvalidOperation);
		}
		Ok(())
	}

	async fn cancel_proven_before_effect(
		&self,
		operation: &AccountOperation,
	) -> Result<AccountManualRecoveryOutcome, AccountLifecycleError> {
		let proven = match (operation.phase, operation.kind) {
			(
				AccountOperationPhase::Prepared,
				AccountOperationKind::Enroll | AccountOperationKind::Import,
			) => operation.target.as_ref().is_some_and(|target| {
				matches!(
					self.credentials.read_exact(&operation.account_id, target),
					Err(CredentialStoreError::NotFound)
				)
			}),
			(
				AccountOperationPhase::Prepared,
				AccountOperationKind::Refresh | AccountOperationKind::Logout,
			) => operation.expected.as_ref().is_some_and(|expected| {
				self.credentials.read_exact(&operation.account_id, expected).is_ok()
			}),
			(
				AccountOperationPhase::RecoveryRequired,
				AccountOperationKind::Enroll | AccountOperationKind::Import,
			) => operation.target.as_ref().is_some_and(|target| {
				matches!(
					self.credentials.read_exact(&operation.account_id, target),
					Err(CredentialStoreError::NotFound)
				)
			}),
			(AccountOperationPhase::RecoveryRequired, AccountOperationKind::Logout) =>
				operation.expected.as_ref().is_some_and(|expected| {
					self.credentials.read_exact(&operation.account_id, expected).is_ok()
				}),
			_ => false,
		};
		if !proven {
			return Ok(AccountManualRecoveryOutcome::StillRequiresRecovery);
		}
		accepted_phase(
			self.store
				.advance_account_operation(
					&operation.operation_id,
					operation.phase,
					AccountOperationPhase::Cancelled,
					None,
				)
				.await?,
		)?;
		Ok(AccountManualRecoveryOutcome::Cancelled)
	}

	async fn reconcile_operation(
		&self,
		operation: &AccountOperation,
	) -> Result<ReconciliationDisposition, AccountLifecycleError> {
		if let Some(disposition) = self.reconcile_terminal_phase(operation).await? {
			return Ok(disposition);
		}
		match operation.kind {
			AccountOperationKind::Enroll | AccountOperationKind::Import =>
				self.reconcile_import_operation(operation).await,
			AccountOperationKind::Refresh => self.reconcile_refresh_operation(operation).await,
			AccountOperationKind::Logout => self.reconcile_logout_operation(operation).await,
		}
	}

	async fn reconcile_terminal_phase(
		&self,
		operation: &AccountOperation,
	) -> Result<Option<ReconciliationDisposition>, AccountLifecycleError> {
		match operation.phase {
			AccountOperationPhase::Committed => Ok(Some(ReconciliationDisposition::Committed)),
			AccountOperationPhase::Cancelled => Ok(Some(ReconciliationDisposition::Cancelled)),
			AccountOperationPhase::RecoveryRequired => Ok(Some(ReconciliationDisposition::Manual)),
			AccountOperationPhase::StoreApplied => {
				self.commit_store_applied(&operation.operation_id).await?;
				Ok(Some(ReconciliationDisposition::Committed))
			},
			AccountOperationPhase::Prepared | AccountOperationPhase::ProviderEffectPending =>
				Ok(None),
		}
	}

	async fn reconcile_import_operation(
		&self,
		operation: &AccountOperation,
	) -> Result<ReconciliationDisposition, AccountLifecycleError> {
		let target = operation.target.as_ref().ok_or(AccountLifecycleError::InvalidOperation)?;
		match self.credentials.read_exact(&operation.account_id, target) {
			Ok(_) => self.commit_reconciled_operation(operation).await,
			Err(CredentialStoreError::NotFound)
				if operation.phase == AccountOperationPhase::Prepared =>
				self.cancel_reconciled_operation(operation).await,
			Err(_) => self.mark_manual(operation, "credential_import_reconciliation").await,
		}
	}

	async fn reconcile_refresh_operation(
		&self,
		operation: &AccountOperation,
	) -> Result<ReconciliationDisposition, AccountLifecycleError> {
		if operation.phase == AccountOperationPhase::Prepared {
			return match classify_prepared_refresh_reconciliation(
				operation.expected.as_ref(),
				operation.target.as_ref(),
				|binding| self.credentials.read_exact(&operation.account_id, binding).map(|_| ()),
			) {
				PreparedRefreshReconciliation::StoreApplied =>
					self.commit_reconciled_operation(operation).await,
				PreparedRefreshReconciliation::NotApplied =>
					self.cancel_reconciled_operation(operation).await,
				PreparedRefreshReconciliation::RecoveryRequired =>
					self.mark_manual(operation, "credential_reauthentication_reconciliation").await,
			};
		}
		let Some(target) = operation.target.as_ref() else {
			return self.mark_manual(operation, PROVIDER_REFRESH_OUTCOME_UNKNOWN).await;
		};
		match self.credentials.read_exact(&operation.account_id, target) {
			Ok(_) => self.commit_reconciled_operation(operation).await,
			Err(_) => self.mark_manual(operation, "credential_refresh_reconciliation").await,
		}
	}

	async fn reconcile_logout_operation(
		&self,
		operation: &AccountOperation,
	) -> Result<ReconciliationDisposition, AccountLifecycleError> {
		let expected =
			operation.expected.as_ref().ok_or(AccountLifecycleError::InvalidOperation)?;
		match self.credentials.delete(&operation.account_id, expected) {
			Ok(()) | Err(CredentialStoreError::NotFound) =>
				self.commit_reconciled_operation(operation).await,
			Err(_) => self.mark_manual(operation, "credential_logout_reconciliation").await,
		}
	}

	async fn commit_reconciled_operation(
		&self,
		operation: &AccountOperation,
	) -> Result<ReconciliationDisposition, AccountLifecycleError> {
		accepted_phase(
			self.store
				.advance_account_operation(
					&operation.operation_id,
					operation.phase,
					AccountOperationPhase::StoreApplied,
					None,
				)
				.await?,
		)?;
		self.commit_store_applied(&operation.operation_id).await?;
		Ok(ReconciliationDisposition::Committed)
	}

	async fn cancel_reconciled_operation(
		&self,
		operation: &AccountOperation,
	) -> Result<ReconciliationDisposition, AccountLifecycleError> {
		accepted_phase(
			self.store
				.advance_account_operation(
					&operation.operation_id,
					operation.phase,
					AccountOperationPhase::Cancelled,
					None,
				)
				.await?,
		)?;
		Ok(ReconciliationDisposition::Cancelled)
	}

	async fn mark_manual(
		&self,
		operation: &AccountOperation,
		code: &'static str,
	) -> Result<ReconciliationDisposition, AccountLifecycleError> {
		accepted_phase(
			self.store
				.advance_account_operation(
					&operation.operation_id,
					operation.phase,
					AccountOperationPhase::RecoveryRequired,
					Some(code),
				)
				.await?,
		)?;
		Ok(ReconciliationDisposition::Manual)
	}

	async fn recover_or_cancel(
		&self,
		operation_id: &AccountOperationId,
		expected: AccountOperationPhase,
		code: &'static str,
		ambiguous: bool,
	) -> Result<(), AccountLifecycleError> {
		let target = if ambiguous {
			AccountOperationPhase::RecoveryRequired
		} else {
			AccountOperationPhase::Cancelled
		};
		accepted_phase(
			self.store
				.advance_account_operation(
					operation_id,
					expected,
					target,
					ambiguous.then_some(code),
				)
				.await?,
		)?;
		Ok(())
	}

	async fn commit_store_applied(
		&self,
		operation_id: &AccountOperationId,
	) -> Result<(), AccountLifecycleError> {
		accepted_phase(
			self.store
				.advance_account_operation(
					operation_id,
					AccountOperationPhase::StoreApplied,
					AccountOperationPhase::Committed,
					None,
				)
				.await?,
		)?;
		Ok(())
	}

	async fn load_account(
		&self,
		account_id: &AccountId,
	) -> Result<AccountRecord, AccountLifecycleError> {
		self.store
			.read_account_registry(Some(account_id), 1)
			.await?
			.into_iter()
			.next()
			.ok_or(AccountLifecycleError::AccountMissing)
	}

	fn project_exact(
		&self,
		account_id: &AccountId,
		binding: &CredentialBinding,
	) -> Result<ChatgptTokenProjection, AccountLifecycleError> {
		let stored = self.credentials.read_exact(account_id, binding)?;
		Ok(projection(binding, stored.bundle()))
	}

	fn lock_for(
		&self,
		account_id: &AccountId,
	) -> Result<Arc<AsyncMutex<()>>, AccountLifecycleError> {
		account_lock_for(&self.account_locks, account_id)
	}

	fn callback_profile(&self) -> Result<String, AccountSelectionRecovery> {
		if !self.callback_ready.load(Ordering::Acquire) {
			return Err(AccountSelectionRecovery::UpgradeCodex);
		}
		self.callback_profile_sha256
			.lock()
			.ok()
			.and_then(|profile| profile.clone())
			.ok_or(AccountSelectionRecovery::UpgradeCodex)
	}
}

fn account_lock_for(
	locks: &Mutex<HashMap<AccountId, Arc<AsyncMutex<()>>>>,
	account_id: &AccountId,
) -> Result<Arc<AsyncMutex<()>>, AccountLifecycleError> {
	let mut locks = locks.lock().map_err(|_| AccountLifecycleError::CoordinatorUnavailable)?;
	Ok(Arc::clone(locks.entry(account_id.clone()).or_insert_with(|| Arc::new(AsyncMutex::new(())))))
}

fn projection(
	binding: &CredentialBinding,
	bundle: &CredentialSecretBundle,
) -> ChatgptTokenProjection {
	ChatgptTokenProjection {
		access_token: bundle.access_token().to_owned(),
		provider_account_id: binding.provider.account_id().to_owned(),
		plan_type: bundle.plan_type().map(str::to_owned),
		binding: binding.clone(),
	}
}

fn current_unix_micros() -> Result<i64, AccountLifecycleError> {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.ok()
		.and_then(|duration| i64::try_from(duration.as_micros()).ok())
		.ok_or(AccountLifecycleError::InvalidOperation)
}

fn access_token_needs_refresh(
	expires_at_unix_micros: i64,
	now_unix_micros: i64,
	minimum_validity: Duration,
) -> Result<bool, AccountLifecycleError> {
	let minimum_validity_micros = i64::try_from(minimum_validity.as_micros())
		.map_err(|_| AccountLifecycleError::InvalidOperation)?;
	let required_until = now_unix_micros
		.checked_add(minimum_validity_micros)
		.ok_or(AccountLifecycleError::InvalidOperation)?;
	Ok(expires_at_unix_micros <= required_until)
}

fn require_refreshed_access_token_for_observation(
	expires_at_unix_micros: i64,
	now_unix_micros: i64,
	minimum_validity: Duration,
) -> Result<(), AccountLifecycleError> {
	if access_token_needs_refresh(expires_at_unix_micros, now_unix_micros, minimum_validity)? {
		return Err(AccountLifecycleError::Refresh(CredentialRefreshError::Unavailable));
	}
	Ok(())
}

fn refreshed_credential_target(
	current: &CredentialBinding,
	account_id: &AccountId,
	operation_id: &AccountOperationId,
	refreshed: &CredentialRefreshResult,
) -> Result<CredentialBinding, AccountLifecycleError> {
	if refreshed.returned_provider != current.provider {
		return Err(AccountLifecycleError::ProviderMismatch);
	}
	let version =
		current.version.successor().map_err(|_| AccountLifecycleError::InvalidOperation)?;
	refreshed
		.bundle
		.binding_for(account_id, operation_id, version, &refreshed.returned_provider)
		.map_err(Into::into)
}

fn reauthentication_current(
	account: &AccountRecord,
	expected_account_revision: i64,
) -> Result<CredentialBinding, AccountLifecycleError> {
	if account.revision != expected_account_revision {
		return Err(AccountLifecycleError::StaleAccount);
	}
	account.credential.clone().ok_or(AccountLifecycleError::CredentialAbsent)
}

fn reauthentication_target(
	current: &CredentialBinding,
	account_id: &AccountId,
	operation_id: &AccountOperationId,
	imported: &ImportedCredential,
) -> Result<CredentialBinding, AccountLifecycleError> {
	if imported.provider != current.provider {
		return Err(AccountLifecycleError::ProviderMismatch);
	}
	imported
		.bundle
		.binding_for(
			account_id,
			operation_id,
			current.version.successor().map_err(|_| AccountLifecycleError::InvalidOperation)?,
			&imported.provider,
		)
		.map_err(Into::into)
}

const fn resolve_reauthentication_store_effect(
	rotation: Result<(), CredentialStoreError>,
	target_is_exact: bool,
) -> Result<(), CredentialStoreError> {
	match rotation {
		Ok(()) => Ok(()),
		Err(_) if target_is_exact => Ok(()),
		Err(error) => Err(error),
	}
}

fn matching_shared_refresh(
	current: &CredentialBinding,
	current_bundle: &CredentialSecretBundle,
	now_unix_micros: i64,
	shared: Result<ImportedCredential, CredentialImportError>,
) -> Option<CredentialRefreshResult> {
	match shared {
		Ok(imported)
			if imported.provider == current.provider
				&& imported.bundle.access_token_expires_at_unix_micros() > now_unix_micros
				&& !same_refresh_bundle(current_bundle, &imported.bundle) =>
			Some(CredentialRefreshResult {
				returned_provider: imported.provider,
				bundle: imported.bundle,
			}),
		Ok(_) | Err(_) => None,
	}
}

fn same_refresh_bundle(first: &CredentialSecretBundle, second: &CredentialSecretBundle) -> bool {
	first.access_token() == second.access_token()
		&& first.refresh_token() == second.refresh_token()
		&& first.id_token() == second.id_token()
		&& first.plan_type() == second.plan_type()
		&& first.provider_email() == second.provider_email()
		&& first.token_type() == second.token_type()
		&& first.access_token_expires_at_unix_micros()
			== second.access_token_expires_at_unix_micros()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReauthenticationReplayDisposition {
	Complete,
	Cancel,
	Recover,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PreparedRefreshReconciliation {
	StoreApplied,
	NotApplied,
	RecoveryRequired,
}

fn classify_prepared_refresh_reconciliation<F>(
	expected: Option<&CredentialBinding>,
	target: Option<&CredentialBinding>,
	mut read_exact: F,
) -> PreparedRefreshReconciliation
where
	F: FnMut(&CredentialBinding) -> Result<(), CredentialStoreError>,
{
	// Ordinary refresh reaches ProviderEffectPending before it records a target. A target-backed
	// Prepared refresh is reauthentication and can have completed its store CAS before a crash.
	let Some(target) = target else {
		return PreparedRefreshReconciliation::NotApplied;
	};
	match read_exact(target) {
		Ok(()) => PreparedRefreshReconciliation::StoreApplied,
		Err(CredentialStoreError::Unavailable) => PreparedRefreshReconciliation::RecoveryRequired,
		Err(_) => match expected {
			Some(expected) if read_exact(expected).is_ok() =>
				PreparedRefreshReconciliation::NotApplied,
			Some(_) | None => PreparedRefreshReconciliation::RecoveryRequired,
		},
	}
}

fn classify_reauthentication_replay<F>(
	phase: AccountOperationPhase,
	expected: Option<&CredentialBinding>,
	target: Option<&CredentialBinding>,
	mut read_exact: F,
) -> ReauthenticationReplayDisposition
where
	F: FnMut(&CredentialBinding) -> Result<(), CredentialStoreError>,
{
	match phase {
		AccountOperationPhase::Committed | AccountOperationPhase::StoreApplied =>
			ReauthenticationReplayDisposition::Complete,
		AccountOperationPhase::Cancelled => ReauthenticationReplayDisposition::Cancel,
		AccountOperationPhase::Prepared => {
			match classify_prepared_refresh_reconciliation(expected, target, read_exact) {
				PreparedRefreshReconciliation::StoreApplied =>
					ReauthenticationReplayDisposition::Complete,
				PreparedRefreshReconciliation::NotApplied =>
					ReauthenticationReplayDisposition::Cancel,
				PreparedRefreshReconciliation::RecoveryRequired =>
					ReauthenticationReplayDisposition::Recover,
			}
		},
		AccountOperationPhase::ProviderEffectPending | AccountOperationPhase::RecoveryRequired =>
			if target.is_some_and(|target| read_exact(target).is_ok()) {
				ReauthenticationReplayDisposition::Complete
			} else {
				ReauthenticationReplayDisposition::Recover
			},
	}
}

fn projection_binding(
	account: &AccountRecord,
	expected_revision: i64,
) -> Result<&CredentialBinding, AccountLifecycleError> {
	if expected_revision <= 0 || account.revision != expected_revision {
		return Err(AccountLifecycleError::StaleAccount);
	}
	if account.tombstoned {
		return Err(AccountLifecycleError::NotReady(AccountLifecycleReadiness::Tombstoned));
	}
	if !account.enabled {
		return Err(AccountLifecycleError::AccountDisabled);
	}
	if account.unsettled_operation.is_some() {
		return Err(AccountLifecycleError::NotReady(AccountLifecycleReadiness::OperationUnsettled));
	}
	if account.lifecycle_readiness != AccountLifecycleReadiness::Ready {
		return Err(AccountLifecycleError::NotReady(account.lifecycle_readiness));
	}
	account.credential.as_ref().ok_or(AccountLifecycleError::CredentialAbsent)
}

enum UseInCodexProjectionError {
	Rejected(AccountLifecycleError),
	OutcomeUnknown,
}

fn use_in_codex_receipt_result<T>(
	result: Result<T, UseInCodexProjectionError>,
) -> Result<Result<T, AccountLifecycleError>, AccountLifecycleError> {
	match result {
		Ok(value) => Ok(Ok(value)),
		Err(UseInCodexProjectionError::Rejected(error)) => Ok(Err(error)),
		Err(UseInCodexProjectionError::OutcomeUnknown) =>
			Err(AccountLifecycleError::CoordinatorUnavailable),
	}
}

const fn projection_error(error: CodexAuthProjectionError) -> AccountLifecycleError {
	match error {
		CodexAuthProjectionError::UnsafePath | CodexAuthProjectionError::InvalidCredential =>
			AccountLifecycleError::CredentialImport,
		CodexAuthProjectionError::MissingIdentityToken => AccountLifecycleError::CredentialAbsent,
		CodexAuthProjectionError::Unavailable | CodexAuthProjectionError::OutcomeUnknown =>
			AccountLifecycleError::CoordinatorUnavailable,
	}
}

fn accepted_phase(
	outcome: AccountLifecycleMutationOutcome,
) -> Result<AccountOperationPhase, AccountLifecycleError> {
	match outcome {
		AccountLifecycleMutationOutcome::Applied(mutation)
		| AccountLifecycleMutationOutcome::Replayed(mutation) => Ok(mutation.phase),
		AccountLifecycleMutationOutcome::Rejected { rejection, .. } =>
			Err(AccountLifecycleError::OperationRejected(rejection)),
	}
}

const fn store_effect_may_be_ambiguous(error: CredentialStoreError) -> bool {
	matches!(error, CredentialStoreError::Unavailable)
}

const fn store_error_observation(
	error: CredentialStoreError,
) -> (AccountStoreObservation, AccountLifecycleReadiness) {
	match error {
		CredentialStoreError::Unavailable =>
			(AccountStoreObservation::Unavailable, AccountLifecycleReadiness::StoreUnavailable),
		CredentialStoreError::ProviderMismatch =>
			(AccountStoreObservation::ProviderMismatch, AccountLifecycleReadiness::ProviderMismatch),
		CredentialStoreError::NotFound =>
			(AccountStoreObservation::Missing, AccountLifecycleReadiness::StoreMismatch),
		_ => (AccountStoreObservation::Mismatch, AccountLifecycleReadiness::StoreMismatch),
	}
}

enum ReconciliationDisposition {
	Committed,
	Cancelled,
	Manual,
}

/// Closed Account Service failure. No variant contains credential material.
#[derive(Debug)]
pub enum AccountLifecycleError {
	/// Durable account authority failed.
	Persistence(StoreError),
	/// The exact host credential store operation failed.
	CredentialStore(CredentialStoreError),
	/// The provider refresh adapter failed.
	Refresh(CredentialRefreshError),
	/// The product store rejected a finite lifecycle transition.
	OperationRejected(decodex_database::AccountLifecycleRejection),
	/// A derived lifecycle gate is not ready.
	NotReady(AccountLifecycleReadiness),
	/// Administrative disablement blocks new work.
	AccountDisabled,
	/// The requested account does not exist.
	AccountMissing,
	/// The account has no current credential binding.
	CredentialAbsent,
	/// Provider identities do not agree.
	ProviderMismatch,
	/// Account revision or credential binding changed.
	StaleAccount,
	/// The requested operation conflicts with its finite state machine.
	InvalidOperation,
	/// The in-process single-writer coordinator is unavailable.
	CoordinatorUnavailable,
	/// Credential import input is unsafe or malformed.
	CredentialImport,
}
impl Error for AccountLifecycleError {}
impl Display for AccountLifecycleError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::Persistence(_) => "account persistence unavailable",
			Self::CredentialStore(_) => "host credential store operation failed",
			Self::Refresh(_) => "provider credential refresh failed",
			Self::OperationRejected(_) => "account lifecycle operation rejected",
			Self::NotReady(_) => "account lifecycle is not ready",
			Self::AccountDisabled => "account is administratively disabled",
			Self::AccountMissing => "account not found",
			Self::CredentialAbsent => "account credential binding absent",
			Self::ProviderMismatch => "account provider mismatch",
			Self::StaleAccount => "account binding changed",
			Self::InvalidOperation => "account lifecycle operation invalid",
			Self::CoordinatorUnavailable => "account lifecycle coordinator unavailable",
			Self::CredentialImport => "account credential source unavailable",
		})
	}
}
impl From<StoreError> for AccountLifecycleError {
	fn from(value: StoreError) -> Self {
		Self::Persistence(value)
	}
}
impl From<CredentialStoreError> for AccountLifecycleError {
	fn from(value: CredentialStoreError) -> Self {
		Self::CredentialStore(value)
	}
}
impl From<CredentialImportError> for AccountLifecycleError {
	fn from(_value: CredentialImportError) -> Self {
		Self::CredentialImport
	}
}

#[cfg(test)]
mod tests {
	use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
	use decodex_core::{
		AccountId, AccountLifecycleReadiness, AccountOperationId, AccountOperationPhase,
		AccountProvider, AccountQuotaWindow, AccountQuotaWindowObservation, AccountRecord,
		AccountState, CredentialBinding, CredentialFingerprint, CredentialStoreSchemaVersion,
		CredentialVersion, ProviderIdentity,
	};
	use serde_json::json;

	use super::{
		ACCOUNT_ALIAS_WORDS, AccountLifecycleError, CredentialImportError, CredentialRefreshError,
		CredentialSecretBundle, ImportedCredential, PROVIDER_REFRESH_OUTCOME_UNKNOWN,
		PreparedRefreshReconciliation, ReauthenticationReplayDisposition, RefreshResponse,
		UseInCodexProjectionError, access_token_needs_refresh, account_lock_for,
		classify_prepared_refresh_reconciliation, classify_reauthentication_replay,
		codex_auth_projection_digest, credential_refresh_result, matching_shared_refresh,
		projection_binding, reauthentication_current, reauthentication_target,
		refreshed_credential_target, require_refreshed_access_token_for_observation,
		resolve_reauthentication_store_effect, stable_account_alias, use_in_codex_receipt_result,
	};
	use std::{
		collections::{HashMap, HashSet},
		sync::{Arc, Mutex},
		time::Duration,
	};
	use tokio::time;

	const OBSERVED_AT_MICROS: i64 = 1_000_000;

	#[test]
	fn observation_refreshes_only_when_the_access_token_cannot_cover_the_process_deadline() {
		let now = 1_000_000_i64;
		let minimum_validity = Duration::from_micros(500);

		assert!(access_token_needs_refresh(now - 1, now, minimum_validity).unwrap());
		assert!(access_token_needs_refresh(now + 500, now, minimum_validity).unwrap());
		assert!(!access_token_needs_refresh(now + 501, now, minimum_validity).unwrap());
		assert!(matches!(
			access_token_needs_refresh(i64::MAX, i64::MAX, minimum_validity),
			Err(AccountLifecycleError::InvalidOperation)
		));
		assert!(matches!(
			require_refreshed_access_token_for_observation(now + 500, now, minimum_validity),
			Err(AccountLifecycleError::Refresh(CredentialRefreshError::Unavailable))
		));
		assert!(
			require_refreshed_access_token_for_observation(now + 501, now, minimum_validity)
				.is_ok()
		);
	}

	#[test]
	fn stable_alias_uses_the_canonical_provider_binding_vector() {
		let provider =
			ProviderIdentity::new(AccountProvider::Chatgpt, "433463f7-74ae-4a7e-ab10-9667f9e4919e")
				.unwrap();

		assert_eq!(stable_account_alias(&provider), "Val");
	}

	#[test]
	fn stable_alias_word_table_is_closed_unique_and_canonical() {
		assert_eq!(ACCOUNT_ALIAS_WORDS.len(), 44);
		assert_eq!(ACCOUNT_ALIAS_WORDS.iter().copied().collect::<HashSet<_>>().len(), 44);
		assert!(ACCOUNT_ALIAS_WORDS.iter().all(|word| {
			let bytes = word.as_bytes();
			(2..=16).contains(&bytes.len())
				&& bytes[0].is_ascii_uppercase()
				&& bytes[1..].iter().all(u8::is_ascii_lowercase)
		}));
	}

	#[test]
	fn exact_reauthentication_builds_only_the_immediate_same_provider_successor() {
		let account_id = AccountId::new("20000000-0000-4000-8000-000000000097").unwrap();
		let operation_id = AccountOperationId::new("10000000-0000-4000-8000-000000000097").unwrap();
		let current = binding("reauth-provider", 7);
		let imported = imported("reauth-provider", "new-access", 3_000_000);

		let target =
			reauthentication_target(&current, &account_id, &operation_id, &imported).unwrap();

		assert_eq!(target.version.get(), 8);
		assert_eq!(target.provider, current.provider);
		assert_eq!(target.writer_operation_id, operation_id);
		assert_ne!(target.fingerprint, current.fingerprint);
		assert_eq!(resolve_reauthentication_store_effect(Ok(()), false), Ok(()));
		assert_eq!(
			resolve_reauthentication_store_effect(
				Err(super::CredentialStoreError::VersionConflict),
				true
			),
			Ok(())
		);
	}

	#[test]
	fn provider_mismatch_and_stale_revision_stop_before_a_reauthentication_target_exists() {
		let account_id = AccountId::new("20000000-0000-4000-8000-000000000096").unwrap();
		let operation_id = AccountOperationId::new("10000000-0000-4000-8000-000000000096").unwrap();
		let current = binding("expected-provider", 3);
		let mismatched = imported("other-provider", "new-access", 3_000_000);
		assert!(matches!(
			reauthentication_target(&current, &account_id, &operation_id, &mismatched),
			Err(AccountLifecycleError::ProviderMismatch)
		));

		let account = projection_account(Some(current.clone()));
		assert!(matches!(
			reauthentication_current(&account, account.revision + 1),
			Err(AccountLifecycleError::StaleAccount)
		));
		assert_eq!(account.credential.as_ref(), Some(&current));
	}

	#[test]
	fn same_operation_replay_after_source_cleanup_uses_only_persisted_target_evidence() {
		let expected = binding("reauth-provider", 7);
		let target = binding("reauth-provider", 8);
		for phase in [
			AccountOperationPhase::Prepared,
			AccountOperationPhase::ProviderEffectPending,
			AccountOperationPhase::RecoveryRequired,
			AccountOperationPhase::StoreApplied,
			AccountOperationPhase::Committed,
		] {
			assert_eq!(
				classify_reauthentication_replay(
					phase,
					Some(&expected),
					Some(&target),
					|binding| {
						assert_eq!(binding, &target);
						Ok(())
					},
				),
				ReauthenticationReplayDisposition::Complete
			);
		}
		assert_eq!(
			classify_reauthentication_replay(
				AccountOperationPhase::Prepared,
				Some(&expected),
				Some(&target),
				|binding| {
					if binding == &expected {
						Ok(())
					} else {
						Err(super::CredentialStoreError::NotFound)
					}
				},
			),
			ReauthenticationReplayDisposition::Cancel
		);
		assert_eq!(
			classify_reauthentication_replay(
				AccountOperationPhase::Cancelled,
				Some(&expected),
				Some(&target),
				|_| panic!("cancelled replay does not read credentials"),
			),
			ReauthenticationReplayDisposition::Cancel
		);
		assert_eq!(
			classify_reauthentication_replay(
				AccountOperationPhase::RecoveryRequired,
				Some(&expected),
				Some(&target),
				|_| Err(super::CredentialStoreError::Unavailable),
			),
			ReauthenticationReplayDisposition::Recover
		);
		assert_eq!(
			classify_reauthentication_replay(
				AccountOperationPhase::Prepared,
				Some(&expected),
				Some(&target),
				|_| Err(super::CredentialStoreError::Unavailable),
			),
			ReauthenticationReplayDisposition::Recover
		);
	}

	#[test]
	fn prepared_target_backed_refresh_startup_commits_an_exact_applied_target() {
		let expected = binding("reauth-provider", 7);
		let target = binding("reauth-provider", 8);
		let mut reads = 0_u8;

		let disposition =
			classify_prepared_refresh_reconciliation(Some(&expected), Some(&target), |binding| {
				reads += 1;
				assert_eq!(binding, &target);
				Ok(())
			});

		assert_eq!(disposition, PreparedRefreshReconciliation::StoreApplied);
		assert_eq!(reads, 1);
	}

	#[test]
	fn prepared_target_backed_refresh_startup_cancels_when_the_target_was_not_applied() {
		let expected = binding("reauth-provider", 7);
		let target = binding("reauth-provider", 8);
		let mut reads = Vec::new();

		let exact_expected =
			classify_prepared_refresh_reconciliation(Some(&expected), Some(&target), |binding| {
				reads.push(binding.version.get());
				if binding == &target {
					Err(super::CredentialStoreError::VersionConflict)
				} else {
					Ok(())
				}
			});
		assert_eq!(exact_expected, PreparedRefreshReconciliation::NotApplied);
		assert_eq!(reads, vec![8, 7]);

		let absent =
			classify_prepared_refresh_reconciliation(Some(&expected), Some(&target), |_| {
				Err(super::CredentialStoreError::NotFound)
			});
		assert_eq!(absent, PreparedRefreshReconciliation::RecoveryRequired);
	}

	#[test]
	fn prepared_target_backed_refresh_startup_requires_recovery_for_ambiguous_or_unavailable_store()
	{
		let expected = binding("reauth-provider", 7);
		let target = binding("reauth-provider", 8);

		let ambiguous =
			classify_prepared_refresh_reconciliation(Some(&expected), Some(&target), |binding| {
				if binding == &target {
					Err(super::CredentialStoreError::VersionConflict)
				} else {
					Err(super::CredentialStoreError::FingerprintMismatch)
				}
			});
		assert_eq!(ambiguous, PreparedRefreshReconciliation::RecoveryRequired);

		let unavailable =
			classify_prepared_refresh_reconciliation(Some(&expected), Some(&target), |_| {
				Err(super::CredentialStoreError::Unavailable)
			});
		assert_eq!(unavailable, PreparedRefreshReconciliation::RecoveryRequired);
	}

	#[test]
	fn ordinary_prepared_refresh_startup_still_cancels_without_reading_the_store() {
		let expected = binding("refresh-provider", 7);

		let disposition = classify_prepared_refresh_reconciliation(Some(&expected), None, |_| {
			panic!("ordinary refresh Prepared has no store effect to inspect")
		});

		assert_eq!(disposition, PreparedRefreshReconciliation::NotApplied);
	}

	#[tokio::test]
	async fn one_account_lock_serializes_projection_and_revision_writers() {
		let locks = Mutex::new(HashMap::new());
		let account_id = AccountId::new("20000000-0000-4000-8000-000000000098").unwrap();
		let first = account_lock_for(&locks, &account_id).unwrap();
		let second = account_lock_for(&locks, &account_id).unwrap();
		assert!(Arc::ptr_eq(&first, &second));
		let held = first.lock().await;

		assert!(time::timeout(Duration::from_millis(10), second.lock()).await.is_err());
		drop(held);
		assert!(time::timeout(Duration::from_secs(1), second.lock()).await.is_ok());
	}

	#[test]
	fn post_rename_unknown_escapes_before_receipt_completion() {
		assert!(matches!(
			use_in_codex_receipt_result::<()>(Err(UseInCodexProjectionError::OutcomeUnknown,)),
			Err(AccountLifecycleError::CoordinatorUnavailable),
		));
		assert!(matches!(
			use_in_codex_receipt_result::<()>(Err(UseInCodexProjectionError::Rejected(
				AccountLifecycleError::CredentialAbsent,
			))),
			Ok(Err(AccountLifecycleError::CredentialAbsent)),
		));
	}

	#[test]
	fn codex_projection_digest_changes_with_only_credential_negative_binding_state() {
		let first = projection_account(Some(binding("projection-provider", 3)));
		let first_binding = first.credential.as_ref().unwrap();
		let first_digest = codex_auth_projection_digest(&first, first_binding);
		let mut revised = first.clone();
		revised.revision += 1;
		let revised_digest =
			codex_auth_projection_digest(&revised, revised.credential.as_ref().unwrap());
		let versioned = projection_account(Some(binding("projection-provider", 4)));
		let versioned_digest =
			codex_auth_projection_digest(&versioned, versioned.credential.as_ref().unwrap());

		assert_eq!(first_digest.len(), 64);
		assert_ne!(first_digest, revised_digest);
		assert_ne!(first_digest, versioned_digest);
	}

	fn identity_token(account_id: &str, email: &str, plan_type: &str) -> String {
		let claims = json!({
			"email": email,
			"https://api.openai.com/auth": {
				"chatgpt_account_id": account_id,
				"chatgpt_plan_type": plan_type,
			},
		});
		let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
		format!("header.{payload}.signature")
	}

	fn current_bundle() -> CredentialSecretBundle {
		CredentialSecretBundle::chatgpt(
			"old-access".to_owned(),
			"old-refresh".to_owned(),
			Some(identity_token("old-provider-account", "old@example.test", "free")),
			Some("free".to_owned()),
			"old@example.test".to_owned(),
			"bearer".to_owned(),
			2_000_000,
		)
		.unwrap()
	}

	fn response(id_token: Option<String>) -> RefreshResponse {
		RefreshResponse {
			id_token,
			access_token: Some("fresh-access".to_owned()),
			refresh_token: None,
			token_type: Some("bearer".to_owned()),
			expires_in: Some(60),
		}
	}

	fn binding(provider_account_id: &str, version: u64) -> CredentialBinding {
		CredentialBinding {
			schema_version: CredentialStoreSchemaVersion::V1,
			version: CredentialVersion::new(version).unwrap(),
			fingerprint: CredentialFingerprint::new("0".repeat(64)).unwrap(),
			provider: ProviderIdentity::new(AccountProvider::Chatgpt, provider_account_id).unwrap(),
			writer_operation_id: AccountOperationId::new("10000000-0000-4000-8000-000000000001")
				.unwrap(),
		}
	}

	fn shared_bundle(
		provider_account_id: &str,
		access_token: &str,
		expires_at_unix_micros: i64,
	) -> CredentialSecretBundle {
		CredentialSecretBundle::chatgpt(
			access_token.to_owned(),
			"shared-refresh".to_owned(),
			Some(identity_token(provider_account_id, "shared@example.test", "pro")),
			Some("pro".to_owned()),
			"shared@example.test".to_owned(),
			"bearer".to_owned(),
			expires_at_unix_micros,
		)
		.unwrap()
	}

	fn imported(
		provider_account_id: &str,
		access_token: &str,
		expires_at_unix_micros: i64,
	) -> ImportedCredential {
		ImportedCredential {
			provider: ProviderIdentity::new(AccountProvider::Chatgpt, provider_account_id).unwrap(),
			bundle: shared_bundle(provider_account_id, access_token, expires_at_unix_micros),
		}
	}

	fn projection_account(credential: Option<CredentialBinding>) -> AccountRecord {
		AccountRecord {
			account_id: AccountId::new("20000000-0000-4000-8000-000000000099").unwrap(),
			label: "Projection".to_owned(),
			enabled: true,
			revision: 9,
			observed_state: AccountState::Available,
			lifecycle_readiness: AccountLifecycleReadiness::Ready,
			credential,
			unsettled_operation: None,
			five_hour_quota: AccountQuotaWindowObservation::unknown(
				AccountQuotaWindow::FIVE_HOURS_MINUTES,
			)
			.unwrap(),
			seven_day_quota: AccountQuotaWindowObservation::unknown(
				AccountQuotaWindow::SEVEN_DAYS_MINUTES,
			)
			.unwrap(),
			tombstoned: false,
		}
	}

	#[test]
	fn codex_projection_requires_exact_ready_revision_and_credential() {
		let exact = projection_account(Some(binding("projection-provider", 3)));

		assert_eq!(
			projection_binding(&exact, 9).unwrap().provider.account_id(),
			"projection-provider",
		);
		assert!(matches!(projection_binding(&exact, 8), Err(AccountLifecycleError::StaleAccount)));
		assert!(matches!(
			projection_binding(&projection_account(None), 9),
			Err(AccountLifecycleError::CredentialAbsent)
		));

		let mut disabled = exact.clone();
		disabled.enabled = false;
		assert!(matches!(
			projection_binding(&disabled, 9),
			Err(AccountLifecycleError::AccountDisabled)
		));

		let mut tombstoned = exact;
		tombstoned.tombstoned = true;
		tombstoned.lifecycle_readiness = AccountLifecycleReadiness::Tombstoned;
		assert!(matches!(
			projection_binding(&tombstoned, 9),
			Err(AccountLifecycleError::NotReady(AccountLifecycleReadiness::Tombstoned))
		));
	}

	#[test]
	fn same_provider_shared_credential_is_selected_with_exact_successor_binding() {
		let current = binding("shared-provider-account", 7);
		let selected = matching_shared_refresh(
			&current,
			&current_bundle(),
			OBSERVED_AT_MICROS,
			Ok(imported("shared-provider-account", "shared-access", 3_000_000)),
		)
		.expect("same-provider shared credential must be selected");

		assert_eq!(selected.returned_provider, current.provider);
		assert_eq!(selected.bundle.access_token(), "shared-access");
		let account_id = AccountId::new("20000000-0000-4000-8000-000000000010").unwrap();
		let operation_id = AccountOperationId::new("30000000-0000-4000-8000-000000000010").unwrap();
		let target =
			refreshed_credential_target(&current, &account_id, &operation_id, &selected).unwrap();
		assert_eq!(target.provider, current.provider);
		assert_eq!(target.version.get(), 8);
		assert_eq!(target.writer_operation_id, operation_id);
	}

	#[test]
	fn mismatched_shared_provider_preserves_provider_refresh_fallback() {
		let current = binding("expected-provider-account", 7);

		assert!(
			matching_shared_refresh(
				&current,
				&current_bundle(),
				OBSERVED_AT_MICROS,
				Ok(imported("different-provider-account", "different-access", 3_000_000)),
			)
			.is_none()
		);
	}

	#[test]
	fn unavailable_shared_credential_preserves_provider_refresh_fallback() {
		let current = binding("expected-provider-account", 7);

		assert!(
			matching_shared_refresh(
				&current,
				&current_bundle(),
				OBSERVED_AT_MICROS,
				Err(CredentialImportError::Unavailable),
			)
			.is_none()
		);
	}

	#[test]
	fn unchanged_or_expired_shared_credential_preserves_provider_refresh_fallback() {
		let provider_account_id = "expected-provider-account";
		let current = binding(provider_account_id, 7);
		let current_bundle = shared_bundle(provider_account_id, "same-access", 3_000_000);

		assert!(
			matching_shared_refresh(
				&current,
				&current_bundle,
				OBSERVED_AT_MICROS,
				Ok(imported(provider_account_id, "same-access", 3_000_000)),
			)
			.is_none()
		);
		assert!(
			matching_shared_refresh(
				&current,
				&current_bundle,
				OBSERVED_AT_MICROS,
				Ok(imported(provider_account_id, "different-access", OBSERVED_AT_MICROS)),
			)
			.is_none()
		);
	}

	#[test]
	fn fresh_refresh_identity_and_target_come_only_from_returned_id_token() {
		let current = current_bundle();
		let fresh_id_token = identity_token("fresh-provider-account", "fresh@example.test", "pro");
		let refreshed = credential_refresh_result(
			&current,
			response(Some(fresh_id_token.clone())),
			OBSERVED_AT_MICROS,
		)
		.unwrap();

		assert_eq!(refreshed.returned_provider.account_id(), "fresh-provider-account");
		assert_eq!(refreshed.bundle.id_token(), Some(fresh_id_token.as_str()));
		assert_eq!(refreshed.bundle.provider_email(), "fresh@example.test");
		assert_eq!(refreshed.bundle.plan_type(), Some("pro"));
		assert_eq!(refreshed.bundle.refresh_token(), "old-refresh");

		let account_id = AccountId::new("20000000-0000-4000-8000-000000000001").unwrap();
		let operation_id = AccountOperationId::new("30000000-0000-4000-8000-000000000001").unwrap();
		let target = refreshed_credential_target(
			&binding("fresh-provider-account", 7),
			&account_id,
			&operation_id,
			&refreshed,
		)
		.unwrap();
		assert_eq!(target.version.get(), 8);
		assert_eq!(target.provider, refreshed.returned_provider);
		assert_eq!(target.writer_operation_id, operation_id);
	}

	#[test]
	fn missing_empty_or_malformed_fresh_id_token_is_ambiguous_without_fallback() {
		let malformed_claims = {
			let payload = URL_SAFE_NO_PAD.encode(
				serde_json::to_vec(&json!({
					"email": "fresh@example.test",
					"https://api.openai.com/auth": {},
				}))
				.unwrap(),
			);
			format!("header.{payload}.signature")
		};
		for id_token in
			[None, Some(String::new()), Some("not-a-jwt".to_owned()), Some(malformed_claims)]
		{
			assert!(matches!(
				credential_refresh_result(
					&current_bundle(),
					response(id_token),
					OBSERVED_AT_MICROS,
				),
				Err(CredentialRefreshError::Ambiguous)
			));
		}
	}

	#[test]
	fn returned_provider_mismatch_precedes_target_construction_and_uses_fixed_recovery_code() {
		let refreshed = credential_refresh_result(
			&current_bundle(),
			response(Some(identity_token("fresh-provider-account", "fresh@example.test", "pro"))),
			OBSERVED_AT_MICROS,
		)
		.unwrap();
		let account_id = AccountId::new("20000000-0000-4000-8000-000000000002").unwrap();
		let operation_id = AccountOperationId::new("30000000-0000-4000-8000-000000000002").unwrap();

		assert!(matches!(
			refreshed_credential_target(
				&binding("old-provider-account", u64::MAX),
				&account_id,
				&operation_id,
				&refreshed,
			),
			Err(AccountLifecycleError::ProviderMismatch)
		));
		assert_eq!(PROVIDER_REFRESH_OUTCOME_UNKNOWN, "provider_refresh_outcome_unknown");
	}
}
