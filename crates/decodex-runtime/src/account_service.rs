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
	AccountRoutingControl, AccountSelectionMode, AccountSelectionRecovery, CredentialBinding,
	CredentialVersion, ProcessGenerationAccountBinding, ProcessGenerationId,
	ProcessGenerationState, ProviderIdentity,
};
use decodex_database::{
	AccountAdministrationOutcome, AccountCommandReceiptLease, AccountEnrollmentResolution,
	AccountLifecycleMutationOutcome, AccountOperationPreparation, AccountStoreObservation,
	CodexAccountCapabilityAttestation, RoutingControlOutcome, SqliteStore, StoreError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tokio::sync::{
	Mutex as AsyncMutex, MutexGuard as AsyncMutexGuard, Notify, OwnedMutexGuard, broadcast,
};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{
	account_import::{
		CredentialImportError, ImportedCredential, decode_chatgpt_identity, decode_expiry_micros,
		read_explicit_credential_file, read_explicit_shared_codex_credential_file,
		read_shared_codex_credential,
	},
	auth_projection::{CodexAuthProjectionError, SharedCodexAuthSnapshot, SharedCodexAuthVersion},
	host_credentials::{
		CredentialSecretBundle, CredentialStoreError, HostCredentialStore, StoredCredential,
	},
	shared_auth_coordinator::{
		CodexAuthOwnerBlocker, CodexLiveness, CodexLivenessObservation, SharedAuthCoordinator,
		StableSharedAuthPoll, StableSharedAuthRead,
	},
};

#[cfg(not(all(feature = "process-acceptance-fixture", debug_assertions)))]
const REFRESH_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
#[cfg(all(feature = "process-acceptance-fixture", debug_assertions))]
pub(crate) const PROCESS_TEST_REFRESH_ENDPOINT_ENV: &str = "DECODEX_PROCESS_TEST_REFRESH_ENDPOINT";
const CHATGPT_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const MAX_ACCOUNT_READ: u16 = 512;
const MAX_UNSETTLED_ACCOUNT_OPERATION_READ: u16 = 1_024;
const PROVIDER_REFRESH_OUTCOME_UNKNOWN: &str = "provider_refresh_outcome_unknown";
const TOMBSTONE_ENROLLMENT_COLLISION: &str = "tombstone_enrollment_collision";
const ACCOUNT_ALIAS_DOMAIN: &[u8] = b"decodex/account-alias/v2\0";
const CODEX_AUTH_PROJECTION_DOMAIN: &[u8] = b"decodex/codex-auth-projection/v1\0";
const ROUTE_REFRESH_OPERATION_DOMAIN: &[u8] = b"decodex/route-refresh-operation/v1\0";
const SHARED_AUTH_IMPORT_OPERATION_DOMAIN: &[u8] = b"decodex/shared-auth-import-operation/v1\0";
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

fn route_refresh_operation_id(
	route_operation_id: &AccountOperationId,
	account_id: &AccountId,
) -> Result<AccountOperationId, AccountLifecycleError> {
	let digest = Sha256::new()
		.chain_update(ROUTE_REFRESH_OPERATION_DOMAIN)
		.chain_update(route_operation_id.as_str().as_bytes())
		.chain_update(b"\0")
		.chain_update(account_id.as_str().as_bytes())
		.finalize();
	operation_id_from_digest(&digest)
}

fn shared_auth_import_operation_id(
	account_id: &AccountId,
	current: &CredentialBinding,
	bundle: &CredentialSecretBundle,
) -> Result<AccountOperationId, AccountLifecycleError> {
	let digest = Sha256::new()
		.chain_update(SHARED_AUTH_IMPORT_OPERATION_DOMAIN)
		.chain_update(account_id.as_str().as_bytes())
		.chain_update(b"\0")
		.chain_update(current.fingerprint.as_str().as_bytes())
		.chain_update(b"\0")
		.chain_update(bundle.access_token().as_bytes())
		.chain_update(b"\0")
		.chain_update(bundle.refresh_token().as_bytes())
		.chain_update(b"\0")
		.chain_update(bundle.id_token().unwrap_or_default().as_bytes())
		.chain_update(b"\0")
		.chain_update(bundle.access_token_expires_at_unix_micros().to_be_bytes())
		.finalize();
	operation_id_from_digest(&digest)
}

fn operation_id_from_digest(digest: &[u8]) -> Result<AccountOperationId, AccountLifecycleError> {
	let mut bytes = [0_u8; 16];
	bytes.copy_from_slice(&digest[..16]);
	bytes[6] = (bytes[6] & 0x0f) | 0x50;
	bytes[8] = (bytes[8] & 0x3f) | 0x80;
	AccountOperationId::new(format!(
		"{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
		bytes[0],
		bytes[1],
		bytes[2],
		bytes[3],
		bytes[4],
		bytes[5],
		bytes[6],
		bytes[7],
		bytes[8],
		bytes[9],
		bytes[10],
		bytes[11],
		bytes[12],
		bytes[13],
		bytes[14],
		bytes[15],
	))
	.map_err(|_| AccountLifecycleError::InvalidOperation)
}

pub(crate) struct CredentialRefreshResult {
	returned_provider: ProviderIdentity,
	bundle: CredentialSecretBundle,
}

#[allow(clippy::large_enum_variant)] // The secret-bearing result is matched immediately and remains zeroizing in one owner.
enum RefreshResolution {
	Rotate { refreshed: CredentialRefreshResult, projected_source: Option<SharedCodexAuthVersion> },
	Current,
}

enum SharedRefreshConvergence {
	Current,
	Previous(SharedCodexAuthVersion),
	Winner(CredentialRefreshResult),
	Unrelated,
	Conflict,
}

struct RefreshPlan {
	allow_disabled: bool,
	shared_family: SharedFamilyRefreshPolicy,
	supplied_refresh: Option<CredentialRefreshResult>,
}

#[derive(Clone, Copy)]
enum SharedFamilyRefreshPolicy {
	Guard,
	ProvedInactive,
}

impl RefreshPlan {
	const ADMISSION: Self = Self {
		allow_disabled: false,
		shared_family: SharedFamilyRefreshPolicy::Guard,
		supplied_refresh: None,
	};
	const EXISTING_WORK: Self = Self {
		allow_disabled: true,
		shared_family: SharedFamilyRefreshPolicy::Guard,
		supplied_refresh: None,
	};
	const ROUTE_TARGET: Self = Self {
		allow_disabled: false,
		shared_family: SharedFamilyRefreshPolicy::ProvedInactive,
		supplied_refresh: None,
	};

	fn route_source(supplied_refresh: CredentialRefreshResult) -> Self {
		Self {
			allow_disabled: true,
			shared_family: SharedFamilyRefreshPolicy::Guard,
			supplied_refresh: Some(supplied_refresh),
		}
	}
}

/// Credential-negative readback of the normal shared Codex auth projection.
pub(crate) enum CodexAuthProjectionInspection {
	Current { account_id: AccountId, account_revision: i64, projection_digest: String },
	Unmanaged,
	Unavailable,
}

/// Complete credential-negative result of one daemon-owned Route command.
#[derive(Clone)]
pub(crate) struct AccountRouteCommit {
	pub(crate) account: AccountRecord,
	pub(crate) routing: AccountRoutingControl,
	pub(crate) projection_digest: String,
}

#[derive(Clone)]
pub(crate) struct AccountRouteCompletion {
	pub(crate) operation_id: AccountOperationId,
	pub(crate) commit: AccountRouteCommit,
}

/// Credential-negative accepted state while one exact safe-cutover prerequisite remains open.
pub(crate) struct AccountRoutePending {
	pub(crate) operation_id: AccountOperationId,
	pub(crate) account_id: AccountId,
	pub(crate) routing_revision: i64,
	pub(crate) wait_reason: AccountRouteWaitReason,
}

/// Current credential-negative reason why an accepted Route cannot finish yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AccountRouteWaitReason {
	ExternalCodex { blockers: Vec<CodexAuthOwnerBlocker>, omitted: u16 },
	CodexObservationUnavailable,
	AccountReadiness(AccountLifecycleReadiness),
	SharedAuthStabilizing,
	SharedAuthUnavailable,
	ProjectionReadback,
}

impl AccountRouteWaitReason {
	fn from_liveness(observation: CodexLivenessObservation) -> Self {
		match observation {
			CodexLivenessObservation::Blocked { blockers, omitted } =>
				Self::ExternalCodex { blockers, omitted },
			CodexLivenessObservation::Unavailable => Self::CodexObservationUnavailable,
			CodexLivenessObservation::Quiescent => Self::SharedAuthStabilizing,
		}
	}
}

pub(crate) enum AccountRouteResult {
	Pending(AccountRoutePending),
	Committed(Box<AccountRouteCommit>),
}

/// Closed failure passed to the Route publication builder before its receipt commits.
pub(crate) enum AccountRouteFailure {
	Lifecycle(AccountLifecycleError),
	Routing(RoutingControlOutcome),
}

struct RouteSharedAuthSource {
	account_id: AccountId,
	credential: ImportedCredential,
}

struct RouteSharedAuthSnapshot {
	version: SharedCodexAuthVersion,
	source: Option<RouteSharedAuthSource>,
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
		let endpoint = refresh_endpoint()?;
		let response = self
			.client
			.post(endpoint)
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

fn refresh_endpoint() -> Result<String, CredentialRefreshError> {
	#[cfg(all(feature = "process-acceptance-fixture", debug_assertions))]
	{
		return process_acceptance_fixture_endpoint().ok_or(CredentialRefreshError::Unavailable);
	}

	#[cfg(not(all(feature = "process-acceptance-fixture", debug_assertions)))]
	{
		Ok(REFRESH_ENDPOINT.to_owned())
	}
}

#[cfg(all(feature = "process-acceptance-fixture", debug_assertions))]
pub(crate) fn process_acceptance_fixture_endpoint() -> Option<String> {
	std::env::var_os(PROCESS_TEST_REFRESH_ENDPOINT_ENV)
		.and_then(|value| value.into_string().ok())
		.filter(|endpoint| process_test_refresh_endpoint_is_safe(endpoint))
}

#[cfg(all(feature = "process-acceptance-fixture", debug_assertions))]
fn process_test_refresh_endpoint_is_safe(value: &str) -> bool {
	let Ok(endpoint) = reqwest::Url::parse(value) else {
		return false;
	};
	endpoint.scheme() == "http"
		&& endpoint.host_str() == Some("127.0.0.1")
		&& endpoint.port().is_some()
		&& endpoint.path() == "/oauth/token"
		&& endpoint.query().is_none()
		&& endpoint.fragment().is_none()
		&& endpoint.username().is_empty()
		&& endpoint.password().is_none()
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
	if refreshed.expires_in.is_none_or(|expires_in| expires_in == 0) {
		return Err(CredentialRefreshError::Ambiguous);
	}
	let expires_at_micros =
		decode_expiry_micros(&access_token).map_err(|_| CredentialRefreshError::Ambiguous)?;
	if expires_at_micros <= observed_at_micros {
		return Err(CredentialRefreshError::Ambiguous);
	}
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
	/// A live or uncertain Codex owner may still hold the same refresh-token family.
	OwnerBusy,
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
			Self::OwnerBusy => "shared Codex auth owner is still active",
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
	recovery_operation_id: Option<&'a AccountOperationId>,
	source_descriptor: &'a str,
	account: &'a AccountRecord,
}

/// Sole account lifecycle coordinator in `decodexd`.
pub struct AccountService {
	store: SqliteStore,
	credentials: Arc<dyn HostCredentialStore>,
	refresher: Arc<dyn CredentialRefreshPort>,
	shared_auth: Arc<SharedAuthCoordinator>,
	route_events: broadcast::Sender<AccountRouteCompletion>,
	pending_route_notify: Notify,
	route_command_lock: AsyncMutex<()>,
	routing_lock: AsyncMutex<()>,
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
		let (route_events, _) = broadcast::channel(16);
		Self {
			store,
			credentials,
			refresher,
			shared_auth: Arc::new(SharedAuthCoordinator::production()),
			route_events,
			pending_route_notify: Notify::new(),
			route_command_lock: AsyncMutex::new(()),
			routing_lock: AsyncMutex::new(()),
			account_locks: Mutex::new(HashMap::new()),
			callback_ready: AtomicBool::new(false),
			callback_profile_sha256: Mutex::new(None),
		}
	}

	pub(crate) fn subscribe_route_events(&self) -> broadcast::Receiver<AccountRouteCompletion> {
		self.route_events.subscribe()
	}

	pub(crate) async fn lock_route_command(&self) -> AsyncMutexGuard<'_, ()> {
		self.route_command_lock.lock().await
	}

	pub(crate) async fn pending_route_notified(&self) {
		self.pending_route_notify.notified().await;
	}

	#[cfg(test)]
	fn with_shared_auth_coordinator(mut self, coordinator: Arc<SharedAuthCoordinator>) -> Self {
		self.shared_auth = coordinator;
		self
	}

	/// Import one stable known-account rotation and never interpret absence or failure as logout.
	pub(crate) async fn follow_shared_auth_once(
		&self,
	) -> Result<Option<AccountRecord>, AccountLifecycleError> {
		let StableSharedAuthPoll::Changed(snapshot) = self.shared_auth.poll_stable_change() else {
			return Ok(None);
		};
		let crate::auth_projection::SharedCodexAuthSnapshot::Managed { version, credential } =
			*snapshot
		else {
			return Ok(None);
		};
		self.import_known_shared_auth_rotation(version, credential).await
	}

	pub(crate) fn shared_auth_may_be_running(&self) -> bool {
		self.shared_auth.liveness() == CodexLiveness::MayBeRunning
	}

	pub(crate) async fn pending_route_wait_reason(
		&self,
		account_id: &AccountId,
	) -> AccountRouteWaitReason {
		match self.inspect(account_id).await {
			Ok(inspection)
				if matches!(
					inspection.readiness,
					AccountLifecycleReadiness::CallbackCapabilityUnready
						| AccountLifecycleReadiness::StoreUnavailable
						| AccountLifecycleReadiness::StoreMismatch
						| AccountLifecycleReadiness::OperationUnsettled
				) =>
			{
				return AccountRouteWaitReason::AccountReadiness(inspection.readiness);
			},
			Ok(_) => {},
			Err(_) => {
				return AccountRouteWaitReason::AccountReadiness(
					AccountLifecycleReadiness::StoreUnavailable,
				);
			},
		}

		let observation = self.shared_auth.liveness_observation();
		if observation.state() == CodexLiveness::MayBeRunning
			&& !self.shared_auth_is_current_for(account_id).await
		{
			return AccountRouteWaitReason::from_liveness(observation);
		}
		AccountRouteWaitReason::SharedAuthStabilizing
	}

	pub(crate) async fn shared_auth_is_current_for(&self, account_id: &AccountId) -> bool {
		let Ok(lock) = self.lock_for(account_id) else {
			return false;
		};
		let _guard = lock.lock().await;
		let Ok(account) = self.load_account(account_id).await else {
			return false;
		};
		let StableSharedAuthRead::Ready(snapshot) = self.shared_auth.read_current_stable() else {
			return false;
		};
		self.confirm_shared_auth_target_locked(account_id, account.revision, &snapshot)
			.await
			.ok()
			.flatten()
			.is_some()
	}

	async fn import_known_shared_auth_rotation(
		&self,
		version: SharedCodexAuthVersion,
		mut credential: ImportedCredential,
	) -> Result<Option<AccountRecord>, AccountLifecycleError> {
		let mut matches =
			self.store.read_account_registry(None, MAX_ACCOUNT_READ).await?.into_iter().filter(
				|account| {
					!account.tombstoned
						&& account
							.credential
							.as_ref()
							.is_some_and(|binding| binding.provider == credential.provider)
				},
			);
		let Some(candidate) = matches.next() else { return Ok(None) };
		if matches.next().is_some() {
			return Err(AccountLifecycleError::CoordinatorUnavailable);
		}
		let account_id = candidate.account_id;
		let lock = self.lock_for(&account_id)?;
		let _guard = lock.lock().await;
		let account = self.load_account(&account_id).await?;
		let binding = account.credential.as_ref().ok_or(AccountLifecycleError::CredentialAbsent)?;
		let stored = self.credentials.read_exact(&account_id, binding)?;
		if self
			.refresh_predecessor(&account_id, binding)
			.await
			.map_err(AccountLifecycleError::Refresh)?
			.as_ref()
			.is_some_and(|previous| {
				bundle_matches_binding(&account_id, previous, &credential.bundle)
			}) {
			let _ = self.shared_auth.project_exact_source(
				stored.bundle(),
				binding.provider.account_id(),
				&version,
			);
			let latest = self
				.shared_auth
				.read_current_exact()
				.map_err(|_| AccountLifecycleError::CoordinatorUnavailable)?;
			let SharedCodexAuthSnapshot::Managed { credential: latest, .. } = *latest else {
				return Err(AccountLifecycleError::CoordinatorUnavailable);
			};
			if latest.provider != binding.provider {
				return Err(AccountLifecycleError::CoordinatorUnavailable);
			}
			if same_refresh_bundle(stored.bundle(), &latest.bundle) {
				return Ok(None);
			}
			credential = latest;
		}
		let Some(supplied_refresh) = matching_shared_refresh(
			binding,
			stored.bundle(),
			current_unix_micros()?,
			Ok(credential),
		) else {
			return Ok(None);
		};
		let operation_id =
			shared_auth_import_operation_id(&account_id, binding, &supplied_refresh.bundle)?;
		self.refresh_while_locked(
			operation_id,
			&account_id,
			Some(account.revision),
			None,
			None,
			RefreshPlan::route_source(supplied_refresh),
		)
		.await?;
		self.load_account(&account_id).await.map(Some)
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
		let credential = match self.shared_auth.read_current_stable() {
			StableSharedAuthRead::Ready(snapshot) => match *snapshot {
				SharedCodexAuthSnapshot::Managed { credential, .. } => credential,
				SharedCodexAuthSnapshot::Unmanaged { .. } => {
					return CodexAuthProjectionInspection::Unmanaged;
				},
			},
			StableSharedAuthRead::Waiting | StableSharedAuthRead::Unavailable => {
				return CodexAuthProjectionInspection::Unavailable;
			},
		};
		let provider_account_id = credential.provider.account_id();
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
		if !same_refresh_bundle(stored.bundle(), &credential.bundle) {
			return CodexAuthProjectionInspection::Unmanaged;
		}
		CodexAuthProjectionInspection::Current {
			account_id: account.account_id.clone(),
			account_revision: account.revision,
			projection_digest: codex_auth_projection_digest(&account, binding),
		}
	}

	async fn shared_auth_route_source(
		&self,
		target_account_id: &AccountId,
		target_binding: &CredentialBinding,
		snapshot: SharedCodexAuthSnapshot,
	) -> Result<RouteSharedAuthSnapshot, AccountLifecycleError> {
		let (version, credential) = match snapshot {
			SharedCodexAuthSnapshot::Managed { version, credential } => (version, credential),
			SharedCodexAuthSnapshot::Unmanaged { version } => {
				return Ok(RouteSharedAuthSnapshot { version, source: None });
			},
		};
		if target_binding.provider == credential.provider {
			return Ok(RouteSharedAuthSnapshot {
				version,
				source: Some(RouteSharedAuthSource {
					account_id: target_account_id.clone(),
					credential,
				}),
			});
		}

		let mut sources =
			self.store.read_account_registry(None, MAX_ACCOUNT_READ).await?.into_iter().filter(
				|account| {
					!account.tombstoned
						&& account
							.credential
							.as_ref()
							.is_some_and(|binding| binding.provider == credential.provider)
				},
			);
		let source = sources
			.next()
			.map(|account| account.account_id)
			.ok_or(AccountLifecycleError::ProviderMismatch)?;
		if sources.next().is_some() {
			return Err(AccountLifecycleError::CoordinatorUnavailable);
		}
		Ok(RouteSharedAuthSnapshot {
			version,
			source: Some(RouteSharedAuthSource { account_id: source, credential }),
		})
	}

	async fn reconcile_shared_auth_route_source(
		&self,
		route_operation_id: &AccountOperationId,
		source: &RouteSharedAuthSource,
	) -> Result<(), AccountLifecycleError> {
		let lock = self.lock_for(&source.account_id)?;
		let _guard = lock.lock().await;
		self.reconcile_shared_auth_route_source_while_locked(route_operation_id, source).await
	}

	async fn reconcile_shared_auth_route_source_while_locked(
		&self,
		route_operation_id: &AccountOperationId,
		source: &RouteSharedAuthSource,
	) -> Result<(), AccountLifecycleError> {
		let source_account_id = &source.account_id;
		let source_operation_id =
			route_refresh_operation_id(route_operation_id, source_account_id)?;
		let account = self.load_account(source_account_id).await?;
		if account.tombstoned {
			return Err(AccountLifecycleError::NotReady(AccountLifecycleReadiness::Tombstoned));
		}
		if account.unsettled_operation.is_some() {
			return Err(AccountLifecycleError::NotReady(
				AccountLifecycleReadiness::OperationUnsettled,
			));
		}
		let binding = account.credential.as_ref().ok_or(AccountLifecycleError::CredentialAbsent)?;
		let stored = self.credentials.read_exact(source_account_id, binding)?;
		if binding.provider != source.credential.provider {
			return Err(AccountLifecycleError::ProviderMismatch);
		}
		if same_refresh_bundle(stored.bundle(), &source.credential.bundle) {
			return Ok(());
		}
		let supplied_refresh = matching_shared_refresh(
			binding,
			stored.bundle(),
			current_unix_micros()?,
			Ok(ImportedCredential {
				provider: source.credential.provider.clone(),
				bundle: source.credential.bundle.clone(),
			}),
		)
		.ok_or(AccountLifecycleError::CoordinatorUnavailable)?;
		self.refresh_while_locked(
			source_operation_id,
			source_account_id,
			Some(account.revision),
			None,
			None,
			RefreshPlan::route_source(supplied_refresh),
		)
		.await?;
		Ok(())
	}

	/// Refresh, project, and select one account under one daemon-owned command receipt.
	#[allow(clippy::too_many_arguments)] // Receipt, Route identity, two revision fences, resume state, and result builder are independent authority inputs.
	pub(crate) async fn route_account_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: AccountOperationId,
		account_id: &AccountId,
		expected_account_revision: i64,
		expected_routing_revision: i64,
		resume_pending: bool,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<AccountRouteResult, AccountRouteFailure>) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let _routing_guard = self.routing_lock.lock().await;
		let mut build_response = Some(build_response);
		let routing = match self.store.read_account_routing_control().await {
			Ok(routing) => routing,
			Err(error) => {
				return self
					.complete_route_command_failure(
						lease,
						AccountRouteFailure::Lifecycle(error.into()),
						build_response.take().expect("Route builder is retained"),
					)
					.await;
			},
		};
		if routing.revision != expected_routing_revision {
			return self
				.complete_route_command_failure(
					lease,
					AccountRouteFailure::Routing(RoutingControlOutcome::StaleRoutingControl {
						revision: routing.revision,
					}),
					build_response.take().expect("Route builder is retained"),
				)
				.await;
		}
		let lock = self.lock_for(account_id)?;
		let _guard = lock.lock().await;
		let target_account = match self.load_account(account_id).await {
			Ok(account) => account,
			Err(error) => {
				return self
					.complete_route_command_failure(
						lease,
						AccountRouteFailure::Lifecycle(error),
						build_response.take().expect("Route builder is retained"),
					)
					.await;
			},
		};
		let target_revision =
			if !resume_pending || target_account.revision == expected_account_revision {
				expected_account_revision
			} else {
				let derived = route_refresh_operation_id(&operation_id, account_id)?;
				let operation = self.store.read_account_operation(&derived).await?;
				let route_successor_is_exact = route_resume_revision_is_valid(
					&target_account,
					expected_account_revision,
					&derived,
					operation.as_ref(),
				);
				if !route_successor_is_exact {
					return self
						.complete_route_command_failure(
							lease,
							AccountRouteFailure::Lifecycle(AccountLifecycleError::StaleAccount),
							build_response.take().expect("Route builder is retained"),
						)
						.await;
				}
				target_account.revision
			};
		let target_binding = match projection_binding(&target_account, target_revision) {
			Ok(binding) => binding.clone(),
			Err(error) => {
				return self
					.complete_route_command_failure(
						lease,
						AccountRouteFailure::Lifecycle(error),
						build_response.take().expect("Route builder is retained"),
					)
					.await;
			},
		};
		let stable = match self.shared_auth.read_current_stable() {
			StableSharedAuthRead::Ready(snapshot) => *snapshot,
			StableSharedAuthRead::Waiting => {
				return self
					.defer_route_command(
						lease,
						operation_id,
						account_id,
						expected_routing_revision,
						AccountRouteWaitReason::SharedAuthStabilizing,
						build_response.take().expect("Route builder is retained"),
					)
					.await;
			},
			StableSharedAuthRead::Unavailable => {
				return self
					.defer_route_command(
						lease,
						operation_id,
						account_id,
						expected_routing_revision,
						AccountRouteWaitReason::SharedAuthUnavailable,
						build_response.take().expect("Route builder is retained"),
					)
					.await;
			},
		};
		let shared_auth =
			match self.shared_auth_route_source(account_id, &target_binding, stable).await {
				Ok(source) => source,
				Err(_) => {
					return self
						.defer_route_command(
							lease,
							operation_id,
							account_id,
							expected_routing_revision,
							AccountRouteWaitReason::SharedAuthUnavailable,
							build_response.take().expect("Route builder is retained"),
						)
						.await;
				},
			};
		let source_is_target =
			shared_auth.source.as_ref().is_some_and(|source| &source.account_id == account_id);
		let liveness = self.shared_auth.liveness_observation();
		if !source_is_target && liveness.state() != CodexLiveness::Quiescent {
			return self
				.defer_route_command(
					lease,
					operation_id,
					account_id,
					expected_routing_revision,
					AccountRouteWaitReason::from_liveness(liveness),
					build_response.take().expect("Route builder is retained"),
				)
				.await;
		}
		if let Some(source) = shared_auth.source.as_ref() {
			let result = if source_is_target {
				self.reconcile_shared_auth_route_source_while_locked(&operation_id, source).await
			} else {
				self.reconcile_shared_auth_route_source(&operation_id, source).await
			};
			if let Err(error) = result {
				let _ = error;
				return self
					.defer_route_command(
						lease,
						operation_id,
						account_id,
						expected_routing_revision,
						AccountRouteWaitReason::SharedAuthUnavailable,
						build_response.take().expect("Route builder is retained"),
					)
					.await;
			}
		}

		if !source_is_target {
			let target_operation_id = match route_refresh_operation_id(&operation_id, account_id) {
				Ok(operation_id) => operation_id,
				Err(error) => {
					return self
						.complete_route_command_failure(
							lease,
							AccountRouteFailure::Lifecycle(error),
							build_response.take().expect("Route builder is retained"),
						)
						.await;
				},
			};
			if let Err(error) = self
				.refresh_while_locked(
					target_operation_id,
					account_id,
					Some(expected_account_revision),
					None,
					None,
					RefreshPlan::ROUTE_TARGET,
				)
				.await
			{
				return self
					.complete_route_command_failure(
						lease,
						AccountRouteFailure::Lifecycle(error),
						build_response.take().expect("Route builder is retained"),
					)
					.await;
			}
		}
		let routed_account = match self.load_account(account_id).await {
			Ok(account) => account,
			Err(error) => {
				return self
					.complete_route_command_failure(
						lease,
						AccountRouteFailure::Lifecycle(error),
						build_response.take().expect("Route builder is retained"),
					)
					.await;
			},
		};
		let liveness = self.shared_auth.liveness_observation();
		if !source_is_target && liveness.state() != CodexLiveness::Quiescent {
			return self
				.defer_route_command(
					lease,
					operation_id,
					account_id,
					expected_routing_revision,
					AccountRouteWaitReason::from_liveness(liveness),
					build_response.take().expect("Route builder is retained"),
				)
				.await;
		}
		let final_source = match self.shared_auth.read_current_stable() {
			StableSharedAuthRead::Ready(snapshot) if snapshot.version() == &shared_auth.version =>
				*snapshot,
			StableSharedAuthRead::Ready(_) | StableSharedAuthRead::Waiting => {
				return self
					.defer_route_command(
						lease,
						operation_id,
						account_id,
						expected_routing_revision,
						AccountRouteWaitReason::SharedAuthStabilizing,
						build_response.take().expect("Route builder is retained"),
					)
					.await;
			},
			StableSharedAuthRead::Unavailable => {
				return self
					.defer_route_command(
						lease,
						operation_id,
						account_id,
						expected_routing_revision,
						AccountRouteWaitReason::SharedAuthUnavailable,
						build_response.take().expect("Route builder is retained"),
					)
					.await;
			},
		};
		let (projected_revision, projection_digest) = if source_is_target {
			match self
				.confirm_shared_auth_target_locked(
					account_id,
					routed_account.revision,
					&final_source,
				)
				.await
			{
				Ok(Some(projection)) => projection,
				Ok(None) => {
					return self
						.defer_route_command(
							lease,
							operation_id,
							account_id,
							expected_routing_revision,
							AccountRouteWaitReason::SharedAuthStabilizing,
							build_response.take().expect("Route builder is retained"),
						)
						.await;
				},
				Err(AccountLifecycleError::CoordinatorUnavailable) => {
					return self
						.defer_route_command(
							lease,
							operation_id,
							account_id,
							expected_routing_revision,
							AccountRouteWaitReason::SharedAuthUnavailable,
							build_response.take().expect("Route builder is retained"),
						)
						.await;
				},
				Err(error) => {
					return self
						.complete_route_command_failure(
							lease,
							AccountRouteFailure::Lifecycle(error),
							build_response.take().expect("Route builder is retained"),
						)
						.await;
				},
			}
		} else {
			match self
				.project_shared_auth_locked(
					account_id,
					routed_account.revision,
					&shared_auth.version,
				)
				.await
			{
				Ok(projection) => projection,
				Err(SharedAuthProjectionError::OutcomeUnknown) => {
					return self
						.defer_route_command(
							lease,
							operation_id,
							account_id,
							expected_routing_revision,
							AccountRouteWaitReason::ProjectionReadback,
							build_response.take().expect("Route builder is retained"),
						)
						.await;
				},
				Err(SharedAuthProjectionError::Rejected(
					AccountLifecycleError::CoordinatorUnavailable,
				)) => {
					return self
						.defer_route_command(
							lease,
							operation_id,
							account_id,
							expected_routing_revision,
							AccountRouteWaitReason::SharedAuthUnavailable,
							build_response.take().expect("Route builder is retained"),
						)
						.await;
				},
				Err(SharedAuthProjectionError::Rejected(error)) => {
					return self
						.complete_route_command_failure(
							lease,
							AccountRouteFailure::Lifecycle(error),
							build_response.take().expect("Route builder is retained"),
						)
						.await;
				},
			}
		};
		if projected_revision != routed_account.revision {
			return self
				.complete_route_command_failure(
					lease,
					AccountRouteFailure::Lifecycle(AccountLifecycleError::StaleAccount),
					build_response.take().expect("Route builder is retained"),
				)
				.await;
		}

		let event_projection_digest = projection_digest.clone();
		let build_response = build_response.take().expect("Route builder is retained");
		let response = self
			.store
			.route_account_command(
				lease,
				expected_routing_revision,
				account_id,
				projected_revision,
				move |outcome, account| {
					let result = match outcome {
						RoutingControlOutcome::Updated { routing } => account
							.cloned()
							.map(|account| {
								AccountRouteResult::Committed(Box::new(AccountRouteCommit {
									account,
									routing: routing.clone(),
									projection_digest,
								}))
							})
							.ok_or(AccountRouteFailure::Lifecycle(
								AccountLifecycleError::AccountMissing,
							)),
						outcome => Err(AccountRouteFailure::Routing(outcome.clone())),
					};
					build_response(result)
				},
			)
			.await?;
		if resume_pending {
			let account = self.load_account(account_id).await?;
			let routing = self.store.read_account_routing_control().await?;
			let _ = self.route_events.send(AccountRouteCompletion {
				operation_id,
				commit: AccountRouteCommit {
					account,
					routing,
					projection_digest: event_projection_digest,
				},
			});
		}
		Ok(response)
	}

	async fn confirm_shared_auth_target_locked(
		&self,
		account_id: &AccountId,
		expected_revision: i64,
		snapshot: &SharedCodexAuthSnapshot,
	) -> Result<Option<(i64, String)>, AccountLifecycleError> {
		let SharedCodexAuthSnapshot::Managed { credential, .. } = snapshot else {
			return Ok(None);
		};
		let account = self.load_account(account_id).await?;
		let binding = projection_binding(&account, expected_revision)?;
		if binding.provider != credential.provider {
			return Ok(None);
		}
		let stored = self.credentials.read_exact(account_id, binding)?;
		if !same_refresh_bundle(stored.bundle(), &credential.bundle) {
			return Ok(None);
		}
		Ok(Some((account.revision, codex_auth_projection_digest(&account, binding))))
	}

	async fn project_shared_auth_locked(
		&self,
		account_id: &AccountId,
		expected_revision: i64,
		expected_source: &SharedCodexAuthVersion,
	) -> Result<(i64, String), SharedAuthProjectionError> {
		let account =
			self.load_account(account_id).await.map_err(SharedAuthProjectionError::Rejected)?;
		let binding = projection_binding(&account, expected_revision)
			.map_err(SharedAuthProjectionError::Rejected)?
			.clone();
		let stored = self
			.credentials
			.read_exact(account_id, &binding)
			.map_err(AccountLifecycleError::from)
			.map_err(SharedAuthProjectionError::Rejected)?;
		let id_token = stored
			.bundle()
			.id_token()
			.ok_or(AccountLifecycleError::CredentialAbsent)
			.map_err(SharedAuthProjectionError::Rejected)?;
		let identity = decode_chatgpt_identity(id_token)
			.map_err(AccountLifecycleError::from)
			.map_err(SharedAuthProjectionError::Rejected)?;
		if identity.provider != binding.provider {
			return Err(SharedAuthProjectionError::Rejected(
				AccountLifecycleError::ProviderMismatch,
			));
		}
		let latest =
			self.load_account(account_id).await.map_err(SharedAuthProjectionError::Rejected)?;
		if latest.revision != account.revision
			|| latest.enabled != account.enabled
			|| latest.lifecycle_readiness != account.lifecycle_readiness
			|| latest.tombstoned != account.tombstoned
			|| latest.credential.as_ref() != Some(&binding)
		{
			return Err(SharedAuthProjectionError::Rejected(AccountLifecycleError::StaleAccount));
		}
		match self.shared_auth.project_if_quiescent(
			stored.bundle(),
			binding.provider.account_id(),
			expected_source,
		) {
			Ok(()) => {},
			Err(CodexAuthProjectionError::OutcomeUnknown) => {
				return Err(SharedAuthProjectionError::OutcomeUnknown);
			},
			Err(error) => return Err(SharedAuthProjectionError::Rejected(projection_error(error))),
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

	/// Enroll from one owner-private Codex device-login auth file and retain the terminal result in
	/// the logical-command journal.
	#[allow(clippy::too_many_arguments)] // The journal, operation, account, source, and response owner are independent authority inputs.
	pub(crate) async fn enroll_from_credential_file_command<F>(
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
		let imported = match read_explicit_shared_codex_credential_file(source_descriptor) {
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

	#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // Keep provider resolution, credential effect, and journal completion in one auditable sequence.
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
		let resolution = self.store.resolve_account_enrollment(&account_id, &provider).await?;
		let (resolved_account_id, expected_account_revision, previous_credential) =
			match &resolution {
				AccountEnrollmentResolution::Fresh { account_id } =>
					(account_id.clone(), None, None),
				AccountEnrollmentResolution::Restore {
					account_id,
					account_revision,
					previous_credential,
				} =>
					(account_id.clone(), Some(*account_revision), Some(previous_credential.clone())),
				AccountEnrollmentResolution::AlreadyEnrolled { .. } =>
					(account_id.clone(), None, None),
			};
		let lock = self.lock_for(&resolved_account_id)?;
		let _guard = lock.lock().await;
		if self.store.resolve_account_enrollment(&account_id, &provider).await? != resolution {
			return self
				.complete_account_command_error(
					lease,
					AccountLifecycleError::StaleAccount,
					build_response,
				)
				.await;
		}
		let target_version = match previous_credential.as_ref() {
			Some(previous) =>
				previous.version.successor().map_err(|_| AccountLifecycleError::InvalidOperation)?,
			None =>
				CredentialVersion::new(1).map_err(|_| AccountLifecycleError::InvalidOperation)?,
		};
		let target =
			bundle.binding_for(&resolved_account_id, &operation_id, target_version, &provider)?;
		let preparation = AccountOperationPreparation {
			operation_id: operation_id.clone(),
			account_id: resolved_account_id,
			kind,
			display_label: Some(alias),
			enabled: Some(enabled),
			expected_account_revision,
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
			let store_result = match previous_credential.as_ref() {
				Some(previous) => self.credentials.restore_absent(
					&preparation.account_id,
					previous,
					&target,
					bundle,
				),
				None => self.credentials.create(&preparation.account_id, &target, bundle),
			};
			if let Err(error) = store_result
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
						ambiguous.then_some(if previous_credential.is_some() {
							"credential_restore_failed"
						} else {
							"credential_create_failed"
						}),
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
			RefreshPlan::ADMISSION,
		)
		.await
	}

	async fn refresh_or_reconcile_shared(
		&self,
		account_id: &AccountId,
		current: &CredentialBinding,
		stored: StoredCredential,
		shared_family: SharedFamilyRefreshPolicy,
	) -> Result<RefreshResolution, CredentialRefreshError> {
		let now_unix_micros =
			current_unix_micros().map_err(|_| CredentialRefreshError::Unavailable)?;
		let shared = match self.shared_auth.read_current_exact() {
			Ok(snapshot) => Some(*snapshot),
			Err(_) if matches!(shared_family, SharedFamilyRefreshPolicy::ProvedInactive) => None,
			Err(_) => return Err(CredentialRefreshError::OwnerBusy),
		};
		let mut projected_source = None;
		if let Some(SharedCodexAuthSnapshot::Managed { version, credential }) = shared
			&& credential.provider == current.provider
		{
			if same_refresh_bundle(stored.bundle(), &credential.bundle) {
				projected_source = Some(version);
			} else if self.refresh_predecessor(account_id, current).await?.as_ref().is_some_and(
				|previous| bundle_matches_binding(account_id, previous, &credential.bundle),
			) {
				let _ = self.shared_auth.project_exact_source(
					stored.bundle(),
					current.provider.account_id(),
					&version,
				);
				let latest = self
					.shared_auth
					.read_current_exact()
					.map_err(|_| CredentialRefreshError::OwnerBusy)?;
				let SharedCodexAuthSnapshot::Managed { credential: latest, .. } = *latest else {
					return Err(CredentialRefreshError::OwnerBusy);
				};
				if latest.provider != current.provider {
					return Err(CredentialRefreshError::OwnerBusy);
				}
				if same_refresh_bundle(stored.bundle(), &latest.bundle) {
					return Ok(RefreshResolution::Current);
				}
				if let Some(winner) =
					matching_shared_refresh(current, stored.bundle(), now_unix_micros, Ok(latest))
				{
					return Ok(RefreshResolution::Rotate {
						refreshed: winner,
						projected_source: None,
					});
				}
				return Err(CredentialRefreshError::OwnerBusy);
			} else if let Some(shared) =
				matching_shared_refresh(current, stored.bundle(), now_unix_micros, Ok(credential))
			{
				return Ok(RefreshResolution::Rotate { refreshed: shared, projected_source: None });
			} else {
				return Err(CredentialRefreshError::OwnerBusy);
			}
		}
		if projected_source.is_none()
			&& matches!(shared_family, SharedFamilyRefreshPolicy::Guard)
			&& self.shared_auth.liveness() == CodexLiveness::MayBeRunning
		{
			return Err(CredentialRefreshError::OwnerBusy);
		}

		let refresher = Arc::clone(&self.refresher);
		let (result, stored) = tokio::task::spawn_blocking(move || {
			let result = refresher.refresh(stored.bundle());
			(result, stored)
		})
		.await
		.map_err(|_| CredentialRefreshError::Ambiguous)?;

		match result {
			Ok(refreshed) => Ok(RefreshResolution::Rotate { refreshed, projected_source }),
			Err(CredentialRefreshError::Rejected) => recover_rejected_refresh_from_shared(
				current,
				stored.bundle(),
				current_unix_micros().map_err(|_| CredentialRefreshError::Rejected)?,
				self.exact_shared_auth_credential(),
			)
			.map(|refreshed| RefreshResolution::Rotate { refreshed, projected_source: None }),
			Err(error) => Err(error),
		}
	}

	async fn refresh_predecessor(
		&self,
		account_id: &AccountId,
		current: &CredentialBinding,
	) -> Result<Option<CredentialBinding>, CredentialRefreshError> {
		let operation = self
			.store
			.read_account_operation(&current.writer_operation_id)
			.await
			.map_err(|_| CredentialRefreshError::OwnerBusy)?;
		Ok(operation.and_then(|operation| {
			(operation.account_id == *account_id
				&& operation.kind == AccountOperationKind::Refresh
				&& operation.target.as_ref() == Some(current)
				&& matches!(
					operation.phase,
					AccountOperationPhase::StoreApplied | AccountOperationPhase::Committed
				))
			.then_some(operation.expected)
			.flatten()
		}))
	}

	fn exact_shared_auth_credential(&self) -> Result<ImportedCredential, CredentialImportError> {
		match *self
			.shared_auth
			.read_current_exact()
			.map_err(|_| CredentialImportError::Unavailable)?
		{
			SharedCodexAuthSnapshot::Managed { credential, .. } => Ok(credential),
			SharedCodexAuthSnapshot::Unmanaged { .. } => Err(CredentialImportError::Unavailable),
		}
	}

	#[allow(clippy::too_many_lines)] // Keep the generation-bound refresh state machine auditable as one sequence.
	async fn refresh_while_locked(
		&self,
		operation_id: AccountOperationId,
		account_id: &AccountId,
		expected_account_revision: Option<i64>,
		callback_generation: Option<(&ProcessGenerationId, &ProcessGenerationAccountBinding)>,
		previous_provider_account_id: Option<&str>,
		plan: RefreshPlan,
	) -> Result<ChatgptTokenProjection, AccountLifecycleError> {
		let RefreshPlan { allow_disabled, shared_family, supplied_refresh } = plan;
		if let Some((generation_id, process_binding)) = callback_generation {
			self.require_active_callback_generation(account_id, generation_id, process_binding)
				.await?;
		}
		let account = self.load_account(account_id).await?;
		let current = account.credential.as_ref().ok_or(AccountLifecycleError::CredentialAbsent)?;
		if current.writer_operation_id == operation_id {
			return self
				.project_committed_refresh_result(&operation_id, account_id, callback_generation)
				.await;
		}
		if previous_provider_account_id.is_some_and(|value| value != current.provider.account_id())
		{
			return Err(AccountLifecycleError::ProviderMismatch);
		}
		if let Some((_, process_binding)) = callback_generation
			&& callback_uses_current_successor(account.revision, process_binding, current)?
		{
			return self.project_refresh_result(account_id, current, callback_generation).await;
		}
		if let Some(operation) = self.store.read_account_operation(&operation_id).await? {
			self.require_operation_identity(&operation, account_id, AccountOperationKind::Refresh)?;
			return match self.reconcile_operation(&operation).await? {
				ReconciliationDisposition::Committed =>
					self.project_committed_refresh_result(
						&operation_id,
						account_id,
						callback_generation,
					)
					.await,
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
			return self
				.project_committed_refresh_result(&operation_id, account_id, callback_generation)
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
		let refreshed = match supplied_refresh {
			Some(refreshed) => Ok(RefreshResolution::Rotate { refreshed, projected_source: None }),
			None =>
				self.refresh_or_reconcile_shared(account_id, current, stored, shared_family).await,
		};
		let resolution = match refreshed {
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
			Err(CredentialRefreshError::OwnerBusy) => {
				self.recover_or_cancel(
					&operation_id,
					AccountOperationPhase::ProviderEffectPending,
					"shared_auth_owner_busy",
					false,
				)
				.await?;
				return Err(AccountLifecycleError::Refresh(CredentialRefreshError::OwnerBusy));
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
		let (refreshed, projected_source) = match resolution {
			RefreshResolution::Rotate { refreshed, projected_source } =>
				(refreshed, projected_source),
			RefreshResolution::Current => {
				self.recover_or_cancel(
					&operation_id,
					AccountOperationPhase::ProviderEffectPending,
					"shared_auth_predecessor_repaired",
					false,
				)
				.await?;
				return self.project_refresh_result(account_id, current, callback_generation).await;
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
		self.commit_and_project_refresh_result(
			&operation_id,
			account_id,
			callback_generation,
			projected_source,
		)
		.await
	}

	/// Replace one exact account credential from a private Codex auth file.
	#[allow(clippy::too_many_arguments)] // The receipt, operation, account fence, private source, and response owner are independent inputs.
	pub(crate) async fn reauthenticate_from_credential_file_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: AccountOperationId,
		account_id: &AccountId,
		expected_account_revision: i64,
		recovery_operation_id: Option<&AccountOperationId>,
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
					recovery_operation_id,
					&operation,
					build_response,
				)
				.await;
		}
		let input = ReauthenticationCommandInput {
			operation_id,
			account_id,
			expected_account_revision,
			recovery_operation_id,
			source_descriptor,
			account: &account,
		};
		self.continue_reauthentication_command(lease, input, build_response).await
	}

	#[allow(clippy::too_many_arguments)] // Replay needs the receipt, operation identity, account fence, recovery identity, and response owner together.
	async fn complete_reauthentication_replay<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: &AccountOperationId,
		account_id: &AccountId,
		expected_account_revision: i64,
		recovery_operation_id: Option<&AccountOperationId>,
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
			recovery_operation_id,
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
		recovery_operation_id: Option<&AccountOperationId>,
	) -> Result<ReauthenticationReplayDisposition, AccountLifecycleError> {
		self.require_operation_identity(operation, account_id, AccountOperationKind::Refresh)?;
		if operation.expected_account_revision != Some(expected_account_revision) {
			return Err(AccountLifecycleError::StaleAccount);
		}
		if operation.recovery_operation_id.as_ref() != recovery_operation_id
			|| operation.superseded_by_operation_id.is_some()
		{
			return Err(AccountLifecycleError::InvalidOperation);
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
			recovery_operation_id,
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
		let preparation_outcome = match recovery_operation_id {
			Some(recovery_operation_id) =>
				self.store
					.prepare_account_reauthentication_takeover(&preparation, recovery_operation_id)
					.await?,
			None => self.store.prepare_account_operation(&preparation).await?,
		};
		let phase = match preparation_outcome {
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
					if operation.phase == AccountOperationPhase::StoreApplied {
						self.commit_and_project_refresh_result(
							&operation_id,
							account_id,
							None,
							None,
						)
						.await?;
					} else {
						self.project_committed_refresh_result(&operation_id, account_id, None)
							.await?;
					}
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
					self.commit_and_project_refresh_result(&operation_id, account_id, None, None)
						.await?;
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
		let refreshed = self
			.refresh_or_reconcile_shared(
				account_id,
				&current,
				stored,
				SharedFamilyRefreshPolicy::Guard,
			)
			.await;
		let resolution = match refreshed {
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
			Err(error @ CredentialRefreshError::OwnerBusy) => {
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
		let (refreshed, projected_source) = match resolution {
			RefreshResolution::Rotate { refreshed, projected_source } =>
				(refreshed, projected_source),
			RefreshResolution::Current => {
				return self
					.complete_account_operation_success(
						lease,
						&operation_id,
						AccountOperationPhase::ProviderEffectPending,
						AccountOperationPhase::Cancelled,
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
		self.commit_and_project_refresh_result(&operation_id, account_id, None, projected_source)
			.await?;
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
		let stored = self.credentials.read_exact(account_id, binding)?;
		// The refresh state machine mirrors a successor only when it proved that this exact
		// account bundle was the shared source. This projection helper itself stays file-agnostic.
		if let Some((generation_id, process_binding)) = callback_generation {
			self.require_active_callback_generation(account_id, generation_id, process_binding)
				.await?;
		}
		Ok(projection(binding, stored.bundle()))
	}

	async fn commit_and_project_refresh_result(
		&self,
		operation_id: &AccountOperationId,
		account_id: &AccountId,
		callback_generation: Option<(&ProcessGenerationId, &ProcessGenerationAccountBinding)>,
		projected_source: Option<SharedCodexAuthVersion>,
	) -> Result<ChatgptTokenProjection, AccountLifecycleError> {
		self.commit_store_applied(operation_id).await?;
		self.project_committed_refresh_result_with_source(
			operation_id,
			account_id,
			callback_generation,
			projected_source,
		)
		.await
	}

	async fn project_committed_refresh_result(
		&self,
		operation_id: &AccountOperationId,
		account_id: &AccountId,
		callback_generation: Option<(&ProcessGenerationId, &ProcessGenerationAccountBinding)>,
	) -> Result<ChatgptTokenProjection, AccountLifecycleError> {
		self.project_committed_refresh_result_with_source(
			operation_id,
			account_id,
			callback_generation,
			None,
		)
		.await
	}

	async fn project_committed_refresh_result_with_source(
		&self,
		operation_id: &AccountOperationId,
		account_id: &AccountId,
		callback_generation: Option<(&ProcessGenerationId, &ProcessGenerationAccountBinding)>,
		projected_source: Option<SharedCodexAuthVersion>,
	) -> Result<ChatgptTokenProjection, AccountLifecycleError> {
		let operation = self
			.store
			.read_account_operation(operation_id)
			.await?
			.ok_or(AccountLifecycleError::InvalidOperation)?;
		self.require_operation_identity(&operation, account_id, AccountOperationKind::Refresh)?;
		let expected =
			operation.expected.as_ref().ok_or(AccountLifecycleError::InvalidOperation)?;
		let target = operation.target.as_ref().ok_or(AccountLifecycleError::InvalidOperation)?;
		let account = self.load_account(account_id).await?;
		let binding = account.credential.as_ref().ok_or(AccountLifecycleError::CredentialAbsent)?;
		if binding != target {
			return Err(AccountLifecycleError::StaleAccount);
		}
		let stored = self.credentials.read_exact(account_id, binding)?;
		if let Some(source) = projected_source {
			let _ = self.shared_auth.project_exact_source(
				stored.bundle(),
				binding.provider.account_id(),
				&source,
			);
		}
		self.resolve_shared_refresh_convergence(
			account_id,
			expected,
			binding,
			stored.bundle(),
			callback_generation,
			true,
		)
		.await
	}

	#[allow(clippy::too_many_arguments)] // Both durable bindings, current secret, callback fence, and one retry boundary define the arbitration proof.
	async fn resolve_shared_refresh_convergence(
		&self,
		account_id: &AccountId,
		expected: &CredentialBinding,
		target: &CredentialBinding,
		target_bundle: &CredentialSecretBundle,
		callback_generation: Option<(&ProcessGenerationId, &ProcessGenerationAccountBinding)>,
		allow_previous_projection: bool,
	) -> Result<ChatgptTokenProjection, AccountLifecycleError> {
		let snapshot = self
			.shared_auth
			.read_current_exact()
			.map_err(|_| AccountLifecycleError::CoordinatorUnavailable)?;
		match classify_shared_refresh_convergence(
			account_id,
			expected,
			target,
			target_bundle,
			*snapshot,
			current_unix_micros()?,
		) {
			SharedRefreshConvergence::Current | SharedRefreshConvergence::Unrelated =>
				self.project_refresh_result(account_id, target, callback_generation).await,
			SharedRefreshConvergence::Winner(winner) =>
				self.adopt_shared_refresh_winner(account_id, target, winner, callback_generation)
					.await,
			SharedRefreshConvergence::Previous(version) if allow_previous_projection => {
				let _ = self.shared_auth.project_exact_source(
					target_bundle,
					target.provider.account_id(),
					&version,
				);
				Box::pin(self.resolve_shared_refresh_convergence(
					account_id,
					expected,
					target,
					target_bundle,
					callback_generation,
					false,
				))
				.await
			},
			SharedRefreshConvergence::Previous(_) | SharedRefreshConvergence::Conflict =>
				Err(AccountLifecycleError::Refresh(CredentialRefreshError::OwnerBusy)),
		}
	}

	async fn adopt_shared_refresh_winner(
		&self,
		account_id: &AccountId,
		current: &CredentialBinding,
		winner: CredentialRefreshResult,
		callback_generation: Option<(&ProcessGenerationId, &ProcessGenerationAccountBinding)>,
	) -> Result<ChatgptTokenProjection, AccountLifecycleError> {
		let operation_id = shared_auth_import_operation_id(account_id, current, &winner.bundle)?;
		let account = self.load_account(account_id).await?;
		if account.credential.as_ref() != Some(current) {
			return Err(AccountLifecycleError::StaleAccount);
		}
		let projection = Box::pin(self.refresh_while_locked(
			operation_id,
			account_id,
			Some(account.revision),
			None,
			None,
			RefreshPlan::route_source(winner),
		))
		.await?;
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
		let _routing_guard = self.routing_lock.lock().await;
		Ok(self
			.store
			.set_balanced_account_selection_command(
				lease,
				expected_routing_revision,
				build_response,
			)
			.await?)
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
		let _routing_guard = self.routing_lock.lock().await;
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

	async fn complete_route_command_failure<F>(
		&self,
		lease: AccountCommandReceiptLease,
		failure: AccountRouteFailure,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<AccountRouteResult, AccountRouteFailure>) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let response = build_response(Err(failure))?;
		self.store.complete_account_command(lease, &response).await?;
		Ok(response)
	}

	async fn defer_route_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: AccountOperationId,
		account_id: &AccountId,
		routing_revision: i64,
		wait_reason: AccountRouteWaitReason,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<AccountRouteResult, AccountRouteFailure>) -> Result<Value, StoreError>
			+ Send
			+ 'static,
	{
		let response = build_response(Ok(AccountRouteResult::Pending(AccountRoutePending {
			operation_id,
			account_id: account_id.clone(),
			routing_revision,
			wait_reason,
		})))?;
		self.store.defer_account_route_command(lease, &response).await?;
		self.pending_route_notify.notify_one();
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
		let operations = self
			.store
			.read_unsettled_account_operations(MAX_UNSETTLED_ACCOUNT_OPERATION_READ)
			.await?;
		let mut summary = StartupAccountReconciliation::default();
		for discovered in operations {
			let Some(operation) =
				self.store.read_account_operation(&discovered.operation_id).await?
			else {
				continue;
			};
			if operation.superseded_by_operation_id.is_some()
				|| matches!(
					operation.phase,
					AccountOperationPhase::Committed | AccountOperationPhase::Cancelled
				) {
				continue;
			}
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
		if operation.superseded_by_operation_id.is_some() {
			return Err(AccountLifecycleError::InvalidOperation);
		}
		let lock = self.lock_for(&operation.account_id)?;
		let _guard = lock.lock().await;
		let operation = self
			.store
			.read_account_operation(operation_id)
			.await?
			.ok_or(AccountLifecycleError::InvalidOperation)?;
		if operation.superseded_by_operation_id.is_some() {
			return Err(AccountLifecycleError::InvalidOperation);
		}
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
		if operation.superseded_by_operation_id.is_some() {
			return self
				.complete_recovery_command_error(
					lease,
					AccountLifecycleError::InvalidOperation,
					build_response.take().expect("builder is retained"),
				)
				.await;
		}
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
				(AccountOperationPhase::RecoveryRequired, AccountOperationKind::Refresh)
					if operation.recovery_operation_id.is_some() =>
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
				RefreshPlan::EXISTING_WORK,
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
			(AccountOperationPhase::RecoveryRequired, AccountOperationKind::Refresh)
				if operation.recovery_operation_id.is_some() =>
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
		if let Some(disposition) =
			self.reconcile_legacy_tombstone_enrollment_collision(operation).await?
		{
			return Ok(Some(disposition));
		}
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

	async fn reconcile_legacy_tombstone_enrollment_collision(
		&self,
		operation: &AccountOperation,
	) -> Result<Option<ReconciliationDisposition>, AccountLifecycleError> {
		if !self.store.is_legacy_tombstone_enrollment_collision(operation).await? {
			return Ok(None);
		}
		let target = operation.target.as_ref().ok_or(AccountLifecycleError::InvalidOperation)?;
		match self.credentials.read_exact(&operation.account_id, target) {
			Ok(_) =>
				if self.credentials.delete(&operation.account_id, target).is_err() {
					if operation.phase == AccountOperationPhase::StoreApplied {
						accepted_phase(
							self.store
								.advance_account_operation(
									&operation.operation_id,
									AccountOperationPhase::StoreApplied,
									AccountOperationPhase::RecoveryRequired,
									Some(TOMBSTONE_ENROLLMENT_COLLISION),
								)
								.await?,
						)?;
					}
					return Ok(Some(ReconciliationDisposition::Manual));
				},
			Err(CredentialStoreError::NotFound) => {},
			Err(_) => {
				if operation.phase == AccountOperationPhase::StoreApplied {
					accepted_phase(
						self.store
							.advance_account_operation(
								&operation.operation_id,
								AccountOperationPhase::StoreApplied,
								AccountOperationPhase::RecoveryRequired,
								Some(TOMBSTONE_ENROLLMENT_COLLISION),
							)
							.await?,
					)?;
				}
				return Ok(Some(ReconciliationDisposition::Manual));
			},
		}
		if operation.phase == AccountOperationPhase::StoreApplied {
			accepted_phase(
				self.store
					.advance_account_operation(
						&operation.operation_id,
						AccountOperationPhase::StoreApplied,
						AccountOperationPhase::RecoveryRequired,
						Some(TOMBSTONE_ENROLLMENT_COLLISION),
					)
					.await?,
			)?;
		}
		accepted_phase(
			self.store
				.advance_account_operation(
					&operation.operation_id,
					AccountOperationPhase::RecoveryRequired,
					AccountOperationPhase::Cancelled,
					None,
				)
				.await?,
		)?;
		Ok(Some(ReconciliationDisposition::Cancelled))
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

fn callback_uses_current_successor(
	account_revision: i64,
	initial: &ProcessGenerationAccountBinding,
	current: &CredentialBinding,
) -> Result<bool, AccountLifecycleError> {
	if &initial.credential == current {
		return Ok(false);
	}
	if initial.credential.provider != current.provider {
		return Err(AccountLifecycleError::ProviderMismatch);
	}
	if current.version.get() <= initial.credential.version.get()
		|| account_revision < initial.account_revision
	{
		return Err(AccountLifecycleError::StaleAccount);
	}
	Ok(true)
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

fn bundle_matches_binding(
	account_id: &AccountId,
	binding: &CredentialBinding,
	bundle: &CredentialSecretBundle,
) -> bool {
	bundle
		.binding_for(account_id, &binding.writer_operation_id, binding.version, &binding.provider)
		.is_ok_and(|derived| derived == *binding)
}

fn classify_shared_refresh_convergence(
	account_id: &AccountId,
	expected: &CredentialBinding,
	target: &CredentialBinding,
	target_bundle: &CredentialSecretBundle,
	snapshot: SharedCodexAuthSnapshot,
	now_unix_micros: i64,
) -> SharedRefreshConvergence {
	let SharedCodexAuthSnapshot::Managed { version, credential } = snapshot else {
		return SharedRefreshConvergence::Unrelated;
	};
	if credential.provider != target.provider {
		return SharedRefreshConvergence::Unrelated;
	}
	if bundle_matches_binding(account_id, target, &credential.bundle) {
		return SharedRefreshConvergence::Current;
	}
	if bundle_matches_binding(account_id, expected, &credential.bundle) {
		return SharedRefreshConvergence::Previous(version);
	}
	matching_shared_refresh(target, target_bundle, now_unix_micros, Ok(credential))
		.map_or(SharedRefreshConvergence::Conflict, SharedRefreshConvergence::Winner)
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
				&& imported.bundle.access_token_expires_at_unix_micros()
					>= current_bundle.access_token_expires_at_unix_micros()
				&& !same_refresh_bundle(current_bundle, &imported.bundle) =>
			Some(CredentialRefreshResult {
				returned_provider: imported.provider,
				bundle: imported.bundle,
			}),
		Ok(_) | Err(_) => None,
	}
}

fn recover_rejected_refresh_from_shared(
	current: &CredentialBinding,
	current_bundle: &CredentialSecretBundle,
	now_unix_micros: i64,
	shared: Result<ImportedCredential, CredentialImportError>,
) -> Result<CredentialRefreshResult, CredentialRefreshError> {
	matching_shared_refresh(current, current_bundle, now_unix_micros, shared)
		.ok_or(CredentialRefreshError::Rejected)
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

fn route_resume_revision_is_valid(
	account: &AccountRecord,
	expected_revision: i64,
	derived_operation_id: &AccountOperationId,
	operation: Option<&AccountOperation>,
) -> bool {
	if account.revision == expected_revision {
		return true;
	}
	account.credential.as_ref().is_some_and(|binding| {
		binding.writer_operation_id == *derived_operation_id
			&& operation.is_some_and(|operation| {
				operation.account_id == account.account_id
					&& operation.kind == AccountOperationKind::Refresh
					&& operation.expected_account_revision == Some(expected_revision)
					&& operation.phase == AccountOperationPhase::Committed
					&& operation.target.as_ref() == Some(binding)
			})
	})
}

enum SharedAuthProjectionError {
	Rejected(AccountLifecycleError),
	OutcomeUnknown,
}

const fn projection_error(error: CodexAuthProjectionError) -> AccountLifecycleError {
	match error {
		CodexAuthProjectionError::UnsafePath | CodexAuthProjectionError::InvalidCredential =>
			AccountLifecycleError::CredentialImport,
		CodexAuthProjectionError::MissingIdentityToken => AccountLifecycleError::CredentialAbsent,
		CodexAuthProjectionError::Unavailable
		| CodexAuthProjectionError::OutcomeUnknown
		| CodexAuthProjectionError::SourceChanged => AccountLifecycleError::CoordinatorUnavailable,
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
		AccountId, AccountLifecycleReadiness, AccountOperationId, AccountOperationKind,
		AccountOperationPhase, AccountProvider, AccountQuotaWindow, AccountQuotaWindowObservation,
		AccountRecord, AccountSelectionMode, AccountState, CredentialBinding,
		CredentialFingerprint, CredentialStoreSchemaVersion, CredentialVersion, DecodexRoot,
		ProcessGenerationAccountBinding, ProviderIdentity,
	};
	use decodex_database::{
		AccountCommandKind, AccountCommandReceiptClaim, AccountLifecycleMutationOutcome,
		AccountOperationPreparation, CodexAccountCapabilityAttestation, CommandIdentity,
		RoutingControlOutcome, SqliteStore,
	};
	use serde_json::json;

	#[cfg(all(feature = "process-acceptance-fixture", debug_assertions))]
	use super::process_test_refresh_endpoint_is_safe;
	use super::{
		ACCOUNT_ALIAS_WORDS, AccountLifecycleError, AccountRouteFailure, AccountRouteResult,
		AccountRouteWaitReason, AccountService, CodexAuthProjectionError, CredentialImportError,
		CredentialRefreshError, CredentialRefreshPort, CredentialRefreshResult,
		CredentialSecretBundle, CredentialStoreError, HostCredentialStore, ImportedCredential,
		PROVIDER_REFRESH_OUTCOME_UNKNOWN, PreparedRefreshReconciliation,
		ReauthenticationReplayDisposition, RefreshResponse, accepted_phase,
		access_token_needs_refresh, account_lock_for, callback_uses_current_successor,
		classify_prepared_refresh_reconciliation, classify_reauthentication_replay,
		codex_auth_projection_digest, credential_refresh_result, decode_chatgpt_identity,
		matching_shared_refresh, projection_binding, reauthentication_current,
		reauthentication_target, recover_rejected_refresh_from_shared, refreshed_credential_target,
		require_refreshed_access_token_for_observation, resolve_reauthentication_store_effect,
		route_resume_revision_is_valid, stable_account_alias,
	};
	#[cfg(not(all(feature = "process-acceptance-fixture", debug_assertions)))]
	use super::{REFRESH_ENDPOINT, refresh_endpoint};
	use std::{
		collections::{HashMap, HashSet, VecDeque},
		fs,
		os::unix::fs::PermissionsExt as _,
		sync::{
			Arc, Mutex,
			atomic::{AtomicBool, AtomicUsize, Ordering},
		},
		time::Duration,
	};
	use tempfile::tempdir;
	use tokio::time;

	use crate::{
		auth_projection::{
			SharedCodexAuthFileStamp, SharedCodexAuthSnapshot, SharedCodexAuthVersion,
		},
		host_credentials::SqliteCredentialStore,
		shared_auth_coordinator::{
			CodexAuthHomeEvidence, CodexAuthOwnerBlocker, CodexAuthOwnerKind, CodexLiveness,
			CodexLivenessObservation, CodexLivenessPort, SharedAuthCoordinator, SharedAuthFilePort,
			StableSharedAuthPoll,
		},
	};

	const OBSERVED_AT_MICROS: i64 = 1_000_000;

	struct UnusedCredentialRefresher;
	impl CredentialRefreshPort for UnusedCredentialRefresher {
		fn refresh(
			&self,
			_current: &CredentialSecretBundle,
		) -> Result<CredentialRefreshResult, CredentialRefreshError> {
			panic!("verified reauthentication must not call the provider refresh adapter")
		}
	}

	struct FixedLiveness(CodexLiveness);
	impl CodexLivenessPort for FixedLiveness {
		fn observe(&self) -> CodexLivenessObservation {
			CodexLivenessObservation::from_state(self.0)
		}
	}

	struct MutableLiveness(Arc<AtomicBool>);
	impl CodexLivenessPort for MutableLiveness {
		fn observe(&self) -> CodexLivenessObservation {
			if self.0.load(Ordering::Relaxed) {
				CodexLivenessObservation::Blocked {
					blockers: vec![CodexAuthOwnerBlocker {
						pid: 44_768,
						kind: CodexAuthOwnerKind::Codex,
						auth_home: CodexAuthHomeEvidence::Shared,
					}],
					omitted: 0,
				}
			} else {
				CodexLivenessObservation::Quiescent
			}
		}
	}

	struct RouteSharedAuthFile {
		source_provider: &'static str,
		source_access: &'static str,
		source_expiry: i64,
		target_provider: &'static str,
		target_access: &'static str,
		projections: AtomicUsize,
	}

	impl RouteSharedAuthFile {
		fn version() -> SharedCodexAuthVersion {
			SharedCodexAuthVersion {
				stamp: SharedCodexAuthFileStamp::Present {
					device: 1,
					inode: 2,
					length: 3,
					modified_seconds: 4,
					modified_nanoseconds: 5,
				},
				sha256: Some([6; 32]),
			}
		}
	}

	impl SharedAuthFilePort for RouteSharedAuthFile {
		fn stamp(&self) -> Result<SharedCodexAuthFileStamp, CodexAuthProjectionError> {
			Ok(Self::version().stamp)
		}

		fn read(
			&self,
			_expected: &SharedCodexAuthFileStamp,
		) -> Result<SharedCodexAuthSnapshot, CodexAuthProjectionError> {
			Ok(SharedCodexAuthSnapshot::Managed {
				version: Self::version(),
				credential: imported(self.source_provider, self.source_access, self.source_expiry),
			})
		}

		fn project(
			&self,
			bundle: &CredentialSecretBundle,
			provider_account_id: &str,
			expected: &SharedCodexAuthVersion,
		) -> Result<(), CodexAuthProjectionError> {
			if provider_account_id != self.target_provider
				|| bundle.access_token() != self.target_access
				|| expected != &Self::version()
			{
				return Err(CodexAuthProjectionError::SourceChanged);
			}
			self.projections.fetch_add(1, Ordering::Relaxed);
			Ok(())
		}
	}

	fn test_coordinator(
		file: Arc<dyn SharedAuthFilePort>,
		liveness: CodexLiveness,
	) -> Arc<SharedAuthCoordinator> {
		test_coordinator_with_liveness(file, Arc::new(FixedLiveness(liveness)))
	}

	fn test_coordinator_with_liveness(
		file: Arc<dyn SharedAuthFilePort>,
		liveness: Arc<dyn CodexLivenessPort>,
	) -> Arc<SharedAuthCoordinator> {
		let coordinator = Arc::new(SharedAuthCoordinator::with_ports(liveness, file));
		assert!(matches!(coordinator.poll_stable_change(), StableSharedAuthPoll::Waiting));
		assert!(matches!(coordinator.poll_stable_change(), StableSharedAuthPoll::Changed(_)));
		coordinator
	}

	fn route_test_coordinator(file: Arc<RouteSharedAuthFile>) -> Arc<SharedAuthCoordinator> {
		test_coordinator(file, CodexLiveness::Quiescent)
	}

	struct UnmanagedSharedAuthFile {
		projections: AtomicUsize,
	}

	impl SharedAuthFilePort for UnmanagedSharedAuthFile {
		fn stamp(&self) -> Result<SharedCodexAuthFileStamp, CodexAuthProjectionError> {
			Ok(SharedCodexAuthFileStamp::Absent)
		}

		fn read(
			&self,
			_expected: &SharedCodexAuthFileStamp,
		) -> Result<SharedCodexAuthSnapshot, CodexAuthProjectionError> {
			Ok(SharedCodexAuthSnapshot::Unmanaged {
				version: SharedCodexAuthVersion {
					stamp: SharedCodexAuthFileStamp::Absent,
					sha256: None,
				},
			})
		}

		fn project(
			&self,
			_bundle: &CredentialSecretBundle,
			_provider_account_id: &str,
			_expected: &SharedCodexAuthVersion,
		) -> Result<(), CodexAuthProjectionError> {
			self.projections.fetch_add(1, Ordering::Relaxed);
			Ok(())
		}
	}

	struct CountingCredentialRefresher(Arc<AtomicUsize>);
	impl CredentialRefreshPort for CountingCredentialRefresher {
		fn refresh(
			&self,
			_current: &CredentialSecretBundle,
		) -> Result<CredentialRefreshResult, CredentialRefreshError> {
			self.0.fetch_add(1, Ordering::Relaxed);
			Err(CredentialRefreshError::Rejected)
		}
	}

	struct RouteCredentialRefresher;
	impl CredentialRefreshPort for RouteCredentialRefresher {
		fn refresh(
			&self,
			current: &CredentialSecretBundle,
		) -> Result<CredentialRefreshResult, CredentialRefreshError> {
			let identity = decode_chatgpt_identity(
				current.id_token().ok_or(CredentialRefreshError::Rejected)?,
			)
			.map_err(|_| CredentialRefreshError::Rejected)?;
			Ok(CredentialRefreshResult {
				bundle: shared_bundle(
					identity.provider.account_id(),
					"target-refreshed-access",
					i64::MAX / 2,
				),
				returned_provider: identity.provider,
			})
		}
	}

	struct RefreshRaceState {
		provider: ProviderIdentity,
		bundle: CredentialSecretBundle,
		sequence: u64,
		winner_on_project: Option<CredentialSecretBundle>,
	}

	struct RefreshRaceSharedAuthFile {
		state: Mutex<RefreshRaceState>,
		project_attempts: AtomicUsize,
	}

	impl RefreshRaceSharedAuthFile {
		fn new(provider_account_id: &str, bundle: CredentialSecretBundle) -> Self {
			Self {
				state: Mutex::new(RefreshRaceState {
					provider: ProviderIdentity::new(AccountProvider::Chatgpt, provider_account_id)
						.expect("provider identity"),
					bundle,
					sequence: 1,
					winner_on_project: None,
				}),
				project_attempts: AtomicUsize::new(0),
			}
		}

		fn version(sequence: u64) -> SharedCodexAuthVersion {
			SharedCodexAuthVersion {
				stamp: SharedCodexAuthFileStamp::Present {
					device: 41,
					inode: 42,
					length: sequence,
					modified_seconds: 43,
					modified_nanoseconds: i64::try_from(sequence).expect("test sequence fits i64"),
				},
				sha256: Some([u8::try_from(sequence).expect("test sequence fits u8"); 32]),
			}
		}

		fn set_winner_on_project(&self, winner: CredentialSecretBundle) {
			self.state.lock().expect("shared auth state").winner_on_project = Some(winner);
		}

		fn replace_from_codex(&self, winner: CredentialSecretBundle) {
			let mut state = self.state.lock().expect("shared auth state");
			state.sequence += 1;
			state.bundle = winner;
		}

		fn current_tokens(&self) -> (String, String) {
			let state = self.state.lock().expect("shared auth state");
			(state.bundle.access_token().to_owned(), state.bundle.refresh_token().to_owned())
		}

		fn current_provider(&self) -> ProviderIdentity {
			self.state.lock().expect("shared auth state").provider.clone()
		}
	}

	impl SharedAuthFilePort for RefreshRaceSharedAuthFile {
		fn stamp(&self) -> Result<SharedCodexAuthFileStamp, CodexAuthProjectionError> {
			let state = self.state.lock().map_err(|_| CodexAuthProjectionError::Unavailable)?;
			Ok(Self::version(state.sequence).stamp)
		}

		fn read(
			&self,
			expected: &SharedCodexAuthFileStamp,
		) -> Result<SharedCodexAuthSnapshot, CodexAuthProjectionError> {
			let state = self.state.lock().map_err(|_| CodexAuthProjectionError::Unavailable)?;
			let version = Self::version(state.sequence);
			if &version.stamp != expected {
				return Err(CodexAuthProjectionError::SourceChanged);
			}
			Ok(SharedCodexAuthSnapshot::Managed {
				version,
				credential: ImportedCredential {
					provider: state.provider.clone(),
					bundle: state.bundle.clone(),
				},
			})
		}

		fn project(
			&self,
			bundle: &CredentialSecretBundle,
			provider_account_id: &str,
			expected: &SharedCodexAuthVersion,
		) -> Result<(), CodexAuthProjectionError> {
			let mut state = self.state.lock().map_err(|_| CodexAuthProjectionError::Unavailable)?;
			if expected != &Self::version(state.sequence) {
				return Err(CodexAuthProjectionError::SourceChanged);
			}
			let provider = ProviderIdentity::new(AccountProvider::Chatgpt, provider_account_id)
				.map_err(|_| CodexAuthProjectionError::InvalidCredential)?;
			self.project_attempts.fetch_add(1, Ordering::Relaxed);
			state.sequence += 1;
			if let Some(winner) = state.winner_on_project.take() {
				state.bundle = winner;
				return Err(CodexAuthProjectionError::SourceChanged);
			}
			state.provider = provider;
			state.bundle = bundle.clone();
			Ok(())
		}
	}

	struct ScriptedCredentialRefresher {
		calls: Arc<AtomicUsize>,
		results: Mutex<VecDeque<CredentialRefreshResult>>,
	}

	impl CredentialRefreshPort for ScriptedCredentialRefresher {
		fn refresh(
			&self,
			_current: &CredentialSecretBundle,
		) -> Result<CredentialRefreshResult, CredentialRefreshError> {
			self.calls.fetch_add(1, Ordering::Relaxed);
			self.results
				.lock()
				.map_err(|_| CredentialRefreshError::Unavailable)?
				.pop_front()
				.ok_or(CredentialRefreshError::Unavailable)
		}
	}

	async fn enroll_route_test_account(
		store: &SqliteStore,
		credentials: &Arc<dyn HostCredentialStore>,
		account_id: &str,
		operation_id: &str,
		provider_account_id: &str,
		access_token: &str,
	) -> AccountId {
		enroll_route_test_account_with_expiry(
			store,
			credentials,
			account_id,
			operation_id,
			provider_account_id,
			access_token,
			2_000_000,
		)
		.await
	}

	async fn enroll_route_test_account_with_expiry(
		store: &SqliteStore,
		credentials: &Arc<dyn HostCredentialStore>,
		account_id: &str,
		operation_id: &str,
		provider_account_id: &str,
		access_token: &str,
		expiry: i64,
	) -> AccountId {
		let account_id = AccountId::new(account_id).expect("account identity");
		let operation_id = AccountOperationId::new(operation_id).expect("enrollment operation");
		let provider = ProviderIdentity::new(AccountProvider::Chatgpt, provider_account_id)
			.expect("provider identity");
		let bundle = shared_bundle(provider.account_id(), access_token, expiry);
		let binding = bundle
			.binding_for(
				&account_id,
				&operation_id,
				CredentialVersion::new(1).expect("credential version"),
				&provider,
			)
			.expect("credential binding");
		accepted_phase(
			store
				.prepare_account_operation(&AccountOperationPreparation {
					operation_id: operation_id.clone(),
					account_id: account_id.clone(),
					kind: AccountOperationKind::Enroll,
					display_label: Some(stable_account_alias(&provider)),
					enabled: Some(true),
					expected_account_revision: None,
					expected: None,
					target: Some(binding.clone()),
					provider,
				})
				.await
				.expect("prepare enrollment"),
		)
		.expect("accept enrollment");
		credentials.create(&account_id, &binding, bundle).expect("create credential");
		accepted_phase(
			store
				.advance_account_operation(
					&operation_id,
					AccountOperationPhase::Prepared,
					AccountOperationPhase::StoreApplied,
					None,
				)
				.await
				.expect("record credential"),
		)
		.expect("accept credential");
		accepted_phase(
			store
				.advance_account_operation(
					&operation_id,
					AccountOperationPhase::StoreApplied,
					AccountOperationPhase::Committed,
					None,
				)
				.await
				.expect("commit enrollment"),
		)
		.expect("accept enrollment commit");
		account_id
	}

	async fn route_test_account_once(
		service: &AccountService,
		store: &SqliteStore,
		account_id: &AccountId,
		expected_account_revision: i64,
		operation_id: &str,
		idempotency_key: &str,
	) -> serde_json::Value {
		let _ = service.shared_auth.poll_stable_change();
		let _ = service.shared_auth.poll_stable_change();
		let routing = store.read_account_routing_control().await.expect("read routing control");
		let request = format!(
			r#"{{"name":"route_account","arguments":{{"operation_id":"{operation_id}","account_id":"{}","expected_account_revision":{expected_account_revision}}}}}"#,
			account_id.as_str(),
		);
		let command = CommandIdentity::new(idempotency_key, request.as_bytes())
			.expect("Route command identity");
		let lease = match store
			.reserve_account_route_command(&command, routing.revision, &request)
			.await
			.expect("reserve Route command")
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Pending(_) | AccountCommandReceiptClaim::Replayed(_) => {
				panic!("new Route command did not acquire its receipt")
			},
		};
		service
			.route_account_command(
				lease,
				AccountOperationId::new(operation_id).expect("Route operation identity"),
				account_id,
				expected_account_revision,
				routing.revision,
				false,
				|result| {
					Ok(match result {
						Ok(AccountRouteResult::Committed(commit)) => json!({
							"outcome": "routed",
							"account_id": commit.account.account_id.as_str(),
							"account_revision": commit.account.revision,
							"routing_revision": commit.routing.revision,
							"projection_digest": commit.projection_digest,
						}),
						Ok(AccountRouteResult::Pending(pending)) => json!({
							"outcome": "pending",
							"wait_reason": format!("{:?}", pending.wait_reason),
						}),
						Err(_) => json!({"outcome": "rejected"}),
					})
				},
			)
			.await
			.expect("complete Route command")
	}

	async fn assert_exact_route_readback(
		store: &SqliteStore,
		credentials: &Arc<dyn HostCredentialStore>,
		shared_auth: &RefreshRaceSharedAuthFile,
		account_id: &AccountId,
		provider_account_id: &str,
	) {
		let routing = store.read_account_routing_control().await.expect("read routed control");
		assert_eq!(routing.mode, AccountSelectionMode::Fixed(account_id.clone()));
		let account = store
			.read_account_registry(Some(account_id), 1)
			.await
			.expect("read routed account")
			.pop()
			.expect("routed account exists");
		let binding = account.credential.expect("routed credential binding");
		let stored = credentials.read_exact(account_id, &binding).expect("read exact credential");
		assert_eq!(shared_auth.current_provider().account_id(), provider_account_id);
		assert_eq!(shared_auth.current_tokens().0, stored.bundle().access_token());
		assert_eq!(shared_auth.current_tokens().1, stored.bundle().refresh_token());
	}

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

	#[tokio::test]
	async fn live_codex_allows_only_the_exact_projected_family_to_refresh() {
		let directory = tempdir().expect("temporary product root");
		let root = DecodexRoot::new(fs::canonicalize(directory.path()).expect("canonical root"))
			.expect("typed product root");
		let store = SqliteStore::open(&root.paths()).expect("open product store");
		let credentials: Arc<dyn HostCredentialStore> =
			Arc::new(SqliteCredentialStore::new(store.clone()));
		let account_id = enroll_route_test_account(
			&store,
			&credentials,
			"21000000-0000-4000-8000-000000000061",
			"22000000-0000-4000-8000-000000000061",
			"live-provider",
			"live-access",
		)
		.await;
		let provider_calls = Arc::new(AtomicUsize::new(0));
		let managed_file = Arc::new(RouteSharedAuthFile {
			source_provider: "live-provider",
			source_access: "live-access",
			source_expiry: 2_000_000,
			target_provider: "live-provider",
			target_access: "unused",
			projections: AtomicUsize::new(0),
		});
		let managed = AccountService::new(
			store.clone(),
			Arc::clone(&credentials),
			Arc::new(CountingCredentialRefresher(Arc::clone(&provider_calls))),
		)
		.with_shared_auth_coordinator(test_coordinator(
			Arc::clone(&managed_file) as Arc<dyn SharedAuthFilePort>,
			CodexLiveness::MayBeRunning,
		));
		assert!(
			managed
				.attest_callback_capability(CodexAccountCapabilityAttestation {
					build_identity: "owner-busy-managed".to_owned(),
					executable_sha256: "1".repeat(64),
					schema_sha256: "2".repeat(64),
					callback_profile_sha256: "3".repeat(64),
					login_chatgpt_auth_tokens: true,
					refresh_callback: true,
				})
				.await
				.expect("attest callback")
		);
		assert!(matches!(
			managed
				.refresh(
					AccountOperationId::new("22000000-0000-4000-8000-000000000062")
						.expect("refresh operation"),
					&account_id,
					Some(1),
					None,
					None,
				)
				.await,
			Err(AccountLifecycleError::Refresh(CredentialRefreshError::Rejected))
		));

		let unmanaged_file = Arc::new(UnmanagedSharedAuthFile { projections: AtomicUsize::new(0) });
		let unmanaged = AccountService::new(
			store,
			Arc::clone(&credentials),
			Arc::new(CountingCredentialRefresher(Arc::clone(&provider_calls))),
		)
		.with_shared_auth_coordinator(test_coordinator(
			Arc::clone(&unmanaged_file) as Arc<dyn SharedAuthFilePort>,
			CodexLiveness::MayBeRunning,
		));
		assert!(
			unmanaged
				.attest_callback_capability(CodexAccountCapabilityAttestation {
					build_identity: "owner-busy-unmanaged".to_owned(),
					executable_sha256: "4".repeat(64),
					schema_sha256: "5".repeat(64),
					callback_profile_sha256: "6".repeat(64),
					login_chatgpt_auth_tokens: true,
					refresh_callback: true,
				})
				.await
				.expect("attest callback")
		);
		assert!(matches!(
			unmanaged
				.refresh(
					AccountOperationId::new("22000000-0000-4000-8000-000000000063")
						.expect("refresh operation"),
					&account_id,
					Some(1),
					None,
					None,
				)
				.await,
			Err(AccountLifecycleError::Refresh(CredentialRefreshError::OwnerBusy))
		));
		assert_eq!(provider_calls.load(Ordering::Relaxed), 1);
		assert_eq!(managed_file.projections.load(Ordering::Relaxed), 0);
		assert_eq!(unmanaged_file.projections.load(Ordering::Relaxed), 0);
	}

	#[tokio::test]
	async fn projected_refresh_mirrors_success_and_adopts_a_concurrent_codex_winner() {
		let directory = tempdir().expect("temporary product root");
		let root = DecodexRoot::new(fs::canonicalize(directory.path()).expect("canonical root"))
			.expect("typed product root");
		let store = SqliteStore::open(&root.paths()).expect("open product store");
		let credentials: Arc<dyn HostCredentialStore> =
			Arc::new(SqliteCredentialStore::new(store.clone()));
		let provider_account_id = "refresh-race-provider";
		let initial_expiry = i64::MAX / 4;
		let account_id = enroll_route_test_account_with_expiry(
			&store,
			&credentials,
			"21000000-0000-4000-8000-000000000071",
			"22000000-0000-4000-8000-000000000071",
			provider_account_id,
			"initial-access",
			initial_expiry,
		)
		.await;
		let provider = ProviderIdentity::new(AccountProvider::Chatgpt, provider_account_id)
			.expect("provider identity");
		let shared_auth_file = Arc::new(RefreshRaceSharedAuthFile::new(
			provider_account_id,
			shared_bundle(provider_account_id, "initial-access", initial_expiry),
		));
		let provider_calls = Arc::new(AtomicUsize::new(0));
		let refresher = Arc::new(ScriptedCredentialRefresher {
			calls: Arc::clone(&provider_calls),
			results: Mutex::new(VecDeque::from([
				CredentialRefreshResult {
					returned_provider: provider.clone(),
					bundle: shared_bundle_with_refresh(
						provider_account_id,
						"decodex-first-access",
						"decodex-first-refresh",
						initial_expiry + 1_000_000,
					),
				},
				CredentialRefreshResult {
					returned_provider: provider,
					bundle: shared_bundle_with_refresh(
						provider_account_id,
						"decodex-loser-access",
						"decodex-loser-refresh",
						initial_expiry + 2_000_000,
					),
				},
			])),
		});
		let service = AccountService::new(store.clone(), Arc::clone(&credentials), refresher)
			.with_shared_auth_coordinator(test_coordinator(
				Arc::clone(&shared_auth_file) as Arc<dyn SharedAuthFilePort>,
				CodexLiveness::MayBeRunning,
			));
		assert!(
			service
				.attest_callback_capability(CodexAccountCapabilityAttestation {
					build_identity: "refresh-race-build".to_owned(),
					executable_sha256: "1".repeat(64),
					schema_sha256: "2".repeat(64),
					callback_profile_sha256: "3".repeat(64),
					login_chatgpt_auth_tokens: true,
					refresh_callback: true,
				})
				.await
				.expect("attest callback")
		);

		let first = service
			.refresh(
				AccountOperationId::new("22000000-0000-4000-8000-000000000072")
					.expect("first refresh operation"),
				&account_id,
				Some(1),
				None,
				None,
			)
			.await
			.expect("refresh the exact projected family");
		assert_eq!(first.access_token(), "decodex-first-access");
		assert_eq!(
			shared_auth_file.current_tokens(),
			("decodex-first-access".to_owned(), "decodex-first-refresh".to_owned())
		);

		let after_first = store
			.read_account_registry(Some(&account_id), 1)
			.await
			.expect("read first refresh")
			.into_iter()
			.next()
			.expect("refreshed account");
		let codex_winner = shared_bundle_with_refresh(
			provider_account_id,
			"codex-winner-access",
			"codex-winner-refresh",
			initial_expiry + 3_000_000,
		);
		shared_auth_file.set_winner_on_project(codex_winner);

		let winner = service
			.refresh(
				AccountOperationId::new("22000000-0000-4000-8000-000000000073")
					.expect("racing refresh operation"),
				&account_id,
				Some(after_first.revision),
				None,
				None,
			)
			.await
			.expect("adopt the concurrent Codex winner");
		assert_eq!(winner.access_token(), "codex-winner-access");
		assert_eq!(
			shared_auth_file.current_tokens(),
			("codex-winner-access".to_owned(), "codex-winner-refresh".to_owned())
		);
		let final_account = store
			.read_account_registry(Some(&account_id), 1)
			.await
			.expect("read converged account")
			.into_iter()
			.next()
			.expect("converged account");
		let final_binding = final_account.credential.expect("converged credential binding");
		let final_stored =
			credentials.read_exact(&account_id, &final_binding).expect("read converged credential");
		assert_eq!(final_stored.bundle().access_token(), "codex-winner-access");
		assert_eq!(final_stored.bundle().refresh_token(), "codex-winner-refresh");
		assert_eq!(provider_calls.load(Ordering::Relaxed), 2);
		assert_eq!(shared_auth_file.project_attempts.load(Ordering::Relaxed), 2);

		shared_auth_file.replace_from_codex(shared_bundle_with_refresh(
			provider_account_id,
			"decodex-loser-access",
			"decodex-loser-refresh",
			initial_expiry + 2_000_000,
		));
		assert!(service.follow_shared_auth_once().await.unwrap().is_none());
		assert!(service.follow_shared_auth_once().await.unwrap().is_none());
		assert_eq!(
			shared_auth_file.current_tokens(),
			("codex-winner-access".to_owned(), "codex-winner-refresh".to_owned())
		);
		let after_stale_rewrite = store
			.read_account_registry(Some(&account_id), 1)
			.await
			.expect("read stale-rewrite repair")
			.into_iter()
			.next()
			.expect("repaired account");
		assert_eq!(after_stale_rewrite.revision, final_account.revision);
		assert_eq!(provider_calls.load(Ordering::Relaxed), 2);
	}

	#[tokio::test]
	async fn stable_known_account_same_expiry_rotation_imports_once_without_provider_work() {
		let directory = tempdir().expect("temporary product root");
		let root = DecodexRoot::new(fs::canonicalize(directory.path()).expect("canonical root"))
			.expect("typed product root");
		let store = SqliteStore::open(&root.paths()).expect("open product store");
		let credentials: Arc<dyn HostCredentialStore> =
			Arc::new(SqliteCredentialStore::new(store.clone()));
		let expiry = i64::MAX / 2;
		let account_id = enroll_route_test_account_with_expiry(
			&store,
			&credentials,
			"21000000-0000-4000-8000-000000000081",
			"22000000-0000-4000-8000-000000000081",
			"rotation-provider",
			"initial-access",
			expiry,
		)
		.await;
		let file = Arc::new(RouteSharedAuthFile {
			source_provider: "rotation-provider",
			source_access: "rotated-access",
			source_expiry: expiry,
			target_provider: "rotation-provider",
			target_access: "rotated-access",
			projections: AtomicUsize::new(0),
		});
		let coordinator = Arc::new(SharedAuthCoordinator::with_ports(
			Arc::new(FixedLiveness(CodexLiveness::MayBeRunning)),
			Arc::clone(&file) as Arc<dyn SharedAuthFilePort>,
		));
		let service = AccountService::new(
			store.clone(),
			Arc::clone(&credentials),
			Arc::new(UnusedCredentialRefresher),
		)
		.with_shared_auth_coordinator(coordinator);

		assert!(
			service
				.attest_callback_capability(CodexAccountCapabilityAttestation {
					build_identity: "rotation-import-build".to_owned(),
					executable_sha256: "a".repeat(64),
					schema_sha256: "b".repeat(64),
					callback_profile_sha256: "c".repeat(64),
					login_chatgpt_auth_tokens: true,
					refresh_callback: true,
				})
				.await
				.expect("attest callback")
		);
		assert!(service.follow_shared_auth_once().await.unwrap().is_none());
		let imported =
			service.follow_shared_auth_once().await.unwrap().expect("known rotation imports");
		assert_eq!(imported.account_id, account_id);
		assert_eq!(imported.revision, 2);
		let binding = imported.credential.expect("imported binding");
		let stored = credentials.read_exact(&account_id, &binding).expect("read imported bundle");
		assert_eq!(stored.bundle().access_token(), "rotated-access");
		assert_eq!(stored.bundle().access_token_expires_at_unix_micros(), expiry);
		assert!(service.follow_shared_auth_once().await.unwrap().is_none());
		assert_eq!(file.projections.load(Ordering::Relaxed), 0);
	}

	#[tokio::test]
	async fn route_command_confirms_an_already_current_shared_target_without_rewriting_it() {
		let directory = tempdir().expect("temporary product root");
		let root = DecodexRoot::new(fs::canonicalize(directory.path()).expect("canonical root"))
			.expect("typed product root");
		let store = SqliteStore::open(&root.paths()).expect("open product store");
		let credentials: Arc<dyn HostCredentialStore> =
			Arc::new(SqliteCredentialStore::new(store.clone()));
		let provider = ProviderIdentity::new(AccountProvider::Chatgpt, "route-provider")
			.expect("provider identity");
		let account_id =
			AccountId::new("21000000-0000-4000-8000-000000000041").expect("account identity");
		let enrollment_operation = AccountOperationId::new("22000000-0000-4000-8000-000000000041")
			.expect("enrollment operation");
		let bundle = shared_bundle(provider.account_id(), "route-access", 3_000_000);
		let binding = bundle
			.binding_for(
				&account_id,
				&enrollment_operation,
				CredentialVersion::new(1).expect("credential version"),
				&provider,
			)
			.expect("credential binding");
		accepted_phase(
			store
				.prepare_account_operation(&AccountOperationPreparation {
					operation_id: enrollment_operation.clone(),
					account_id: account_id.clone(),
					kind: AccountOperationKind::Enroll,
					display_label: Some(stable_account_alias(&provider)),
					enabled: Some(true),
					expected_account_revision: None,
					expected: None,
					target: Some(binding.clone()),
					provider: provider.clone(),
				})
				.await
				.expect("prepare enrollment"),
		)
		.expect("accept enrollment");
		credentials.create(&account_id, &binding, bundle).expect("create credential");
		accepted_phase(
			store
				.advance_account_operation(
					&enrollment_operation,
					AccountOperationPhase::Prepared,
					AccountOperationPhase::StoreApplied,
					None,
				)
				.await
				.expect("record credential"),
		)
		.expect("accept credential");
		accepted_phase(
			store
				.advance_account_operation(
					&enrollment_operation,
					AccountOperationPhase::StoreApplied,
					AccountOperationPhase::Committed,
					None,
				)
				.await
				.expect("commit enrollment"),
		)
		.expect("accept enrollment commit");
		let routing = store.read_account_routing_control().await.expect("read routing");
		let request = format!(
			r#"{{"name":"route_account","arguments":{{"operation_id":"22000000-0000-4000-8000-000000000042","account_id":"{}","expected_account_revision":1}}}}"#,
			account_id.as_str(),
		);
		let command = CommandIdentity::new("route-command", request.as_bytes())
			.expect("Route command identity");
		let lease = match store
			.reserve_account_route_command(&command, routing.revision, &request)
			.await
			.expect("reserve Route")
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Pending(_) | AccountCommandReceiptClaim::Replayed(_) => {
				panic!("new Route replayed")
			},
		};
		let shared_auth_file = Arc::new(RouteSharedAuthFile {
			source_provider: "route-provider",
			source_access: "route-access",
			source_expiry: 3_000_000,
			target_provider: "route-provider",
			target_access: "route-access",
			projections: AtomicUsize::new(0),
		});
		let service = AccountService::new(
			store.clone(),
			Arc::clone(&credentials),
			Arc::new(UnusedCredentialRefresher),
		)
		.with_shared_auth_coordinator(test_coordinator(
			Arc::clone(&shared_auth_file) as Arc<dyn SharedAuthFilePort>,
			CodexLiveness::MayBeRunning,
		));
		assert!(
			service
				.attest_callback_capability(CodexAccountCapabilityAttestation {
					build_identity: "route-test-build".to_owned(),
					executable_sha256: "a".repeat(64),
					schema_sha256: "b".repeat(64),
					callback_profile_sha256: "c".repeat(64),
					login_chatgpt_auth_tokens: true,
					refresh_callback: true,
				})
				.await
				.expect("attest Route callback capability")
		);
		let response = service
			.route_account_command(
				lease,
				AccountOperationId::new("22000000-0000-4000-8000-000000000042")
					.expect("Route operation"),
				&account_id,
				1,
				routing.revision,
				false,
				|result| {
					Ok(match result {
						Ok(AccountRouteResult::Committed(commit)) => json!({
							"account_revision": commit.account.revision,
							"routing_revision": commit.routing.revision,
							"projection_digest": commit.projection_digest,
						}),
						Ok(AccountRouteResult::Pending(_)) | Err(_) => {
							json!({"outcome": "unexpected"})
						},
					})
				},
			)
			.await
			.expect("complete Route");

		assert_eq!(response["account_revision"], 1);
		assert_eq!(response["routing_revision"], routing.revision + 1);
		assert_eq!(response["projection_digest"].as_str().map(str::len), Some(64));
		assert_eq!(shared_auth_file.projections.load(Ordering::Relaxed), 0);
		let routed = store.read_account_routing_control().await.expect("read routed policy");
		assert_eq!(routed.mode, AccountSelectionMode::Fixed(account_id.clone()));

		let stale_request = format!(
			r#"{{"name":"route_account","arguments":{{"operation_id":"22000000-0000-4000-8000-000000000043","account_id":"{}","expected_account_revision":1}}}}"#,
			account_id.as_str(),
		);
		let stale_command = CommandIdentity::new("stale-route-command", stale_request.as_bytes())
			.expect("stale Route command identity");
		let stale_lease = match store
			.reserve_account_route_command(&stale_command, routing.revision, &stale_request)
			.await
			.expect("reserve stale Route")
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Pending(_) | AccountCommandReceiptClaim::Replayed(_) => {
				panic!("new stale Route replayed")
			},
		};
		let stale_response = service
			.route_account_command(
				stale_lease,
				AccountOperationId::new("22000000-0000-4000-8000-000000000043")
					.expect("stale Route operation"),
				&account_id,
				1,
				routing.revision,
				false,
				|result| {
					Ok(match result {
						Err(AccountRouteFailure::Routing(
							RoutingControlOutcome::StaleRoutingControl { revision },
						)) => json!({"outcome": "stale_routing", "revision": revision}),
						_ => json!({"outcome": "unexpected"}),
					})
				},
			)
			.await
			.expect("complete stale Route");
		assert_eq!(stale_response["outcome"], "stale_routing");
		assert_eq!(shared_auth_file.projections.load(Ordering::Relaxed), 0);
		assert!(
			store
				.read_pending_account_route_commands(8)
				.await
				.expect("read pending Routes")
				.is_empty()
		);
	}

	#[tokio::test]
	async fn account_route_a_b_a_and_post_restart_switch_keep_sqlite_and_shared_auth_exact() {
		let directory = tempdir().expect("temporary product root");
		let root = DecodexRoot::new(fs::canonicalize(directory.path()).expect("canonical root"))
			.expect("typed product root");
		let store = SqliteStore::open(&root.paths()).expect("open product store");
		let credentials: Arc<dyn HostCredentialStore> =
			Arc::new(SqliteCredentialStore::new(store.clone()));
		let expiry = i64::MAX / 2;
		let account_a = enroll_route_test_account_with_expiry(
			&store,
			&credentials,
			"21000000-0000-4000-8000-0000000000e1",
			"22000000-0000-4000-8000-0000000000e1",
			"round-trip-provider-a",
			"round-trip-access-a",
			expiry,
		)
		.await;
		let account_b = enroll_route_test_account_with_expiry(
			&store,
			&credentials,
			"21000000-0000-4000-8000-0000000000e2",
			"22000000-0000-4000-8000-0000000000e2",
			"round-trip-provider-b",
			"round-trip-access-b",
			expiry,
		)
		.await;
		let shared_auth = Arc::new(RefreshRaceSharedAuthFile::new(
			"round-trip-provider-a",
			shared_bundle("round-trip-provider-a", "round-trip-access-a", expiry),
		));
		let provider_calls = Arc::new(AtomicUsize::new(0));
		let refresher = Arc::new(ScriptedCredentialRefresher {
			calls: Arc::clone(&provider_calls),
			results: Mutex::new(VecDeque::from([
				CredentialRefreshResult {
					returned_provider: ProviderIdentity::new(
						AccountProvider::Chatgpt,
						"round-trip-provider-b",
					)
					.expect("provider B identity"),
					bundle: shared_bundle_with_refresh(
						"round-trip-provider-b",
						"round-trip-access-b-routed",
						"round-trip-refresh-b-routed",
						expiry,
					),
				},
				CredentialRefreshResult {
					returned_provider: ProviderIdentity::new(
						AccountProvider::Chatgpt,
						"round-trip-provider-a",
					)
					.expect("provider A identity"),
					bundle: shared_bundle_with_refresh(
						"round-trip-provider-a",
						"round-trip-access-a-routed",
						"round-trip-refresh-a-routed",
						expiry,
					),
				},
				CredentialRefreshResult {
					returned_provider: ProviderIdentity::new(
						AccountProvider::Chatgpt,
						"round-trip-provider-b",
					)
					.expect("provider B identity"),
					bundle: shared_bundle_with_refresh(
						"round-trip-provider-b",
						"round-trip-access-b-restarted",
						"round-trip-refresh-b-restarted",
						expiry,
					),
				},
			])),
		});
		let build_service = || {
			AccountService::new(
				store.clone(),
				Arc::clone(&credentials),
				Arc::clone(&refresher) as Arc<dyn CredentialRefreshPort>,
			)
			.with_shared_auth_coordinator(test_coordinator(
				Arc::clone(&shared_auth) as Arc<dyn SharedAuthFilePort>,
				CodexLiveness::Quiescent,
			))
		};
		let service = build_service();
		assert!(
			service
				.attest_callback_capability(CodexAccountCapabilityAttestation {
					build_identity: "round-trip-route-build".to_owned(),
					executable_sha256: "1".repeat(64),
					schema_sha256: "2".repeat(64),
					callback_profile_sha256: "3".repeat(64),
					login_chatgpt_auth_tokens: true,
					refresh_callback: true,
				})
				.await
				.expect("attest callback capability")
		);

		for (account_id, expected_revision, provider, operation, key) in [
			(
				&account_b,
				1,
				"round-trip-provider-b",
				"22000000-0000-4000-8000-0000000000e3",
				"round-trip-route-b",
			),
			(
				&account_a,
				1,
				"round-trip-provider-a",
				"22000000-0000-4000-8000-0000000000e4",
				"round-trip-route-a",
			),
		] {
			let response = route_test_account_once(
				&service,
				&store,
				account_id,
				expected_revision,
				operation,
				key,
			)
			.await;
			assert_eq!(response["outcome"], "routed");
			assert_eq!(response["account_id"], account_id.as_str());
			assert_eq!(response["projection_digest"].as_str().map(str::len), Some(64));
			assert_exact_route_readback(&store, &credentials, &shared_auth, account_id, provider)
				.await;
		}

		drop(service);
		let restarted = build_service();
		assert!(
			restarted
				.attest_callback_capability(CodexAccountCapabilityAttestation {
					build_identity: "round-trip-route-restarted-build".to_owned(),
					executable_sha256: "4".repeat(64),
					schema_sha256: "5".repeat(64),
					callback_profile_sha256: "6".repeat(64),
					login_chatgpt_auth_tokens: true,
					refresh_callback: true,
				})
				.await
				.expect("attest restarted callback capability")
		);
		let response = route_test_account_once(
			&restarted,
			&store,
			&account_b,
			2,
			"22000000-0000-4000-8000-0000000000e5",
			"round-trip-route-b-after-restart",
		)
		.await;
		assert_eq!(response["outcome"], "routed");
		assert_exact_route_readback(
			&store,
			&credentials,
			&shared_auth,
			&account_b,
			"round-trip-provider-b",
		)
		.await;
		assert_eq!(shared_auth.project_attempts.load(Ordering::Relaxed), 3);
		assert_eq!(provider_calls.load(Ordering::Relaxed), 3);
	}

	#[tokio::test]
	async fn cross_account_route_preserves_the_exact_shared_source_before_projection() {
		let directory = tempdir().expect("temporary product root");
		let root = DecodexRoot::new(fs::canonicalize(directory.path()).expect("canonical root"))
			.expect("typed product root");
		let store = SqliteStore::open(&root.paths()).expect("open product store");
		let credentials: Arc<dyn HostCredentialStore> =
			Arc::new(SqliteCredentialStore::new(store.clone()));
		let source_account_id = enroll_route_test_account(
			&store,
			&credentials,
			"21000000-0000-4000-8000-000000000051",
			"22000000-0000-4000-8000-000000000051",
			"source-provider",
			"source-access",
		)
		.await;
		let target_account_id = enroll_route_test_account(
			&store,
			&credentials,
			"21000000-0000-4000-8000-000000000052",
			"22000000-0000-4000-8000-000000000052",
			"target-provider",
			"target-access",
		)
		.await;
		let routing = store.read_account_routing_control().await.expect("read routing");
		let request = format!(
			r#"{{"name":"route_account","arguments":{{"operation_id":"22000000-0000-4000-8000-000000000053","account_id":"{}","expected_account_revision":1}}}}"#,
			target_account_id.as_str(),
		);
		let command = CommandIdentity::new("cross-account-route", request.as_bytes())
			.expect("cross-account Route identity");
		let lease = match store
			.reserve_account_route_command(&command, routing.revision, &request)
			.await
			.expect("reserve cross-account Route")
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Pending(_) | AccountCommandReceiptClaim::Replayed(_) => {
				panic!("new Route replayed")
			},
		};
		let shared_auth_file = Arc::new(RouteSharedAuthFile {
			source_provider: "source-provider",
			source_access: "source-rotated-access",
			source_expiry: i64::MAX / 2,
			target_provider: "target-provider",
			target_access: "target-refreshed-access",
			projections: AtomicUsize::new(0),
		});
		let service = AccountService::new(
			store.clone(),
			Arc::clone(&credentials),
			Arc::new(RouteCredentialRefresher),
		)
		.with_shared_auth_coordinator(route_test_coordinator(Arc::clone(&shared_auth_file)));
		assert!(
			service
				.attest_callback_capability(CodexAccountCapabilityAttestation {
					build_identity: "cross-route-test-build".to_owned(),
					executable_sha256: "d".repeat(64),
					schema_sha256: "e".repeat(64),
					callback_profile_sha256: "f".repeat(64),
					login_chatgpt_auth_tokens: true,
					refresh_callback: true,
				})
				.await
				.expect("attest cross-Route callback capability")
		);

		let response = service
			.route_account_command(
				lease,
				AccountOperationId::new("22000000-0000-4000-8000-000000000053")
					.expect("Route operation"),
				&target_account_id,
				1,
				routing.revision,
				false,
				|result| {
					Ok(match result {
						Ok(AccountRouteResult::Committed(commit)) => json!({
							"account_revision": commit.account.revision,
							"routing_revision": commit.routing.revision,
						}),
						Ok(AccountRouteResult::Pending(_)) | Err(_) => {
							json!({"outcome": "unexpected"})
						},
					})
				},
			)
			.await
			.expect("complete cross-account Route");

		assert_eq!(response["account_revision"], 2);
		assert_eq!(response["routing_revision"], routing.revision + 1);
		assert_eq!(shared_auth_file.projections.load(Ordering::Relaxed), 1);
		let source = store
			.read_account_registry(Some(&source_account_id), 1)
			.await
			.expect("read source account")
			.pop()
			.expect("source account");
		let source_binding = source.credential.expect("source binding");
		let source_stored = credentials
			.read_exact(&source_account_id, &source_binding)
			.expect("read preserved source credential");
		assert_eq!(source.revision, 2);
		assert_eq!(source_stored.bundle().access_token(), "source-rotated-access");
	}

	#[tokio::test]
	async fn live_cross_account_route_waits_for_quiescence_then_follows_same_account_rotation() {
		let directory = tempdir().expect("temporary product root");
		let root = DecodexRoot::new(fs::canonicalize(directory.path()).expect("canonical root"))
			.expect("typed product root");
		let store = SqliteStore::open(&root.paths()).expect("open product store");
		let credentials: Arc<dyn HostCredentialStore> =
			Arc::new(SqliteCredentialStore::new(store.clone()));
		let _source_account_id = enroll_route_test_account(
			&store,
			&credentials,
			"21000000-0000-4000-8000-000000000071",
			"22000000-0000-4000-8000-000000000071",
			"pending-source-provider",
			"pending-source-access",
		)
		.await;
		let account_id = enroll_route_test_account(
			&store,
			&credentials,
			"21000000-0000-4000-8000-000000000073",
			"22000000-0000-4000-8000-000000000073",
			"pending-target-provider",
			"pending-target-access",
		)
		.await;
		let routing = store.read_account_routing_control().await.expect("read routing");
		let request = format!(
			r#"{{"name":"route_account","arguments":{{"operation_id":"22000000-0000-4000-8000-000000000072","account_id":"{}","expected_account_revision":1}}}}"#,
			account_id.as_str(),
		);
		let command = CommandIdentity::new("pending-route", request.as_bytes())
			.expect("Route command identity");
		let lease = match store
			.reserve_account_route_command(&command, routing.revision, &request)
			.await
			.expect("reserve Route")
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Pending(_) | AccountCommandReceiptClaim::Replayed(_) => {
				panic!("new Route replayed")
			},
		};
		let shared_auth_file = Arc::new(RefreshRaceSharedAuthFile::new(
			"pending-source-provider",
			shared_bundle("pending-source-provider", "pending-source-access", 2_000_000),
		));
		let running = Arc::new(AtomicBool::new(true));
		let coordinator = test_coordinator_with_liveness(
			Arc::clone(&shared_auth_file) as Arc<dyn SharedAuthFilePort>,
			Arc::new(MutableLiveness(Arc::clone(&running))),
		);
		let provider_calls = Arc::new(AtomicUsize::new(0));
		let service = AccountService::new(
			store.clone(),
			Arc::clone(&credentials),
			Arc::new(ScriptedCredentialRefresher {
				calls: Arc::clone(&provider_calls),
				results: Mutex::new(VecDeque::from([CredentialRefreshResult {
					returned_provider: ProviderIdentity::new(
						AccountProvider::Chatgpt,
						"pending-target-provider",
					)
					.expect("target provider"),
					bundle: shared_bundle_with_refresh(
						"pending-target-provider",
						"target-refreshed-access",
						"target-refreshed-refresh",
						i64::MAX / 2,
					),
				}])),
			}),
		)
		.with_shared_auth_coordinator(coordinator);
		assert!(
			service
				.attest_callback_capability(CodexAccountCapabilityAttestation {
					build_identity: "pending-route-build".to_owned(),
					executable_sha256: "7".repeat(64),
					schema_sha256: "8".repeat(64),
					callback_profile_sha256: "9".repeat(64),
					login_chatgpt_auth_tokens: true,
					refresh_callback: true,
				})
				.await
				.expect("attest callback")
		);
		let operation_id = AccountOperationId::new("22000000-0000-4000-8000-000000000072")
			.expect("Route operation");
		let response_builder = |result| {
			Ok(match result {
				Ok(AccountRouteResult::Pending(pending)) => {
					let wait_reason = match pending.wait_reason {
						AccountRouteWaitReason::ExternalCodex { blockers, omitted } => json!({
							"reason": "external_codex",
							"pids": blockers.into_iter().map(|blocker| blocker.pid).collect::<Vec<_>>(),
							"omitted": omitted,
						}),
						_ => json!({"reason": "unexpected"}),
					};
					json!({
						"outcome": "pending",
						"operation_id": pending.operation_id.as_str(),
						"account_id": pending.account_id.as_str(),
						"routing_revision": pending.routing_revision,
						"wait_reason": wait_reason,
					})
				},
				Ok(AccountRouteResult::Committed(commit)) => json!({
					"outcome": "routed",
					"routing_revision": commit.routing.revision,
				}),
				Err(_) => json!({"outcome": "unexpected"}),
			})
		};
		let pending_response = service
			.route_account_command(
				lease,
				operation_id.clone(),
				&account_id,
				1,
				routing.revision,
				false,
				response_builder,
			)
			.await
			.expect("defer Route");
		assert_eq!(pending_response["outcome"], "pending");
		assert_eq!(pending_response["wait_reason"]["reason"], "external_codex");
		assert_eq!(pending_response["wait_reason"]["pids"], json!([44_768]));
		time::timeout(Duration::from_millis(10), service.pending_route_notified())
			.await
			.expect("Pending Route wakes the recovery loop immediately");
		assert_eq!(shared_auth_file.project_attempts.load(Ordering::Relaxed), 0);
		assert_eq!(provider_calls.load(Ordering::Relaxed), 0);
		assert_eq!(shared_auth_file.current_tokens().0, "pending-source-access");
		assert!(matches!(
			store
				.reserve_account_route_command(&command, routing.revision, &request)
				.await
				.expect("replay pending Route"),
			AccountCommandReceiptClaim::Pending(value) if value == pending_response
		));

		running.store(false, Ordering::Relaxed);
		time::sleep(Duration::from_millis(2)).await;
		let pending = store
			.read_pending_account_route_commands(1)
			.await
			.expect("read pending Route")
			.pop()
			.expect("pending Route");
		let lease = match store
			.reclaim_account_route_command(&pending)
			.await
			.expect("reclaim pending Route")
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Pending(_) | AccountCommandReceiptClaim::Replayed(_) => {
				panic!("pending Route did not reclaim")
			},
		};
		let routed = service
			.route_account_command(
				lease,
				operation_id,
				&account_id,
				1,
				routing.revision,
				true,
				response_builder,
			)
			.await
			.expect("complete Route");
		assert_eq!(routed["outcome"], "routed");
		assert_eq!(shared_auth_file.project_attempts.load(Ordering::Relaxed), 1);
		assert_eq!(provider_calls.load(Ordering::Relaxed), 1);
		assert_eq!(
			shared_auth_file.current_tokens(),
			("target-refreshed-access".to_owned(), "target-refreshed-refresh".to_owned())
		);
		assert!(store.read_pending_account_route_commands(1).await.unwrap().is_empty());
		assert_eq!(
			store.read_account_routing_control().await.unwrap().mode,
			AccountSelectionMode::Fixed(account_id.clone())
		);

		shared_auth_file.replace_from_codex(shared_bundle_with_refresh(
			"pending-target-provider",
			"codex-rotated-access",
			"codex-rotated-refresh",
			i64::MAX / 2,
		));
		assert!(service.follow_shared_auth_once().await.unwrap().is_none());
		let followed = service
			.follow_shared_auth_once()
			.await
			.unwrap()
			.expect("stable same-account rotation is adopted");
		assert_eq!(followed.account_id, account_id);
		assert_eq!(followed.revision, 3);
		let followed_binding = followed.credential.expect("followed credential binding");
		let followed_stored = credentials
			.read_exact(&followed.account_id, &followed_binding)
			.expect("read followed credential");
		assert_eq!(followed_stored.bundle().access_token(), "codex-rotated-access");
		assert_eq!(followed_stored.bundle().refresh_token(), "codex-rotated-refresh");
		assert_eq!(provider_calls.load(Ordering::Relaxed), 1);
	}

	#[tokio::test]
	async fn restarted_pending_route_waits_for_callback_readiness_then_converges_if_auth_is_current()
	 {
		let directory = tempdir().expect("temporary product root");
		let root = DecodexRoot::new(fs::canonicalize(directory.path()).expect("canonical root"))
			.expect("typed product root");
		let store = SqliteStore::open(&root.paths()).expect("open product store");
		let credentials: Arc<dyn HostCredentialStore> =
			Arc::new(SqliteCredentialStore::new(store.clone()));
		let target = enroll_route_test_account(
			&store,
			&credentials,
			"21000000-0000-4000-8000-0000000000c1",
			"22000000-0000-4000-8000-0000000000c1",
			"restart-current-provider",
			"restart-current-access",
		)
		.await;
		let routing = store.read_account_routing_control().await.unwrap();
		let operation = "22000000-0000-4000-8000-0000000000c2";
		let request = format!(
			r#"{{"name":"route_account","arguments":{{"operation_id":"{operation}","account_id":"{}","expected_account_revision":1}}}}"#,
			target.as_str(),
		);
		let command = CommandIdentity::new("restart-pending-route", request.as_bytes()).unwrap();
		let lease = match store
			.reserve_account_route_command(&command, routing.revision, &request)
			.await
			.unwrap()
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Pending(_) | AccountCommandReceiptClaim::Replayed(_) => {
				panic!("new Route replayed")
			},
		};
		store.defer_account_route_command(lease, &json!({"outcome": "pending"})).await.unwrap();
		let shared_auth_file = Arc::new(RouteSharedAuthFile {
			source_provider: "restart-current-provider",
			source_access: "restart-current-access",
			source_expiry: 2_000_000,
			target_provider: "restart-current-provider",
			target_access: "restart-current-access",
			projections: AtomicUsize::new(0),
		});
		let service = Arc::new(
			AccountService::new(
				store.clone(),
				Arc::clone(&credentials),
				Arc::new(UnusedCredentialRefresher),
			)
			.with_shared_auth_coordinator(test_coordinator(
				Arc::clone(&shared_auth_file) as Arc<dyn SharedAuthFilePort>,
				CodexLiveness::MayBeRunning,
			)),
		);
		assert_eq!(
			service.pending_route_wait_reason(&target).await,
			AccountRouteWaitReason::AccountReadiness(
				AccountLifecycleReadiness::CallbackCapabilityUnready
			)
		);
		time::sleep(Duration::from_millis(2)).await;
		let _ = crate::application::recover_pending_account_routes_once(
			Arc::clone(&service),
			&store,
			None,
		)
		.await;
		assert_eq!(store.read_pending_account_route_commands(1).await.unwrap().len(), 1);

		assert!(
			service
				.attest_callback_capability(CodexAccountCapabilityAttestation {
					build_identity: "restart-route-build".to_owned(),
					executable_sha256: "a".repeat(64),
					schema_sha256: "b".repeat(64),
					callback_profile_sha256: "c".repeat(64),
					login_chatgpt_auth_tokens: true,
					refresh_callback: true,
				})
				.await
				.unwrap()
		);
		let _ =
			crate::application::recover_pending_account_routes_once(service, &store, None).await;
		assert!(store.read_pending_account_route_commands(1).await.unwrap().is_empty());
		assert_eq!(
			store.read_account_routing_control().await.unwrap().mode,
			AccountSelectionMode::Fixed(target)
		);
		assert_eq!(shared_auth_file.projections.load(Ordering::Relaxed), 0);
	}

	#[tokio::test]
	async fn newer_pending_route_replaces_the_target_without_a_shared_auth_write() {
		let directory = tempdir().expect("temporary product root");
		let root = DecodexRoot::new(fs::canonicalize(directory.path()).expect("canonical root"))
			.expect("typed product root");
		let store = SqliteStore::open(&root.paths()).expect("open product store");
		let credentials: Arc<dyn HostCredentialStore> =
			Arc::new(SqliteCredentialStore::new(store.clone()));
		let _source = enroll_route_test_account(
			&store,
			&credentials,
			"21000000-0000-4000-8000-0000000000d1",
			"22000000-0000-4000-8000-0000000000d1",
			"replace-source-provider",
			"replace-source-access",
		)
		.await;
		let first_target = enroll_route_test_account(
			&store,
			&credentials,
			"21000000-0000-4000-8000-0000000000d2",
			"22000000-0000-4000-8000-0000000000d2",
			"replace-first-provider",
			"replace-first-access",
		)
		.await;
		let second_target = enroll_route_test_account(
			&store,
			&credentials,
			"21000000-0000-4000-8000-0000000000d3",
			"22000000-0000-4000-8000-0000000000d3",
			"replace-second-provider",
			"replace-second-access",
		)
		.await;
		let routing = store.read_account_routing_control().await.unwrap();
		let shared_auth_file = Arc::new(RouteSharedAuthFile {
			source_provider: "replace-source-provider",
			source_access: "replace-source-access",
			source_expiry: 2_000_000,
			target_provider: "replace-second-provider",
			target_access: "target-refreshed-access",
			projections: AtomicUsize::new(0),
		});
		let service = AccountService::new(
			store.clone(),
			Arc::clone(&credentials),
			Arc::new(RouteCredentialRefresher),
		)
		.with_shared_auth_coordinator(test_coordinator(
			Arc::clone(&shared_auth_file) as Arc<dyn SharedAuthFilePort>,
			CodexLiveness::MayBeRunning,
		));
		assert!(
			service
				.attest_callback_capability(CodexAccountCapabilityAttestation {
					build_identity: "replace-route-build".to_owned(),
					executable_sha256: "d".repeat(64),
					schema_sha256: "e".repeat(64),
					callback_profile_sha256: "f".repeat(64),
					login_chatgpt_auth_tokens: true,
					refresh_callback: true,
				})
				.await
				.unwrap()
		);
		let pending_response = |result| {
			Ok(match result {
				Ok(AccountRouteResult::Pending(pending)) => json!({
					"outcome": "pending",
					"account_id": pending.account_id.as_str(),
				}),
				_ => json!({"outcome": "unexpected"}),
			})
		};
		let first_operation = "22000000-0000-4000-8000-0000000000d4";
		let first_request = format!(
			r#"{{"name":"route_account","arguments":{{"operation_id":"{first_operation}","account_id":"{}","expected_account_revision":1}}}}"#,
			first_target.as_str(),
		);
		let first_command =
			CommandIdentity::new("replace-first-route", first_request.as_bytes()).unwrap();
		let first_lease = match store
			.reserve_account_route_command(&first_command, routing.revision, &first_request)
			.await
			.unwrap()
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			_ => panic!("first Route replayed"),
		};
		let first_result = service
			.route_account_command(
				first_lease,
				AccountOperationId::new(first_operation).unwrap(),
				&first_target,
				1,
				routing.revision,
				false,
				pending_response,
			)
			.await
			.unwrap();
		assert_eq!(first_result["account_id"], first_target.as_str());

		let second_operation = "22000000-0000-4000-8000-0000000000d5";
		let second_request = format!(
			r#"{{"name":"route_account","arguments":{{"operation_id":"{second_operation}","account_id":"{}","expected_account_revision":1}}}}"#,
			second_target.as_str(),
		);
		let second_command =
			CommandIdentity::new("replace-second-route", second_request.as_bytes()).unwrap();
		let superseded = json!({"outcome": "rejected", "reason": "route_superseded"});
		let second_lease = match store
			.reserve_replacing_account_route_command(
				&second_command,
				routing.revision,
				&second_request,
				&superseded,
			)
			.await
			.unwrap()
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			_ => panic!("second Route replayed"),
		};
		assert_eq!(shared_auth_file.projections.load(Ordering::Relaxed), 0);
		let second_result = service
			.route_account_command(
				second_lease,
				AccountOperationId::new(second_operation).unwrap(),
				&second_target,
				1,
				routing.revision,
				false,
				pending_response,
			)
			.await
			.unwrap();
		assert_eq!(second_result["account_id"], second_target.as_str());
		assert_eq!(shared_auth_file.projections.load(Ordering::Relaxed), 0);
		assert!(matches!(
			store
				.reserve_account_route_command(&first_command, routing.revision, &first_request)
				.await
				.unwrap(),
			AccountCommandReceiptClaim::Replayed(value) if value == superseded
		));
		let pending = store.read_pending_account_route_commands(8).await.unwrap();
		assert_eq!(pending.len(), 1);
		assert_eq!(pending[0].idempotency_key, "replace-second-route");
	}

	#[tokio::test]
	async fn live_codex_does_not_delay_terminal_pending_route_routing_drift() {
		let directory = tempdir().expect("temporary product root");
		let root = DecodexRoot::new(fs::canonicalize(directory.path()).expect("canonical root"))
			.expect("typed product root");
		let store = SqliteStore::open(&root.paths()).expect("open product store");
		let credentials: Arc<dyn HostCredentialStore> =
			Arc::new(SqliteCredentialStore::new(store.clone()));
		let target = enroll_route_test_account(
			&store,
			&credentials,
			"21000000-0000-4000-8000-0000000000b1",
			"22000000-0000-4000-8000-0000000000b1",
			"drift-target-provider",
			"drift-target-access",
		)
		.await;
		let routing = store.read_account_routing_control().await.unwrap();
		let operation = "22000000-0000-4000-8000-0000000000b2";
		let request = format!(
			r#"{{"name":"route_account","arguments":{{"operation_id":"{operation}","account_id":"{}","expected_account_revision":1}}}}"#,
			target.as_str(),
		);
		let command = CommandIdentity::new("drifted-pending-route", request.as_bytes()).unwrap();
		let lease = match store
			.reserve_account_route_command(&command, routing.revision, &request)
			.await
			.unwrap()
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Pending(_) | AccountCommandReceiptClaim::Replayed(_) => {
				panic!("new Route replayed")
			},
		};
		store.defer_account_route_command(lease, &json!({"outcome": "pending"})).await.unwrap();
		enroll_route_test_account(
			&store,
			&credentials,
			"21000000-0000-4000-8000-0000000000b3",
			"22000000-0000-4000-8000-0000000000b3",
			"drift-other-provider",
			"drift-other-access",
		)
		.await;
		assert_ne!(store.read_account_routing_control().await.unwrap().revision, routing.revision);

		let shared_auth_file = Arc::new(RouteSharedAuthFile {
			source_provider: "drift-target-provider",
			source_access: "drift-target-access",
			source_expiry: 2_000_000,
			target_provider: "drift-target-provider",
			target_access: "drift-target-access",
			projections: AtomicUsize::new(0),
		});
		let service = Arc::new(
			AccountService::new(
				store.clone(),
				Arc::clone(&credentials),
				Arc::new(UnusedCredentialRefresher),
			)
			.with_shared_auth_coordinator(test_coordinator(
				Arc::clone(&shared_auth_file) as Arc<dyn SharedAuthFilePort>,
				CodexLiveness::MayBeRunning,
			)),
		);
		time::sleep(Duration::from_millis(2)).await;
		let _ =
			crate::application::recover_pending_account_routes_once(service, &store, None).await;

		assert!(store.read_pending_account_route_commands(1).await.unwrap().is_empty());
		assert_eq!(shared_auth_file.projections.load(Ordering::Relaxed), 0);
	}

	#[tokio::test]
	#[allow(clippy::too_many_lines)] // The precommitted refresh fixture and receipt readback form one crash-window proof.
	async fn committed_refresh_replay_mirrors_its_successor_without_provider_work() {
		let directory = tempdir().expect("temporary product root");
		let root = DecodexRoot::new(fs::canonicalize(directory.path()).expect("canonical root"))
			.expect("typed product root");
		let store = SqliteStore::open(&root.paths()).expect("open product store");
		let credentials: Arc<dyn HostCredentialStore> =
			Arc::new(SqliteCredentialStore::new(store.clone()));
		let provider = ProviderIdentity::new(AccountProvider::Chatgpt, "receipt-provider-account")
			.expect("provider identity");
		let account_id =
			AccountId::new("21000000-0000-4000-8000-000000000031").expect("account identity");
		let enrollment_operation = AccountOperationId::new("22000000-0000-4000-8000-000000000031")
			.expect("enrollment operation");
		let initial_bundle = shared_bundle(provider.account_id(), "initial-access", 2_000_000);
		let shared_auth_file =
			Arc::new(RefreshRaceSharedAuthFile::new(provider.account_id(), initial_bundle.clone()));
		let initial_binding = initial_bundle
			.binding_for(
				&account_id,
				&enrollment_operation,
				CredentialVersion::new(1).expect("initial version"),
				&provider,
			)
			.expect("initial binding");
		accepted_phase(
			store
				.prepare_account_operation(&AccountOperationPreparation {
					operation_id: enrollment_operation.clone(),
					account_id: account_id.clone(),
					kind: AccountOperationKind::Enroll,
					display_label: Some(stable_account_alias(&provider)),
					enabled: Some(true),
					expected_account_revision: None,
					expected: None,
					target: Some(initial_binding.clone()),
					provider: provider.clone(),
				})
				.await
				.expect("prepare enrollment"),
		)
		.expect("accept enrollment preparation");
		credentials
			.create(&account_id, &initial_binding, initial_bundle)
			.expect("create initial credential");
		accepted_phase(
			store
				.advance_account_operation(
					&enrollment_operation,
					AccountOperationPhase::Prepared,
					AccountOperationPhase::StoreApplied,
					None,
				)
				.await
				.expect("record enrollment store effect"),
		)
		.expect("accept enrollment store effect");
		accepted_phase(
			store
				.advance_account_operation(
					&enrollment_operation,
					AccountOperationPhase::StoreApplied,
					AccountOperationPhase::Committed,
					None,
				)
				.await
				.expect("commit enrollment"),
		)
		.expect("accept enrollment commit");

		let refresh_operation = AccountOperationId::new("22000000-0000-4000-8000-000000000032")
			.expect("refresh operation");
		let refreshed_bundle = shared_bundle(provider.account_id(), "refreshed-access", 3_000_000);
		let refreshed_binding = refreshed_bundle
			.binding_for(
				&account_id,
				&refresh_operation,
				CredentialVersion::new(2).expect("refreshed version"),
				&provider,
			)
			.expect("refreshed binding");
		accepted_phase(
			store
				.prepare_account_operation(&AccountOperationPreparation {
					operation_id: refresh_operation.clone(),
					account_id: account_id.clone(),
					kind: AccountOperationKind::Refresh,
					display_label: None,
					enabled: None,
					expected_account_revision: Some(1),
					expected: Some(initial_binding.clone()),
					target: None,
					provider: provider.clone(),
				})
				.await
				.expect("prepare refresh"),
		)
		.expect("accept refresh preparation");
		accepted_phase(
			store
				.advance_account_operation(
					&refresh_operation,
					AccountOperationPhase::Prepared,
					AccountOperationPhase::ProviderEffectPending,
					None,
				)
				.await
				.expect("record provider effect boundary"),
		)
		.expect("accept provider effect boundary");
		accepted_phase(
			store
				.set_account_operation_target(&refresh_operation, &refreshed_binding)
				.await
				.expect("record refresh target"),
		)
		.expect("accept refresh target");
		credentials
			.compare_and_swap_rotate(
				&account_id,
				&initial_binding,
				&refreshed_binding,
				refreshed_bundle,
			)
			.expect("persist refreshed credential");
		accepted_phase(
			store
				.advance_account_operation(
					&refresh_operation,
					AccountOperationPhase::ProviderEffectPending,
					AccountOperationPhase::StoreApplied,
					None,
				)
				.await
				.expect("record refresh store effect"),
		)
		.expect("accept refresh store effect");

		let command = CommandIdentity::new("refresh-reprojection-receipt", b"exact refresh replay")
			.expect("command identity");
		let lease = match store
			.reserve_account_command(
				&command,
				AccountCommandKind::Refresh,
				account_id.as_str(),
				Some(1),
			)
			.await
			.expect("reserve refresh command")
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Pending(_) | AccountCommandReceiptClaim::Replayed(_) => {
				panic!("new refresh command replayed")
			},
		};
		let service = AccountService::new(
			store.clone(),
			Arc::clone(&credentials),
			Arc::new(UnusedCredentialRefresher),
		)
		.with_shared_auth_coordinator(test_coordinator(
			Arc::clone(&shared_auth_file) as Arc<dyn SharedAuthFilePort>,
			CodexLiveness::MayBeRunning,
		));
		let response = service
			.refresh_command(lease, refresh_operation.clone(), &account_id, 1, |result| {
				Ok(match result {
					Ok(account) => json!({
						"outcome": "succeeded",
						"account_revision": account.revision,
					}),
					Err(_) => json!({"outcome": "unexpected"}),
				})
			})
			.await
			.expect("complete committed refresh command");

		assert_eq!(response, json!({"outcome": "succeeded", "account_revision": 2}));
		let refreshed = service.inspect(&account_id).await.expect("read refreshed account").account;
		assert_eq!(refreshed.revision, 2);
		assert_eq!(refreshed.credential.as_ref(), Some(&refreshed_binding));
		assert_eq!(shared_auth_file.current_tokens().0, "refreshed-access");
		assert_eq!(shared_auth_file.project_attempts.load(Ordering::Relaxed), 1);
		let operation = store
			.read_account_operation(&refresh_operation)
			.await
			.expect("read refresh operation")
			.expect("refresh operation exists");
		assert_eq!(operation.phase, AccountOperationPhase::Committed);
		match store
			.reserve_account_command(
				&command,
				AccountCommandKind::Refresh,
				account_id.as_str(),
				Some(1),
			)
			.await
			.expect("replay refresh command")
		{
			AccountCommandReceiptClaim::Replayed(replayed) => assert_eq!(replayed, response),
			AccountCommandReceiptClaim::Owned(_) | AccountCommandReceiptClaim::Pending(_) => {
				panic!("committed refresh receipt was not terminal")
			},
		}
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

	#[tokio::test]
	async fn device_login_enrollment_commits_one_credential_bound_account_and_replays_its_receipt()
	{
		let directory = tempdir().expect("temporary product root");
		let root = DecodexRoot::new(fs::canonicalize(directory.path()).expect("canonical root"))
			.expect("typed product root");
		let store = SqliteStore::open(&root.paths()).expect("open product store");
		let credentials: Arc<dyn HostCredentialStore> =
			Arc::new(SqliteCredentialStore::new(store.clone()));
		let service = AccountService::new(
			store.clone(),
			Arc::clone(&credentials),
			Arc::new(UnusedCredentialRefresher),
		);
		let account_id =
			AccountId::new("21000000-0000-4000-8000-000000000008").expect("new account");
		let operation_id = AccountOperationId::new("22000000-0000-4000-8000-000000000008")
			.expect("enrollment operation");
		let command = CommandIdentity::new("device-login-enroll", b"exact device enrollment")
			.expect("command identity");
		let lease = match store
			.reserve_account_command(
				&command,
				AccountCommandKind::Enroll,
				account_id.as_str(),
				None,
			)
			.await
			.expect("reserve enrollment command")
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Pending(_) | AccountCommandReceiptClaim::Replayed(_) => {
				panic!("new enrollment command replayed")
			},
		};
		let (_source_directory, source_descriptor) =
			owner_private_shared_codex_auth("device-login-provider", "device-login@example.test");
		let response = service
			.enroll_from_credential_file_command(
				lease,
				operation_id.clone(),
				account_id.clone(),
				true,
				&source_descriptor,
				|result| {
					Ok(match result {
						Ok(account) => json!({
							"outcome": "succeeded",
							"account_revision": account.revision,
						}),
						Err(_) => json!({"outcome": "unexpected"}),
					})
				},
			)
			.await
			.expect("complete device-login enrollment");

		assert_eq!(response, json!({"outcome": "succeeded", "account_revision": 1}));
		let accounts = service.list().await.expect("list enrolled account");
		assert_eq!(accounts.len(), 1);
		assert_eq!(accounts[0].account.account_id, account_id);
		assert_eq!(accounts[0].readiness, AccountLifecycleReadiness::CallbackCapabilityUnready);
		let binding = accounts[0]
			.account
			.credential
			.as_ref()
			.expect("enrollment commits a credential binding");
		assert_eq!(binding.provider.account_id(), "device-login-provider");
		credentials
			.read_exact(&account_id, binding)
			.expect("daemon credential store owns the enrolled bundle");
		let operation = store
			.read_account_operation(&operation_id)
			.await
			.expect("read enrollment operation")
			.expect("enrollment operation remains recorded");
		assert_eq!(operation.kind, AccountOperationKind::Enroll);
		assert_eq!(operation.phase, AccountOperationPhase::Committed);
		match store
			.reserve_account_command(
				&command,
				AccountCommandKind::Enroll,
				account_id.as_str(),
				None,
			)
			.await
			.expect("replay enrollment command")
		{
			AccountCommandReceiptClaim::Replayed(replayed) => assert_eq!(replayed, response),
			AccountCommandReceiptClaim::Owned(_) | AccountCommandReceiptClaim::Pending(_) => {
				panic!("completed enrollment did not replay")
			},
		}
	}

	#[tokio::test]
	#[allow(clippy::too_many_lines)] // The existing-provider setup and exact durable rejection remain one end-to-end boundary.
	async fn duplicate_provider_enrollment_is_cancelled_and_replays_its_typed_receipt() {
		let directory = tempdir().expect("temporary product root");
		fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
			.expect("private product root");
		let root = DecodexRoot::new(fs::canonicalize(directory.path()).expect("canonical root"))
			.expect("typed product root");
		let store = SqliteStore::open(&root.paths()).expect("open product store");
		let credentials: Arc<dyn HostCredentialStore> =
			Arc::new(SqliteCredentialStore::new(store.clone()));
		let service = AccountService::new(
			store.clone(),
			Arc::clone(&credentials),
			Arc::new(UnusedCredentialRefresher),
		);
		let provider =
			ProviderIdentity::new(AccountProvider::Chatgpt, "duplicate-provider-account")
				.expect("provider identity");
		let existing_account =
			AccountId::new("21000000-0000-4000-8000-000000000010").expect("existing account");
		let existing_operation = AccountOperationId::new("22000000-0000-4000-8000-000000000010")
			.expect("existing operation");
		let existing_bundle = shared_bundle(provider.account_id(), "existing-access", 3_000_000);
		let existing_binding = existing_bundle
			.binding_for(
				&existing_account,
				&existing_operation,
				CredentialVersion::new(1).expect("initial version"),
				&provider,
			)
			.expect("existing binding");
		assert!(matches!(
			store
				.prepare_account_operation(&AccountOperationPreparation {
					operation_id: existing_operation.clone(),
					account_id: existing_account.clone(),
					kind: AccountOperationKind::Enroll,
					display_label: Some(stable_account_alias(&provider)),
					enabled: Some(true),
					expected_account_revision: None,
					expected: None,
					target: Some(existing_binding.clone()),
					provider: provider.clone(),
				})
				.await
				.expect("prepare existing enrollment"),
			AccountLifecycleMutationOutcome::Applied(_)
		));
		credentials
			.create(&existing_account, &existing_binding, existing_bundle)
			.expect("write existing credential");
		store
			.advance_account_operation(
				&existing_operation,
				AccountOperationPhase::Prepared,
				AccountOperationPhase::StoreApplied,
				None,
			)
			.await
			.expect("record existing credential");
		store
			.advance_account_operation(
				&existing_operation,
				AccountOperationPhase::StoreApplied,
				AccountOperationPhase::Committed,
				None,
			)
			.await
			.expect("commit existing account");

		let account_id =
			AccountId::new("21000000-0000-4000-8000-000000000011").expect("new account");
		let operation_id =
			AccountOperationId::new("22000000-0000-4000-8000-000000000011").expect("new operation");
		let command = CommandIdentity::new("duplicate-provider-enroll", b"exact duplicate request")
			.expect("command identity");
		let lease = match store
			.reserve_account_command(
				&command,
				AccountCommandKind::Enroll,
				account_id.as_str(),
				None,
			)
			.await
			.expect("reserve enrollment command")
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Pending(_) | AccountCommandReceiptClaim::Replayed(_) => {
				panic!("new enrollment command replayed")
			},
		};
		let (_source_directory, source_descriptor) =
			owner_private_shared_codex_auth(provider.account_id(), "duplicate@example.test");
		let response = service
			.enroll_from_credential_file_command(
				lease,
				operation_id.clone(),
				account_id.clone(),
				true,
				&source_descriptor,
				|result| {
					Ok(match result {
						Err(AccountLifecycleError::CredentialStore(
							CredentialStoreError::DuplicateProvider,
						)) => json!({
							"outcome": "rejected",
							"rejection": "provider_already_enrolled",
						}),
						_ => json!({"outcome": "unexpected"}),
					})
				},
			)
			.await
			.expect("complete duplicate enrollment");

		assert_eq!(
			response,
			json!({"outcome": "rejected", "rejection": "provider_already_enrolled"})
		);
		assert!(matches!(
			credentials.read_exact(&account_id, &existing_binding),
			Err(CredentialStoreError::NotFound)
		));
		let operation = store
			.read_account_operation(&operation_id)
			.await
			.expect("read duplicate operation")
			.expect("duplicate operation remains recorded");
		assert_eq!(operation.phase, AccountOperationPhase::Cancelled);
		match store
			.reserve_account_command(
				&command,
				AccountCommandKind::Enroll,
				account_id.as_str(),
				None,
			)
			.await
			.expect("replay enrollment command")
		{
			AccountCommandReceiptClaim::Replayed(replayed) => assert_eq!(replayed, response),
			AccountCommandReceiptClaim::Owned(_) | AccountCommandReceiptClaim::Pending(_) => {
				panic!("completed enrollment did not replay")
			},
		}
		credentials
			.read_exact(&existing_account, &existing_binding)
			.expect("existing credential remains authoritative");
		assert_eq!(service.list().await.expect("list accounts").len(), 1);
	}

	#[tokio::test]
	#[allow(clippy::too_many_lines)] // One regression proves legacy cleanup, restoration, replay, routing, and reopen together.
	async fn logged_out_provider_enrollment_restores_the_original_account_and_receipt() {
		let directory = tempdir().expect("temporary product root");
		fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
			.expect("private product root");
		let root = DecodexRoot::new(fs::canonicalize(directory.path()).expect("canonical root"))
			.expect("typed product root");
		let store = SqliteStore::open(&root.paths()).expect("open product store");
		let credentials: Arc<dyn HostCredentialStore> =
			Arc::new(SqliteCredentialStore::new(store.clone()));
		let service = AccountService::new(
			store.clone(),
			Arc::clone(&credentials),
			Arc::new(UnusedCredentialRefresher),
		);
		let provider = ProviderIdentity::new(AccountProvider::Chatgpt, "restored-provider-account")
			.expect("provider identity");
		let original_account =
			AccountId::new("21000000-0000-4000-8000-000000000020").expect("original account");
		let enrollment_operation = AccountOperationId::new("22000000-0000-4000-8000-000000000020")
			.expect("enrollment operation");
		let enrollment_command =
			CommandIdentity::new("restore-provider-enroll", b"initial enrollment")
				.expect("enrollment command");
		let enrollment_lease = match store
			.reserve_account_command(
				&enrollment_command,
				AccountCommandKind::Enroll,
				original_account.as_str(),
				None,
			)
			.await
			.expect("reserve initial enrollment")
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Pending(_) | AccountCommandReceiptClaim::Replayed(_) => {
				panic!("initial enrollment replayed")
			},
		};
		let (_initial_source, initial_descriptor) =
			owner_private_shared_codex_auth(provider.account_id(), "initial@example.test");
		let initial = service
			.enroll_from_credential_file_command(
				enrollment_lease,
				enrollment_operation,
				original_account.clone(),
				true,
				&initial_descriptor,
				|result| {
					Ok(match result {
						Ok(account) => json!({
							"outcome": "succeeded",
							"account_id": account.account_id.as_str(),
							"account_revision": account.revision,
						}),
						Err(_) => json!({"outcome": "unexpected"}),
					})
				},
			)
			.await
			.expect("complete initial enrollment");
		assert_eq!(
			initial,
			json!({
				"outcome": "succeeded",
				"account_id": original_account.as_str(),
				"account_revision": 1,
			})
		);

		let logout_operation = AccountOperationId::new("22000000-0000-4000-8000-000000000021")
			.expect("logout operation");
		let tombstone = service
			.logout(logout_operation, &original_account, 1)
			.await
			.expect("logout original account");
		assert!(tombstone.tombstoned);
		assert_eq!(tombstone.revision, 2);
		assert!(service.list().await.expect("list after logout").is_empty());

		let legacy_account =
			AccountId::new("21000000-0000-4000-8000-000000000022").expect("legacy account");
		let legacy_operation = AccountOperationId::new("22000000-0000-4000-8000-000000000022")
			.expect("legacy operation");
		let legacy_bundle = shared_bundle(provider.account_id(), "legacy-access", 3_000_000);
		let legacy_target = legacy_bundle
			.binding_for(
				&legacy_account,
				&legacy_operation,
				CredentialVersion::new(1).expect("legacy version"),
				&provider,
			)
			.expect("legacy target");
		assert!(matches!(
			store
				.prepare_account_operation(&AccountOperationPreparation {
					operation_id: legacy_operation.clone(),
					account_id: legacy_account.clone(),
					kind: AccountOperationKind::Enroll,
					display_label: Some(stable_account_alias(&provider)),
					enabled: Some(true),
					expected_account_revision: None,
					expected: None,
					target: Some(legacy_target.clone()),
					provider: provider.clone(),
				})
				.await
				.expect("prepare legacy collision"),
			AccountLifecycleMutationOutcome::Applied(_)
		));
		credentials
			.create(&legacy_account, &legacy_target, legacy_bundle)
			.expect("write legacy credential");
		store
			.advance_account_operation(
				&legacy_operation,
				AccountOperationPhase::Prepared,
				AccountOperationPhase::StoreApplied,
				None,
			)
			.await
			.expect("record legacy store effect");
		let startup = service.reconcile_startup().await.expect("reconcile legacy collision");
		assert_eq!(startup.cancelled, 1);
		assert_eq!(startup.committed, 0);
		assert!(startup.manual_recovery.is_empty());
		assert!(matches!(
			credentials.read_exact(&legacy_account, &legacy_target),
			Err(CredentialStoreError::NotFound)
		));
		let legacy = store
			.read_account_operation(&legacy_operation)
			.await
			.expect("read legacy collision")
			.expect("legacy collision is retained");
		assert_eq!(legacy.phase, AccountOperationPhase::Cancelled);

		let requested_account =
			AccountId::new("21000000-0000-4000-8000-000000000023").expect("requested account");
		let restore_operation = AccountOperationId::new("22000000-0000-4000-8000-000000000023")
			.expect("restore operation");
		let restore_command =
			CommandIdentity::new("restore-provider-reenroll", b"restore enrollment")
				.expect("restore command");
		let restore_lease = match store
			.reserve_account_command(
				&restore_command,
				AccountCommandKind::Enroll,
				requested_account.as_str(),
				None,
			)
			.await
			.expect("reserve restore enrollment")
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Pending(_) | AccountCommandReceiptClaim::Replayed(_) => {
				panic!("restore enrollment replayed")
			},
		};
		let (_restore_source, restore_descriptor) =
			owner_private_shared_codex_auth(provider.account_id(), "restored@example.test");
		let restored = service
			.enroll_from_credential_file_command(
				restore_lease,
				restore_operation.clone(),
				requested_account.clone(),
				true,
				&restore_descriptor,
				|result| {
					Ok(match result {
						Ok(account) => json!({
							"outcome": "succeeded",
							"account_id": account.account_id.as_str(),
							"account_revision": account.revision,
						}),
						Err(_) => json!({"outcome": "unexpected"}),
					})
				},
			)
			.await;
		if restored.is_err() {
			let operation = store
				.read_account_operation(&restore_operation)
				.await
				.expect("read failed restore operation")
				.expect("failed restore operation is journaled");
			assert_eq!(operation.phase, AccountOperationPhase::StoreApplied);
			let target = operation.target.expect("failed restore retains its target binding");
			credentials
				.read_exact(&requested_account, &target)
				.expect("failed restore wrote the requested account credential");
			assert!(service.list().await.expect("list after failed restore").is_empty());
		}
		let restored = restored.expect("restore logged-out provider");

		assert_eq!(
			restored,
			json!({
				"outcome": "succeeded",
				"account_id": original_account.as_str(),
				"account_revision": 3,
			})
		);
		let accounts = service.list().await.expect("list restored account");
		assert_eq!(accounts.len(), 1);
		assert_eq!(accounts[0].account.account_id, original_account);
		let binding = accounts[0].account.credential.as_ref().expect("restored credential binding");
		assert_eq!(binding.version.get(), 2);
		assert_eq!(binding.writer_operation_id, restore_operation);
		credentials
			.read_exact(&accounts[0].account.account_id, binding)
			.expect("restored credential is authoritative");
		let restored_account_id = accounts[0].account.account_id.clone();
		let restored_binding = binding.clone();
		assert!(matches!(
			store
				.reserve_account_command(
					&restore_command,
					AccountCommandKind::Enroll,
					requested_account.as_str(),
					None,
				)
				.await
				.expect("replay restore enrollment"),
			AccountCommandReceiptClaim::Replayed(replayed) if replayed == restored
		));

		drop(accounts);
		drop(service);
		drop(credentials);
		drop(store);
		let reopened = SqliteStore::open(&root.paths()).expect("reopen product store");
		let reopened_credentials = SqliteCredentialStore::new(reopened.clone());
		reopened_credentials
			.read_exact(&restored_account_id, &restored_binding)
			.expect("restored credential survives reopen");
		let reopened_service = AccountService::new(
			reopened,
			Arc::new(reopened_credentials),
			Arc::new(UnusedCredentialRefresher),
		);
		let reopened_accounts = reopened_service.list().await.expect("list restored after reopen");
		assert_eq!(reopened_accounts.len(), 1);
		assert_eq!(reopened_accounts[0].account.account_id, restored_account_id);
		assert_eq!(reopened_accounts[0].account.credential.as_ref(), Some(&restored_binding));
	}

	#[tokio::test]
	#[allow(clippy::too_many_lines)] // One full cross-store takeover regression keeps every safety boundary visible together.
	async fn verified_device_login_takes_over_targetless_refresh_ambiguity_end_to_end() {
		let directory = tempdir().expect("temporary product root");
		let root = DecodexRoot::new(fs::canonicalize(directory.path()).expect("canonical root"))
			.expect("typed product root");
		let store = SqliteStore::open(&root.paths()).expect("open product store");
		let credentials: Arc<dyn HostCredentialStore> =
			Arc::new(SqliteCredentialStore::new(store.clone()));
		let service = AccountService::new(
			store.clone(),
			Arc::clone(&credentials),
			Arc::new(UnusedCredentialRefresher),
		);
		let account_id =
			AccountId::new("21000000-0000-4000-8000-000000000001").expect("account identity");
		let enrollment_id = AccountOperationId::new("22000000-0000-4000-8000-000000000001")
			.expect("enrollment identity");
		let ambiguity_id = AccountOperationId::new("22000000-0000-4000-8000-000000000002")
			.expect("ambiguity identity");
		let takeover_id = AccountOperationId::new("22000000-0000-4000-8000-000000000003")
			.expect("takeover identity");
		let provider = ProviderIdentity::new(AccountProvider::Chatgpt, "old-provider-account")
			.expect("provider identity");
		let current_bundle = current_bundle();
		let current = current_bundle
			.binding_for(
				&account_id,
				&enrollment_id,
				CredentialVersion::new(1).expect("initial version"),
				&provider,
			)
			.expect("initial credential binding");
		assert!(matches!(
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
				.expect("prepare enrollment"),
			AccountLifecycleMutationOutcome::Applied(_)
		));
		credentials
			.create(&account_id, &current, current_bundle)
			.expect("write initial credential");
		store
			.advance_account_operation(
				&enrollment_id,
				AccountOperationPhase::Prepared,
				AccountOperationPhase::StoreApplied,
				None,
			)
			.await
			.expect("record initial credential");
		store
			.advance_account_operation(
				&enrollment_id,
				AccountOperationPhase::StoreApplied,
				AccountOperationPhase::Committed,
				None,
			)
			.await
			.expect("commit initial account");
		store
			.prepare_account_operation(&AccountOperationPreparation {
				operation_id: ambiguity_id.clone(),
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
				&ambiguity_id,
				AccountOperationPhase::Prepared,
				AccountOperationPhase::ProviderEffectPending,
				None,
			)
			.await
			.expect("record possible provider effect");
		store
			.advance_account_operation(
				&ambiguity_id,
				AccountOperationPhase::ProviderEffectPending,
				AccountOperationPhase::RecoveryRequired,
				Some("provider_refresh_ambiguous"),
			)
			.await
			.expect("preserve provider ambiguity");

		let login_directory = tempdir().expect("private login directory");
		let login_root = fs::canonicalize(login_directory.path()).expect("canonical login root");
		let auth_path = login_root.join("auth.json");
		let access_payload = URL_SAFE_NO_PAD
			.encode(serde_json::to_vec(&json!({"exp": 4_000_000_000_i64})).expect("access claims"));
		fs::write(
			&auth_path,
			serde_json::to_vec(&json!({
				"auth_mode": "chatgpt",
				"OPENAI_API_KEY": null,
				"tokens": {
					"id_token": identity_token(
						"old-provider-account",
						"verified@example.test",
						"team"
					),
					"access_token": format!("header.{access_payload}.signature"),
					"refresh_token": "verified-refresh",
					"account_id": "old-provider-account"
				},
				"last_refresh": null
			}))
			.expect("login document"),
		)
		.expect("write login document");
		fs::set_permissions(&auth_path, fs::Permissions::from_mode(0o600))
			.expect("private login document");
		let command = CommandIdentity::new("verified-login-takeover", b"exact takeover request")
			.expect("command identity");
		let lease = match store
			.reserve_account_command(
				&command,
				AccountCommandKind::Refresh,
				account_id.as_str(),
				Some(1),
			)
			.await
			.expect("reserve takeover command")
		{
			AccountCommandReceiptClaim::Owned(lease) => lease,
			AccountCommandReceiptClaim::Pending(_) | AccountCommandReceiptClaim::Replayed(_) => {
				panic!("new takeover command replayed")
			},
		};
		let response = service
			.reauthenticate_from_credential_file_command(
				lease,
				takeover_id.clone(),
				&account_id,
				1,
				Some(&ambiguity_id),
				auth_path.to_string_lossy().as_ref(),
				|result| {
					Ok(match result {
						Ok(account) => json!({"outcome": "applied", "revision": account.revision}),
						Err(_) => json!({"outcome": "rejected"}),
					})
				},
			)
			.await
			.expect("complete verified login takeover");

		assert_eq!(response, json!({"outcome": "applied", "revision": 2}));
		let account = service.inspect(&account_id).await.expect("inspect settled account");
		assert_eq!(account.account.revision, 2);
		assert!(account.account.unsettled_operation.is_none());
		let target = account.account.credential.expect("replacement binding");
		assert_eq!(target.version.get(), 2);
		assert_eq!(target.writer_operation_id, takeover_id.clone());
		credentials.read_exact(&account_id, &target).expect("read exact replacement credential");
		let ambiguity = store
			.read_account_operation(&ambiguity_id)
			.await
			.expect("read ambiguity")
			.expect("ambiguity remains recorded");
		assert_eq!(ambiguity.phase, AccountOperationPhase::RecoveryRequired);
		assert_eq!(ambiguity.recovery_code.as_deref(), Some("provider_refresh_ambiguous"));
		assert_eq!(ambiguity.superseded_by_operation_id, Some(takeover_id));
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
	fn conversation_callback_accepts_only_a_newer_same_provider_sqlite_successor() {
		let initial = binding("callback-provider", 3);
		let process = ProcessGenerationAccountBinding::new(7, initial.clone(), "a".repeat(64))
			.expect("process binding");
		assert!(!callback_uses_current_successor(7, &process, &initial).unwrap());

		let mut successor = binding("callback-provider", 4);
		successor.writer_operation_id =
			AccountOperationId::new("10000000-0000-4000-8000-000000000004").unwrap();
		assert!(callback_uses_current_successor(8, &process, &successor).unwrap());
		assert!(matches!(
			callback_uses_current_successor(6, &process, &successor),
			Err(AccountLifecycleError::StaleAccount)
		));

		let switched = binding("different-callback-provider", 4);
		assert!(matches!(
			callback_uses_current_successor(8, &process, &switched),
			Err(AccountLifecycleError::ProviderMismatch)
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

	#[test]
	fn pending_route_rejects_unrelated_account_revision_drift() {
		let mut account = projection_account(Some(binding("revision-drift-provider", 3)));
		let expected_revision = account.revision;
		account.revision += 2;
		let route_operation =
			AccountOperationId::new("22000000-0000-4000-8000-000000000091").unwrap();
		let derived = super::route_refresh_operation_id(&route_operation, &account.account_id)
			.expect("derived Route refresh operation");

		assert!(!route_resume_revision_is_valid(&account, expected_revision, &derived, None,));
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

	fn owner_private_shared_codex_auth(
		provider_account_id: &str,
		email: &str,
	) -> (tempfile::TempDir, String) {
		let directory = tempdir().expect("temporary device-login home");
		let root = fs::canonicalize(directory.path()).expect("canonical device-login home");
		let path = root.join("auth.json");
		let access_payload =
			URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({"exp": 2_000_000_000_i64})).unwrap());
		let access_token = format!("header.{access_payload}.signature");
		let value = json!({
			"auth_mode": "chatgpt",
			"OPENAI_API_KEY": null,
			"tokens": {
				"id_token": identity_token(provider_account_id, email, "pro"),
				"access_token": access_token,
				"refresh_token": "device-login-refresh",
				"account_id": provider_account_id,
			},
			"last_refresh": null,
		});
		fs::write(&path, serde_json::to_vec(&value).unwrap()).expect("write device-login auth");
		fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
			.expect("protect device-login auth");
		(directory, path.to_string_lossy().into_owned())
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
		let access_payload =
			URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({"exp": 61_i64})).unwrap());
		RefreshResponse {
			id_token,
			access_token: Some(format!("header.{access_payload}.signature")),
			refresh_token: None,
			token_type: Some("bearer".to_owned()),
			expires_in: Some(60),
		}
	}

	#[cfg(all(feature = "process-acceptance-fixture", debug_assertions))]
	#[test]
	fn process_acceptance_refresh_endpoint_accepts_only_exact_loopback_http() {
		assert!(process_test_refresh_endpoint_is_safe("http://127.0.0.1:49152/oauth/token"));
		for unsafe_endpoint in [
			"https://127.0.0.1:49152/oauth/token",
			"http://localhost:49152/oauth/token",
			"http://127.0.0.1/oauth/token",
			"http://127.0.0.1:49152/other",
			"http://127.0.0.1:49152/oauth/token?redirect=true",
			"http://user@127.0.0.1:49152/oauth/token",
			"https://auth.openai.com/oauth/token",
		] {
			assert!(!process_test_refresh_endpoint_is_safe(unsafe_endpoint));
		}
	}

	#[cfg(not(all(feature = "process-acceptance-fixture", debug_assertions)))]
	#[test]
	fn ordinary_build_refresh_endpoint_is_the_fixed_https_authority() {
		assert!(
			matches!(refresh_endpoint(), Ok(endpoint) if endpoint == REFRESH_ENDPOINT),
			"ordinary builds must retain the fixed refresh authority"
		);
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
		shared_bundle_with_refresh(
			provider_account_id,
			access_token,
			"shared-refresh",
			expires_at_unix_micros,
		)
	}

	fn shared_bundle_with_refresh(
		provider_account_id: &str,
		access_token: &str,
		refresh_token: &str,
		expires_at_unix_micros: i64,
	) -> CredentialSecretBundle {
		CredentialSecretBundle::chatgpt(
			access_token.to_owned(),
			refresh_token.to_owned(),
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
	fn rejected_provider_refresh_recovers_only_from_a_non_older_matching_shared_bundle() {
		let provider_account_id = "expected-provider-account";
		let current = binding(provider_account_id, 7);
		let current_bundle = shared_bundle(provider_account_id, "current-access", 2_000_000);

		let recovered = recover_rejected_refresh_from_shared(
			&current,
			&current_bundle,
			OBSERVED_AT_MICROS,
			Ok(imported(provider_account_id, "newer-access", 3_000_000)),
		)
		.expect("non-older exact-identity shared auth must recover provider rejection");
		assert_eq!(recovered.returned_provider, current.provider);

		assert!(matches!(
			recover_rejected_refresh_from_shared(
				&current,
				&current_bundle,
				OBSERVED_AT_MICROS,
				Ok(imported("different-provider-account", "different-access", 3_000_000)),
			),
			Err(CredentialRefreshError::Rejected)
		));
		assert!(matches!(
			recover_rejected_refresh_from_shared(
				&current,
				&current_bundle,
				OBSERVED_AT_MICROS,
				Ok(imported(provider_account_id, "older-access", 1_500_000)),
			),
			Err(CredentialRefreshError::Rejected)
		));
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
	fn unchanged_older_or_expired_shared_credential_preserves_provider_refresh_fallback() {
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
		let same_expiry_rotation = matching_shared_refresh(
			&current,
			&current_bundle,
			OBSERVED_AT_MICROS,
			Ok(imported(provider_account_id, "rotated-access", 3_000_000)),
		)
		.expect("same-expiry token rotation must preserve the live shared writer");
		assert_eq!(same_expiry_rotation.bundle.access_token(), "rotated-access");
		assert!(
			matching_shared_refresh(
				&current,
				&current_bundle,
				OBSERVED_AT_MICROS,
				Ok(imported(provider_account_id, "different-access", 2_500_000)),
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
		assert_eq!(
			refreshed.bundle.access_token_expires_at_unix_micros(),
			61_000_000,
			"stored expiry must use the access-token authority used by shared-auth readback",
		);

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
