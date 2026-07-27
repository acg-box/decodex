//! Sole daemon coordinator for PostgreSQL account state and host credential effects.

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
	AccountOperationKind, AccountOperationPhase, AccountRecord, AccountSelectionMode,
	AccountSelectionRecovery, CredentialBinding, CredentialVersion, PostgresConnectionConfig,
	ProcessGenerationAccountBinding, ProcessGenerationId, ProcessGenerationState, ProviderIdentity,
};
use decodex_postgres::{
	AccountAdministrationOutcome, AccountCommandReceiptLease, AccountLifecycleMutationOutcome,
	AccountMigrationReceipt, AccountOperationPreparation, AccountStoreObservation,
	CodexAccountCapabilityAttestation, PostgresStore, RoutingControlOutcome, StoreError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{
	account_import::{
		CredentialImportError, decode_chatgpt_identity, read_explicit_credential_file,
		read_shared_codex_credential,
	},
	host_credentials::{
		CredentialSecretBundle, CredentialStoreError, HostCredentialStore, StoredCredential,
	},
};

const REFRESH_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
const CHATGPT_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const MAX_ACCOUNT_READ: u16 = 512;
const PROVIDER_REFRESH_OUTCOME_UNKNOWN: &str = "provider_refresh_outcome_unknown";

#[cfg(all(target_os = "macos", feature = "account-migration-transition-gate"))]
fn migration_transition_checkpoint(
	name: &str,
	account_id: &AccountId,
) -> Result<(), AccountLifecycleError> {
	crate::account_migration::account_migration_transition_checkpoint(name, Some(account_id))
		.map_err(|_| AccountLifecycleError::InvalidOperation)
}

#[cfg(not(all(target_os = "macos", feature = "account-migration-transition-gate")))]
fn migration_transition_checkpoint(
	_name: &str,
	_account_id: &AccountId,
) -> Result<(), AccountLifecycleError> {
	Ok(())
}

pub(crate) struct CredentialRefreshResult {
	returned_provider: ProviderIdentity,
	bundle: CredentialSecretBundle,
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
	/// Re-read exact PostgreSQL and host-store state and settle only proved effects.
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

/// Private offline-migration precondition mapped onto the existing Import operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AccountMigrationTransition {
	/// Initialize an account that was absent when the operation was first prepared.
	AbsentInitialize {
		/// Absence is represented by the existing nullable revision precondition.
		expected_revision: Option<i64>,
	},
	/// Hydrate credentials against the complete current PostgreSQL account tuple.
	ExistingHydrate {
		/// Current account revision at first preparation.
		revision: i64,
		/// Current label at first preparation.
		display_label: String,
		/// Current administrative state at first preparation.
		enabled: bool,
	},
}

/// Sole account lifecycle coordinator in `decodexd`.
pub struct AccountService {
	store: PostgresStore,
	credentials: Arc<dyn HostCredentialStore>,
	refresher: Arc<dyn CredentialRefreshPort>,
	account_locks: Mutex<HashMap<AccountId, Arc<AsyncMutex<()>>>>,
	callback_ready: AtomicBool,
	callback_profile_sha256: Mutex<Option<String>>,
}
impl AccountService {
	/// Apply or strictly replay the V27 account-cutover intent on one single-use migration
	/// connection before this service can coordinate a destination effect.
	pub(crate) async fn migrate_account_cutover(
		config: &PostgresConnectionConfig,
		migration_password: Option<&str>,
		manifest_sha256: &str,
		manifest: &Value,
		account_count: u32,
	) -> Result<bool, AccountLifecycleError> {
		Ok(PostgresStore::migrate_account_cutover_explicit(
			config,
			migration_password,
			manifest_sha256,
			manifest,
			account_count,
		)
		.await?)
	}

	/// Assemble one coordinator from its three narrow infrastructure ports.
	pub(crate) fn new(
		store: PostgresStore,
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

	/// Persist and activate generated exact-build callback evidence. Unsupported builds stay
	/// closed.
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
	pub(crate) async fn arm_callback_capability_probe(
		&self,
		attestation: &CodexAccountCapabilityAttestation,
	) -> Result<(), AccountLifecycleError> {
		if !attestation.login_chatgpt_auth_tokens
			|| !attestation.refresh_callback
			|| attestation.callback_profile_sha256.len() != 64
			|| !attestation
				.callback_profile_sha256
				.bytes()
				.all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
		{
			return Err(AccountLifecycleError::InvalidOperation);
		}
		if !self.store.attest_codex_account_capability(attestation).await? {
			return Err(AccountLifecycleError::InvalidOperation);
		}
		self.set_callback_capability(attestation.callback_profile_sha256.clone(), true);
		Ok(())
	}

	/// List account registry state with current exact host-store readiness.
	pub async fn list(&self) -> Result<Vec<AccountInspection>, AccountLifecycleError> {
		let accounts = self.store.read_account_registry(None, MAX_ACCOUNT_READ).await?;
		Ok(accounts
			.into_iter()
			.map(|account| AccountInspection { readiness: account.lifecycle_readiness, account })
			.collect())
	}

	/// Read the canonical fast account skeleton and routing control in one PostgreSQL snapshot.
	pub async fn list_snapshot(
		&self,
	) -> Result<(Vec<AccountInspection>, decodex_core::AccountRoutingControl), AccountLifecycleError>
	{
		let (accounts, routing) =
			self.store.read_account_registry_snapshot(MAX_ACCOUNT_READ).await?;
		Ok((
			accounts
				.into_iter()
				.map(|account| AccountInspection {
					readiness: account.lifecycle_readiness,
					account,
				})
				.collect(),
			routing,
		))
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

	/// Enroll through the logical-command journal and commit the terminal registry projection with
	/// its exact public result in one PostgreSQL transaction.
	pub(crate) async fn enroll_from_shared_codex_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: AccountOperationId,
		account_id: AccountId,
		label: String,
		enabled: bool,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<&AccountRecord, AccountLifecycleError>) -> Result<Value, StoreError>
			+ Send,
	{
		let imported = match read_shared_codex_credential() {
			Ok(imported) => imported,
			Err(error) =>
				return self
					.complete_account_command_error(lease, error.into(), build_response)
					.await,
		};
		self.install_credentials_command(
			lease,
			operation_id,
			account_id,
			AccountOperationKind::Enroll,
			label,
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
		label: String,
		enabled: bool,
		source_descriptor: &str,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<&AccountRecord, AccountLifecycleError>) -> Result<Value, StoreError>
			+ Send,
	{
		let imported = match read_explicit_credential_file(source_descriptor) {
			Ok(imported) => imported,
			Err(error) =>
				return self
					.complete_account_command_error(lease, error.into(), build_response)
					.await,
		};
		self.install_credentials_command(
			lease,
			operation_id,
			account_id,
			AccountOperationKind::Import,
			label,
			enabled,
			imported.provider,
			imported.bundle,
			build_response,
		)
		.await
	}

	/// Read the exact manifest operation before migration classifies current account state.
	pub(crate) async fn read_migration_operation(
		&self,
		operation_id: &AccountOperationId,
	) -> Result<Option<AccountOperation>, AccountLifecycleError> {
		Ok(self.store.read_account_operation(operation_id).await?)
	}

	/// Execute or resume one manifest-bound Import through its exact persisted phase.
	#[allow(clippy::too_many_arguments)] // The transition, desired administration, and exact credential target are independent authority inputs.
	pub(crate) async fn install_migrated_credentials(
		&self,
		operation_id: AccountOperationId,
		account_id: AccountId,
		transition: AccountMigrationTransition,
		desired_label: String,
		desired_enabled: bool,
		provider: ProviderIdentity,
		target: CredentialBinding,
		bundle: CredentialSecretBundle,
	) -> Result<AccountRecord, AccountLifecycleError> {
		if target.writer_operation_id != operation_id || target.provider != provider {
			return Err(AccountLifecycleError::InvalidOperation);
		}
		let mut bundle = Some(bundle);
		let (expected_account_revision, requested_display_label, requested_enabled) =
			match &transition {
				AccountMigrationTransition::AbsentInitialize { expected_revision: None } =>
					(None, desired_label, desired_enabled),
				AccountMigrationTransition::AbsentInitialize { expected_revision: Some(_) } =>
					return Err(AccountLifecycleError::InvalidOperation),
				AccountMigrationTransition::ExistingHydrate {
					revision,
					display_label,
					enabled,
				} if *revision > 0 => (Some(*revision), display_label.clone(), *enabled),
				AccountMigrationTransition::ExistingHydrate { .. } =>
					return Err(AccountLifecycleError::InvalidOperation),
			};
		let lock = self.lock_for(&account_id)?;
		let _guard = lock.lock().await;
		let preparation = AccountOperationPreparation {
			operation_id: operation_id.clone(),
			account_id: account_id.clone(),
			kind: AccountOperationKind::Import,
			display_label: Some(requested_display_label),
			enabled: Some(requested_enabled),
			expected_account_revision,
			expected: None,
			target: Some(target.clone()),
			provider,
		};
		let persisted = self.store.read_account_operation(&operation_id).await?;
		let phase = match persisted.as_ref() {
			Some(operation) => {
				self.require_migration_operation(operation, &preparation)?;
				operation.phase
			},
			None => accepted_phase(self.store.prepare_account_operation(&preparation).await?)?,
		};
		if matches!(
			phase,
			AccountOperationPhase::Prepared
				| AccountOperationPhase::StoreApplied
				| AccountOperationPhase::RecoveryRequired
		) {
			self.require_migration_precommit_state(&operation_id, &transition, phase).await?;
		}
		if phase == AccountOperationPhase::Prepared {
			migration_transition_checkpoint("operation_prepared", &account_id)?;
			let bundle = bundle.take().ok_or(AccountLifecycleError::InvalidOperation)?;
			match self.credentials.create(&account_id, &target, bundle) {
				Ok(()) => {},
				Err(CredentialStoreError::AlreadyExists)
					if self.credentials.read_exact(&account_id, &target).is_ok() => {},
				Err(error) => {
					self.recover_or_cancel(
						&operation_id,
						AccountOperationPhase::Prepared,
						"credential_create_failed",
						store_effect_may_be_ambiguous(error),
					)
					.await?;
					return Err(error.into());
				},
			}
			migration_transition_checkpoint("keychain_applied", &account_id)?;
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
			migration_transition_checkpoint("store_applied", &account_id)?;
		} else if phase == AccountOperationPhase::StoreApplied {
			migration_transition_checkpoint("store_applied", &account_id)?;
		}
		match phase {
			AccountOperationPhase::Prepared | AccountOperationPhase::StoreApplied => {
				self.credentials.read_exact(&account_id, &target)?;
				self.commit_store_applied(&operation_id).await?;
			},
			AccountOperationPhase::Committed => {
				self.credentials.read_exact(&account_id, &target)?;
			},
			AccountOperationPhase::RecoveryRequired => {
				let operation = self
					.store
					.read_account_operation(&operation_id)
					.await?
					.ok_or(AccountLifecycleError::InvalidOperation)?;
				self.require_migration_operation(&operation, &preparation)?;
				let target_ready = match self.credentials.read_exact(&account_id, &target) {
					Ok(_) => true,
					Err(CredentialStoreError::NotFound) => {
						let bundle =
							bundle.take().ok_or(AccountLifecycleError::InvalidOperation)?;
						match self.credentials.create(&account_id, &target, bundle) {
							Ok(()) => true,
							Err(CredentialStoreError::AlreadyExists) =>
								self.credentials.read_exact(&account_id, &target).is_ok(),
							Err(_) => false,
						}
					},
					Err(_) => false,
				};
				if !target_ready {
					return Err(AccountLifecycleError::NotReady(
						AccountLifecycleReadiness::OperationUnsettled,
					));
				}
				migration_transition_checkpoint("keychain_applied", &account_id)?;
				accepted_phase(
					self.store
						.advance_account_operation(
							&operation_id,
							AccountOperationPhase::RecoveryRequired,
							AccountOperationPhase::StoreApplied,
							None,
						)
						.await?,
				)?;
				migration_transition_checkpoint("store_applied", &account_id)?;
				self.commit_store_applied(&operation_id).await?;
			},
			AccountOperationPhase::Cancelled | AccountOperationPhase::ProviderEffectPending =>
				return Err(AccountLifecycleError::InvalidOperation),
		}
		migration_transition_checkpoint("credential_committed", &account_id)?;
		let account = self.load_account(&account_id).await?;
		if account.credential.as_ref() != Some(&target) {
			return Err(AccountLifecycleError::StaleAccount);
		}
		Ok(account)
	}

	async fn require_migration_precommit_state(
		&self,
		operation_id: &AccountOperationId,
		transition: &AccountMigrationTransition,
		phase: AccountOperationPhase,
	) -> Result<(), AccountLifecycleError> {
		let operation = self
			.store
			.read_account_operation(operation_id)
			.await?
			.ok_or(AccountLifecycleError::InvalidOperation)?;
		let account = self.load_account(&operation.account_id).await?;
		let (revision, display_label, enabled) = match transition {
			AccountMigrationTransition::AbsentInitialize { expected_revision: None } =>
				(1, operation.requested_display_label.as_deref(), operation.requested_enabled),
			AccountMigrationTransition::ExistingHydrate { revision, display_label, enabled } =>
				(*revision, Some(display_label.as_str()), Some(*enabled)),
			AccountMigrationTransition::AbsentInitialize { expected_revision: Some(_) } =>
				return Err(AccountLifecycleError::InvalidOperation),
		};
		let unsettled = account.unsettled_operation.as_ref();
		if account.revision != revision
			|| account.label != display_label.unwrap_or_default()
			|| Some(account.enabled) != enabled
			|| account.credential.is_some()
			|| account.tombstoned
			|| unsettled.map(|status| &status.operation_id) != Some(operation_id)
			|| unsettled.map(|status| status.kind) != Some(AccountOperationKind::Import)
			|| unsettled.map(|status| status.phase) != Some(phase)
		{
			return Err(AccountLifecycleError::StaleAccount);
		}
		Ok(())
	}

	fn require_migration_operation(
		&self,
		operation: &AccountOperation,
		expected: &AccountOperationPreparation,
	) -> Result<(), AccountLifecycleError> {
		if operation.operation_id != expected.operation_id
			|| operation.account_id != expected.account_id
			|| operation.kind != AccountOperationKind::Import
			|| operation.expected_account_revision != expected.expected_account_revision
			|| operation.requested_display_label.as_ref() != expected.display_label.as_ref()
			|| operation.requested_enabled != expected.enabled
			|| operation.expected.is_some()
			|| operation.target.as_ref() != expected.target.as_ref()
			|| operation.target.as_ref().map(|target| &target.provider) != Some(&expected.provider)
		{
			return Err(AccountLifecycleError::InvalidOperation);
		}
		Ok(())
	}

	#[allow(clippy::too_many_arguments)]
	async fn install_credentials_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		operation_id: AccountOperationId,
		account_id: AccountId,
		kind: AccountOperationKind,
		label: String,
		enabled: bool,
		provider: ProviderIdentity,
		bundle: CredentialSecretBundle,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(Result<&AccountRecord, AccountLifecycleError>) -> Result<Value, StoreError>
			+ Send,
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
			display_label: Some(label),
			enabled: Some(enabled),
			expected_account_revision: None,
			expected: None,
			target: Some(target.clone()),
			provider,
		};
		let phase = match self.store.prepare_account_operation(&preparation).await? {
			AccountLifecycleMutationOutcome::Applied(mutation)
			| AccountLifecycleMutationOutcome::Replayed(mutation) => mutation.phase,
			AccountLifecycleMutationOutcome::Rejected { rejection, .. } =>
				return self
					.complete_account_command_error(
						lease,
						AccountLifecycleError::OperationRejected(rejection),
						build_response,
					)
					.await,
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
	#[allow(clippy::too_many_lines)] // Keep the generation-bound refresh state machine auditable as one sequence.
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
		let refreshed = tokio::task::spawn_blocking(move || refresher.refresh(stored.bundle()))
			.await
			.map_err(|_| AccountLifecycleError::Refresh(CredentialRefreshError::Ambiguous))?;
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
			+ Send,
	{
		let lock = self.lock_for(account_id)?;
		let _guard = lock.lock().await;
		let mut build_response = Some(build_response);
		let account = match self.load_account(account_id).await {
			Ok(account) => account,
			Err(error) =>
				return self
					.complete_account_command_error(
						lease,
						error,
						build_response.take().expect("builder is retained"),
					)
					.await,
		};
		let current = match account.credential.clone() {
			Some(current) => current,
			None =>
				return self
					.complete_account_command_error(
						lease,
						AccountLifecycleError::CredentialAbsent,
						build_response.take().expect("builder is retained"),
					)
					.await,
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
				AccountOperationPhase::Committed | AccountOperationPhase::StoreApplied =>
					return self
						.complete_account_operation_success(
							lease,
							&operation_id,
							AccountOperationPhase::StoreApplied,
							AccountOperationPhase::Committed,
							build_response.take().expect("builder is retained"),
						)
						.await,
				AccountOperationPhase::Cancelled =>
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
						.await,
				AccountOperationPhase::RecoveryRequired =>
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
						.await,
				AccountOperationPhase::Prepared =>
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
						.await,
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
			Err(error) =>
				return self
					.complete_account_command_error(
						lease,
						error,
						build_response.take().expect("builder is retained"),
					)
					.await,
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
			AccountLifecycleMutationOutcome::Rejected { rejection, .. } =>
				return self
					.complete_account_command_error(
						lease,
						AccountLifecycleError::OperationRejected(rejection),
						build_response.take().expect("builder is retained"),
					)
					.await,
			_ =>
				return self
					.complete_account_command_error(
						lease,
						AccountLifecycleError::InvalidOperation,
						build_response.take().expect("builder is retained"),
					)
					.await,
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
		let refresher = Arc::clone(&self.refresher);
		let refreshed =
			match tokio::task::spawn_blocking(move || refresher.refresh(stored.bundle())).await {
				Ok(refreshed) => refreshed,
				Err(_) =>
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
						.await,
			};
		let refreshed = match refreshed {
			Ok(refreshed) => refreshed,
			Err(error @ CredentialRefreshError::Rejected) =>
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
					.await,
			Err(error) =>
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
					.await,
		};
		let target =
			match refreshed_credential_target(&current, account_id, &operation_id, &refreshed) {
				Ok(target) => target,
				Err(AccountLifecycleError::ProviderMismatch) =>
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
						.await,
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

	/// Verify the exact callback-authored successor in PostgreSQL and the host store.
	pub(crate) async fn verify_callback_successor(
		&self,
		account_id: &AccountId,
		initial: &CredentialBinding,
	) -> Result<(), AccountLifecycleError> {
		let account = self.load_account(account_id).await?;
		let target = account.credential.as_ref().ok_or(AccountLifecycleError::CredentialAbsent)?;
		if target.provider != initial.provider
			|| target.version
				!= initial
					.version
					.successor()
					.map_err(|_| AccountLifecycleError::InvalidOperation)?
			|| target.writer_operation_id == initial.writer_operation_id
		{
			return Err(AccountLifecycleError::StaleAccount);
		}
		let operation = self
			.store
			.read_account_operation(&target.writer_operation_id)
			.await?
			.ok_or(AccountLifecycleError::StaleAccount)?;
		if operation.account_id != *account_id
			|| operation.kind != AccountOperationKind::Refresh
			|| operation.phase != AccountOperationPhase::Committed
			|| operation.expected.as_ref() != Some(initial)
			|| operation.target.as_ref() != Some(target)
		{
			return Err(AccountLifecycleError::StaleAccount);
		}
		let observed = self.credentials.read_exact(account_id, target)?;
		if observed.binding() != target {
			return Err(AccountLifecycleError::StaleAccount);
		}
		Ok(())
	}

	/// Delete an exact host bundle, then tombstone its PostgreSQL account projection.
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
			+ Send,
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
				Err(error) =>
					return self
						.complete_account_command_error(
							lease,
							error,
							build_response.take().expect("builder is retained"),
						)
						.await,
			};
			let credential = match account.credential.clone() {
				Some(credential) => credential,
				None =>
					return self
						.complete_account_command_error(
							lease,
							AccountLifecycleError::CredentialAbsent,
							build_response.take().expect("builder is retained"),
						)
						.await,
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
				AccountLifecycleMutationOutcome::Rejected { rejection, .. } =>
					return self
						.complete_account_command_error(
							lease,
							AccountLifecycleError::OperationRejected(rejection),
							build_response.take().expect("builder is retained"),
						)
						.await,
			}
			expected = Some(credential);
		}
		let phase = phase.expect("logout preparation or replay has one phase");
		match phase {
			AccountOperationPhase::Committed | AccountOperationPhase::StoreApplied =>
				return self
					.complete_account_operation_success(
						lease,
						&operation_id,
						AccountOperationPhase::StoreApplied,
						AccountOperationPhase::Committed,
						build_response.take().expect("builder is retained"),
					)
					.await,
			AccountOperationPhase::Cancelled =>
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
					.await,
			AccountOperationPhase::RecoveryRequired
			| AccountOperationPhase::ProviderEffectPending =>
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
					.await,
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

	/// Rename, enable, or disable without changing observed health.
	pub async fn update_administration(
		&self,
		account_id: &AccountId,
		expected_revision: i64,
		label: Option<&str>,
		enabled: Option<bool>,
	) -> Result<AccountAdministrationOutcome, AccountLifecycleError> {
		Ok(self
			.store
			.update_account_administration(account_id, expected_revision, label, enabled)
			.await?)
	}

	/// Apply one administrative command and its durable public result in one PG transaction.
	pub async fn update_administration_command<F>(
		&self,
		lease: AccountCommandReceiptLease,
		account_id: &AccountId,
		expected_revision: i64,
		label: Option<&str>,
		enabled: Option<bool>,
		build_response: F,
	) -> Result<Value, AccountLifecycleError>
	where
		F: FnOnce(
				&AccountAdministrationOutcome,
				Option<&AccountRecord>,
			) -> Result<Value, StoreError>
			+ Send,
	{
		Ok(self
			.store
			.update_account_administration_command(
				lease,
				account_id,
				expected_revision,
				label,
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
		F: FnOnce(&RoutingControlOutcome) -> Result<Value, StoreError> + Send,
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
		F: FnOnce(&RoutingControlOutcome) -> Result<Value, StoreError> + Send,
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
		F: FnOnce(&RoutingControlOutcome) -> Result<Value, StoreError> + Send,
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
			+ Send,
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
			+ Send,
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
			+ Send,
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
			+ Send,
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
			+ Send,
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

	/// Strictly replay the singleton canonical migration identity already committed with V27.
	pub async fn prepare_migration_intent(
		&self,
		manifest_sha256: &str,
		manifest: &Value,
		account_count: u32,
	) -> Result<bool, AccountLifecycleError> {
		Ok(self
			.store
			.prepare_account_migration_intent(manifest_sha256, manifest, account_count)
			.await?)
	}

	/// Persist or replay one exact completed offline migration receipt.
	pub async fn record_migration_receipt(
		&self,
		receipt: &AccountMigrationReceipt,
	) -> Result<bool, AccountLifecycleError> {
		Ok(self.store.record_account_migration_receipt(receipt).await?)
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
						Ok(_) =>
							return Ok(AccountSelectionResult {
								account: account.clone(),
								callback_profile_sha256,
							}),
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
			(AccountOperationPhase::Committed | AccountOperationPhase::Cancelled, _) =>
				return Err(AccountLifecycleError::InvalidOperation),
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
			+ Send,
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
			(AccountOperationPhase::Committed | AccountOperationPhase::Cancelled, _) =>
				return self
					.complete_recovery_command_error(
						lease,
						AccountLifecycleError::InvalidOperation,
						build_response.take().expect("builder is retained"),
					)
					.await,
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

	/// Acquire one exact projection for the bounded per-account observation query. Administrative
	/// disablement blocks effects and new work admission, not a user-requested health read.
	pub(crate) async fn process_credential_for_observation(
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
		let stored = self.read_exact_for_existing_work(&account).await?;
		let callback_profile = self.callback_profile().map_err(|_| {
			AccountLifecycleError::NotReady(AccountLifecycleReadiness::CallbackCapabilityUnready)
		})?;
		let binding =
			ProcessGenerationAccountBinding::new(account.revision, credential, callback_profile)
				.map_err(|_| AccountLifecycleError::InvalidOperation)?;
		Ok(AccountProcessCredential { stored, binding, launch_guard })
	}

	/// Acquire the retained exact binding for already-admitted work. Administrative changes do not
	/// revoke that work, but credential and callback changes still fail before provider effect.
	pub(crate) async fn process_credential_for_existing_work(
		&self,
		account_id: &AccountId,
		retained: &ProcessGenerationAccountBinding,
	) -> Result<AccountProcessCredential, AccountLifecycleError> {
		let launch_guard = self.lock_for(account_id)?.lock_owned().await;
		let account = self.load_account(account_id).await?;
		let credential =
			account.credential.clone().ok_or(AccountLifecycleError::CredentialAbsent)?;
		if credential != retained.credential {
			return Err(AccountLifecycleError::StaleAccount);
		}
		let stored = self.read_exact_for_existing_work(&account).await?;
		let callback_profile = self.callback_profile().map_err(|_| {
			AccountLifecycleError::NotReady(AccountLifecycleReadiness::CallbackCapabilityUnready)
		})?;
		if callback_profile != retained.refresh_callback_profile_sha256 {
			return Err(AccountLifecycleError::StaleAccount);
		}
		let binding =
			ProcessGenerationAccountBinding::new(account.revision, credential, callback_profile)
				.map_err(|_| AccountLifecycleError::InvalidOperation)?;
		Ok(AccountProcessCredential { stored, binding, launch_guard })
	}

	/// Acquire current exact credentials for one already-started Reset Card reconciliation.
	pub(crate) async fn process_credential_for_reconciliation(
		&self,
		account_id: &AccountId,
		retained: &ProcessGenerationAccountBinding,
	) -> Result<AccountProcessCredential, AccountLifecycleError> {
		let launch_guard = self.lock_for(account_id)?.lock_owned().await;
		let account = self.load_account(account_id).await?;
		if account.tombstoned {
			return Err(AccountLifecycleError::StaleAccount);
		}
		let credential =
			account.credential.clone().ok_or(AccountLifecycleError::CredentialAbsent)?;
		if credential.provider != retained.credential.provider {
			return Err(AccountLifecycleError::ProviderMismatch);
		}
		let stored = self.credentials.read_exact(account_id, &credential)?;
		self.store
			.observe_account_store(
				account_id,
				account.revision,
				&credential,
				AccountStoreObservation::Exact,
			)
			.await?;
		let binding = ProcessGenerationAccountBinding::new(
			account.revision,
			credential,
			retained.refresh_callback_profile_sha256.clone(),
		)
		.map_err(|_| AccountLifecycleError::InvalidOperation)?;
		Ok(AccountProcessCredential { stored, binding, launch_guard })
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
			AccountLifecycleReadiness::CredentialAbsent =>
				return Err(AccountSelectionRecovery::EnrollCredentials),
			AccountLifecycleReadiness::OperationUnsettled =>
				return Err(AccountSelectionRecovery::ResolveCredentialOperation),
			AccountLifecycleReadiness::CallbackCapabilityUnready =>
				return Err(AccountSelectionRecovery::UpgradeCodex),
			AccountLifecycleReadiness::ProviderMismatch =>
				return Err(AccountSelectionRecovery::RestoreProviderAgreement),
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
		if operation.phase == AccountOperationPhase::Committed {
			return Ok(ReconciliationDisposition::Committed);
		}
		if operation.phase == AccountOperationPhase::Cancelled {
			return Ok(ReconciliationDisposition::Cancelled);
		}
		if operation.phase == AccountOperationPhase::RecoveryRequired {
			return Ok(ReconciliationDisposition::Manual);
		}
		if operation.phase == AccountOperationPhase::StoreApplied {
			self.commit_store_applied(&operation.operation_id).await?;
			return Ok(ReconciliationDisposition::Committed);
		}
		match operation.kind {
			AccountOperationKind::Enroll | AccountOperationKind::Import => {
				let target =
					operation.target.as_ref().ok_or(AccountLifecycleError::InvalidOperation)?;
				match self.credentials.read_exact(&operation.account_id, target) {
					Ok(_) => {
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
					},
					Err(CredentialStoreError::NotFound)
						if operation.phase == AccountOperationPhase::Prepared =>
					{
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
					},
					Err(_) => self.mark_manual(operation, "credential_import_reconciliation").await,
				}
			},
			AccountOperationKind::Refresh => {
				if operation.phase == AccountOperationPhase::Prepared {
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
					return Ok(ReconciliationDisposition::Cancelled);
				}
				let Some(target) = operation.target.as_ref() else {
					return self.mark_manual(operation, PROVIDER_REFRESH_OUTCOME_UNKNOWN).await;
				};
				match self.credentials.read_exact(&operation.account_id, target) {
					Ok(_) => {
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
					},
					Err(_) =>
						self.mark_manual(operation, "credential_refresh_reconciliation").await,
				}
			},
			AccountOperationKind::Logout => {
				let expected =
					operation.expected.as_ref().ok_or(AccountLifecycleError::InvalidOperation)?;
				match self.credentials.delete(&operation.account_id, expected) {
					Ok(()) | Err(CredentialStoreError::NotFound) => {
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
					},
					Err(_) => self.mark_manual(operation, "credential_logout_reconciliation").await,
				}
			},
		}
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
		let mut locks =
			self.account_locks.lock().map_err(|_| AccountLifecycleError::CoordinatorUnavailable)?;
		Ok(Arc::clone(
			locks.entry(account_id.clone()).or_insert_with(|| Arc::new(AsyncMutex::new(()))),
		))
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

	pub(crate) fn reset_card_callback_profile(&self) -> Option<String> {
		self.callback_profile().ok()
	}
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
	/// PostgreSQL authority failed.
	Persistence(StoreError),
	/// The exact host credential store operation failed.
	CredentialStore(CredentialStoreError),
	/// The provider refresh adapter failed.
	Refresh(CredentialRefreshError),
	/// PostgreSQL rejected a finite lifecycle transition.
	OperationRejected(decodex_postgres::AccountLifecycleRejection),
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
		AccountId, AccountOperationId, AccountProvider, CredentialBinding, CredentialFingerprint,
		CredentialStoreSchemaVersion, CredentialVersion, ProviderIdentity,
	};
	use serde_json::json;

	use super::{
		AccountLifecycleError, CredentialRefreshError, CredentialSecretBundle,
		PROVIDER_REFRESH_OUTCOME_UNKNOWN, RefreshResponse, credential_refresh_result,
		refreshed_credential_target,
	};

	const OBSERVED_AT_MICROS: i64 = 1_000_000;

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
