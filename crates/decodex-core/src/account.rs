use std::{
	error::Error,
	fmt::{Display, Formatter},
};

/// Stable non-secret Decodex account identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AccountId(String);
impl AccountId {
	/// Parse one canonical lower-case UUID without accepting provider identity or credentials.
	pub fn new(value: impl Into<String>) -> Result<Self, AccountError> {
		let value = value.into();

		if !is_canonical_uuid(&value) {
			return Err(AccountError::InvalidAccountId);
		}

		Ok(Self(value))
	}

	/// Borrow the canonical account identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Stable non-secret provider kind supported by the Slice 1 account authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountProvider {
	/// ChatGPT-backed Codex account authentication.
	Chatgpt,
}

/// Credential-negative binding between a Decodex account and one provider account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderIdentity {
	provider: AccountProvider,
	account_id: String,
}
impl ProviderIdentity {
	/// Construct a provider binding from a bounded non-secret provider account identifier.
	pub fn new(
		provider: AccountProvider,
		account_id: impl Into<String>,
	) -> Result<Self, AccountError> {
		let account_id = account_id.into();

		if account_id.is_empty()
			|| account_id.len() > 512
			|| account_id.chars().any(char::is_control)
		{
			return Err(AccountError::InvalidProviderIdentity);
		}

		Ok(Self { provider, account_id })
	}

	/// Return the provider kind.
	pub const fn provider(&self) -> AccountProvider {
		self.provider
	}

	/// Borrow the provider account identifier.
	pub fn account_id(&self) -> &str {
		&self.account_id
	}
}

/// Version of the serialized secret bundle stored by the host credential store.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CredentialStoreSchemaVersion(u16);
impl CredentialStoreSchemaVersion {
	/// Initial closed host credential-bundle schema.
	pub const V1: Self = Self(1);

	/// Construct a supported store schema version.
	pub const fn new(value: u16) -> Result<Self, AccountError> {
		if value == Self::V1.0 {
			Ok(Self(value))
		} else {
			Err(AccountError::UnsupportedCredentialStoreSchema)
		}
	}

	/// Return the wire and persistence value.
	pub const fn get(self) -> u16 {
		self.0
	}
}

/// Monotonic version of one account's secret bundle.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CredentialVersion(u64);
impl CredentialVersion {
	/// Construct a nonzero credential version.
	pub const fn new(value: u64) -> Result<Self, AccountError> {
		if value == 0 { Err(AccountError::InvalidCredentialVersion) } else { Ok(Self(value)) }
	}

	/// Return the persistence value.
	pub const fn get(self) -> u64 {
		self.0
	}

	/// Return the next monotonic version, if it can be represented.
	pub const fn successor(self) -> Result<Self, AccountError> {
		match self.0.checked_add(1) {
			Some(value) => Ok(Self(value)),
			None => Err(AccountError::CredentialVersionExhausted),
		}
	}
}

/// Canonical SHA-256 fingerprint of one complete serialized credential bundle.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct CredentialFingerprint(String);
impl CredentialFingerprint {
	/// Parse exactly 64 lower-case hexadecimal SHA-256 characters.
	pub fn new(value: impl Into<String>) -> Result<Self, AccountError> {
		let value = value.into();

		if value.len() != 64
			|| !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
		{
			return Err(AccountError::InvalidCredentialFingerprint);
		}

		Ok(Self(value))
	}

	/// Borrow the canonical digest.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Credential-negative registry projection that must agree with the host store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialBinding {
	/// Host-store serialization schema.
	pub schema_version: CredentialStoreSchemaVersion,
	/// Monotonic secret-bundle version.
	pub version: CredentialVersion,
	/// Canonical digest of the complete serialized bundle.
	pub fingerprint: CredentialFingerprint,
	/// Non-secret provider identity bound to the bundle.
	pub provider: ProviderIdentity,
	/// Exact finite account operation that wrote this host-store version.
	pub writer_operation_id: AccountOperationId,
}

/// Finite cross-store lifecycle operations coordinated by the Account Service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountOperationKind {
	/// Create one account from the shared Codex credentials.
	Enroll,
	/// Create or hydrate one account from an explicit credential file.
	Import,
	/// Rotate one account to the next credential version.
	Refresh,
	/// Delete one credential bundle and tombstone its account.
	Logout,
}

/// Recoverable phase of one finite durable-store/host-store account operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountOperationPhase {
	/// durable-store accepted the operation before an external effect.
	Prepared,
	/// A provider refresh effect can no longer be proved absent.
	ProviderEffectPending,
	/// The host-store effect is proved and the registry commit is pending.
	StoreApplied,
	/// The host-store and registry projections agree.
	Committed,
	/// The operation ended before an external effect.
	Cancelled,
	/// The operation requires an explicit reconciliation action.
	RecoveryRequired,
}
impl AccountOperationPhase {
	/// Return whether no startup reconciliation work remains for this operation.
	pub const fn is_terminal(self) -> bool {
		matches!(self, Self::Committed | Self::Cancelled | Self::RecoveryRequired)
	}
}

/// Stable identity of one idempotent account lifecycle operation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AccountOperationId(String);
impl AccountOperationId {
	/// Generate one random version-4 operation UUID from the host randomness source.
	pub fn generate() -> Result<Self, AccountError> {
		let mut bytes = [0_u8; 16];
		getrandom::fill(&mut bytes).map_err(|_| AccountError::RandomnessUnavailable)?;
		bytes[6] = (bytes[6] & 0x0f) | 0x40;
		bytes[8] = (bytes[8] & 0x3f) | 0x80;
		Self::new(format!(
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
	}

	/// Parse one canonical lower-case UUID.
	pub fn new(value: impl Into<String>) -> Result<Self, AccountError> {
		let value = value.into();

		if !is_canonical_uuid(&value) {
			return Err(AccountError::InvalidOperationId);
		}

		Ok(Self(value))
	}

	/// Borrow the canonical operation identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Credential-negative operation projection used for startup reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountOperation {
	/// Stable operation identity.
	pub operation_id: AccountOperationId,
	/// Account changed by the operation.
	pub account_id: AccountId,
	/// Operation effect class.
	pub kind: AccountOperationKind,
	/// Current finite phase.
	pub phase: AccountOperationPhase,
	/// Exact account revision required when the operation was prepared.
	pub expected_account_revision: Option<i64>,
	/// Exact label persisted in the immutable operation descriptor.
	pub requested_display_label: Option<String>,
	/// Exact enabled value persisted in the immutable operation descriptor.
	pub requested_enabled: Option<bool>,
	/// Exact credential binding before the effect, when required.
	pub expected: Option<CredentialBinding>,
	/// Exact credential binding after the effect, when known.
	pub target: Option<CredentialBinding>,
}

/// Credential-negative unsettled operation state rendered with one account row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountOperationStatus {
	/// Stable operation identity.
	pub operation_id: AccountOperationId,
	/// Operation effect class.
	pub kind: AccountOperationKind,
	/// Current nonterminal phase.
	pub phase: AccountOperationPhase,
	/// Stable manual-recovery reason, when required.
	pub recovery_code: Option<String>,
}

/// Independent lifecycle gate for account-bound admission and process launch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountLifecycleReadiness {
	/// All account admission boundaries agree.
	Ready,
	/// The registry has no current credential binding.
	CredentialAbsent,
	/// The host credential store could not be read.
	StoreUnavailable,
	/// Registry and host-store credential metadata differ.
	StoreMismatch,
	/// Registry and host-store provider identities differ.
	ProviderMismatch,
	/// A finite credential operation is not settled.
	OperationUnsettled,
	/// The exact Codex refresh-callback capability is not ready.
	CallbackCapabilityUnready,
	/// The account is tombstoned and cannot admit work.
	Tombstoned,
}

/// User-controlled initial account selection mode.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountSelectionMode {
	/// Select only the configured account.
	Fixed(AccountId),
	/// Select the first eligible account in the complete order.
	Balanced,
}

/// Versioned user-owned routing controls. Order is deterministic and contains no secrets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountRoutingControl {
	/// Optimistic routing-control revision.
	pub revision: i64,
	/// Current initial-selection mode.
	pub mode: AccountSelectionMode,
	/// Complete deterministic order of visible accounts.
	pub order: Vec<AccountId>,
}

/// One observed quota window used for deterministic initial selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountQuotaWindow {
	/// Exact supported window duration in minutes.
	pub duration_minutes: u32,
	/// Provider-reported percentage used.
	pub used_percent: u8,
	/// Provider-reported reset time in Unix microseconds.
	pub resets_at_unix_micros: i64,
}
impl AccountQuotaWindow {
	/// Five-hour Codex quota window.
	pub const FIVE_HOURS_MINUTES: u32 = 300;
	/// Seven-day Codex quota window.
	pub const SEVEN_DAYS_MINUTES: u32 = 10_080;

	/// Construct only an accepted deterministic-selection window.
	pub const fn new(
		duration_minutes: u32,
		used_percent: u8,
		resets_at_unix_micros: i64,
	) -> Result<Self, AccountError> {
		if !matches!(duration_minutes, Self::FIVE_HOURS_MINUTES | Self::SEVEN_DAYS_MINUTES)
			|| used_percent > 100
			|| resets_at_unix_micros <= 0
		{
			return Err(AccountError::InvalidQuotaWindow);
		}

		Ok(Self { duration_minutes, used_percent, resets_at_unix_micros })
	}
}

/// Bounded row-scoped failure from one provider quota observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountQuotaObservationError {
	/// The provider request could not complete.
	ProviderUnavailable,
	/// The provider response did not satisfy the protocol contract.
	ProtocolUnavailable,
	/// The response identified a different account.
	AccountMismatch,
	/// The response did not contain one required quota duration.
	UnsupportedWindow,
}

/// Freshness and result of one exact quota-window observation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountQuotaDisposition {
	/// No observation exists.
	Unknown,
	/// The retained fact is current.
	Current(AccountQuotaWindow),
	/// The retained fact is expired or older than the freshness limit.
	Stale(AccountQuotaWindow),
	/// The latest observation produced a bounded error.
	Error(AccountQuotaObservationError),
}

/// One required quota window with server-owned freshness classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountQuotaWindowObservation {
	/// Exact supported window duration in minutes.
	pub duration_minutes: u32,
	/// Time of the retained observation, or none for an unknown value.
	pub observed_at_unix_micros: Option<i64>,
	/// Server-owned freshness and result classification.
	pub disposition: AccountQuotaDisposition,
}
impl AccountQuotaWindowObservation {
	/// Construct the absent initial state for one accepted duration.
	pub const fn unknown(duration_minutes: u32) -> Result<Self, AccountError> {
		if !matches!(
			duration_minutes,
			AccountQuotaWindow::FIVE_HOURS_MINUTES | AccountQuotaWindow::SEVEN_DAYS_MINUTES
		) {
			return Err(AccountError::InvalidQuotaWindow);
		}
		Ok(Self {
			duration_minutes,
			observed_at_unix_micros: None,
			disposition: AccountQuotaDisposition::Unknown,
		})
	}

	/// Return a current fact for selection; stale, error, and unknown observations are excluded.
	pub const fn current(self) -> Option<AccountQuotaWindow> {
		match self.disposition {
			AccountQuotaDisposition::Current(fact) => Some(fact),
			_ => None,
		}
	}
}

/// Credential-negative account registry view owned by durable-store.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountRecord {
	/// Canonical Decodex account identity.
	pub account_id: AccountId,
	/// Non-secret operator label.
	pub label: String,
	/// Independent administrative admission switch.
	pub enabled: bool,
	/// Optimistic account revision.
	pub revision: i64,
	/// Persisted provider-observed health.
	pub observed_state: AccountState,
	/// Derived lifecycle admission gate.
	pub lifecycle_readiness: AccountLifecycleReadiness,
	/// Current credential-negative host-store binding.
	pub credential: Option<CredentialBinding>,
	/// Current unsettled lifecycle operation, when one exists.
	pub unsettled_operation: Option<AccountOperationStatus>,
	/// Required 300-minute quota observation.
	pub five_hour_quota: AccountQuotaWindowObservation,
	/// Required 10,080-minute quota observation.
	pub seven_day_quota: AccountQuotaWindowObservation,
	/// Whether logout has permanently removed this account from admission.
	pub tombstoned: bool,
}

/// Typed manual action when deterministic initial selection cannot proceed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountSelectionRecovery {
	/// Select an existing account in fixed mode.
	ConfigureFixedAccount,
	/// Enable the selected account.
	EnableAccount,
	/// Install credentials for an account.
	EnrollCredentials,
	/// Reconcile or cancel the unsettled credential operation.
	ResolveCredentialOperation,
	/// Restore exact registry and host-store agreement.
	RepairCredentialStore,
	/// Restore exact provider identity agreement.
	RestoreProviderAgreement,
	/// Refresh the required quota observations.
	RefreshQuota,
	/// Install a Codex build with the required callback capability.
	UpgradeCodex,
}

impl Display for AccountId {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.0)
	}
}

/// Persistable non-secret account health observation.
///
/// This enum intentionally exposes no eligibility or selection operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountState {
	/// The account or its required host boundary is unavailable.
	Unavailable,
	/// No current evidence establishes readiness.
	Unknown,
	/// Fresh evidence reports availability; live routing remains separately disabled.
	Available,
	/// A known quota window is depleted.
	Depleted,
	/// Authentication was rejected or is absent.
	AuthFailed,
	/// Required plugin readiness was not established.
	PluginUnready,
}

/// Closed account-domain validation failure without caller-provided text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountError {
	/// The account identity was not one canonical lower-case UUID.
	InvalidAccountId,
	/// The provider identity was empty, oversized, or contained control characters.
	InvalidProviderIdentity,
	/// The host credential-store schema is not supported by this build.
	UnsupportedCredentialStoreSchema,
	/// Credential versions start at one.
	InvalidCredentialVersion,
	/// The credential version cannot advance further.
	CredentialVersionExhausted,
	/// The fingerprint was not a canonical SHA-256 digest.
	InvalidCredentialFingerprint,
	/// The operation identity was not one canonical lower-case UUID.
	InvalidOperationId,
	/// The host could not supply randomness for a new operation identity.
	RandomnessUnavailable,
	/// The quota fact was not one accepted window or percentage.
	InvalidQuotaWindow,
}
impl Error for AccountError {}

impl Display for AccountError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidAccountId => "invalid account identity",
			Self::InvalidProviderIdentity => "invalid provider identity",
			Self::UnsupportedCredentialStoreSchema => "unsupported credential-store schema",
			Self::InvalidCredentialVersion => "invalid credential version",
			Self::CredentialVersionExhausted => "credential version exhausted",
			Self::InvalidCredentialFingerprint => "invalid credential fingerprint",
			Self::InvalidOperationId => "invalid account operation identity",
			Self::RandomnessUnavailable => "account operation randomness unavailable",
			Self::InvalidQuotaWindow => "invalid account quota window",
		})
	}
}

fn is_canonical_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
		})
}

#[cfg(test)]
mod tests {
	use crate::account::{AccountError, AccountId, AccountState};

	#[test]
	fn account_identity_and_health_are_non_secret_closed_types() {
		let account = AccountId::new("10000000-0000-4000-8000-000000000001").unwrap();

		assert_eq!(account.as_str(), "10000000-0000-4000-8000-000000000001");
		assert_eq!(format!("{account:?}"), "AccountId(\"10000000-0000-4000-8000-000000000001\")");
		assert_eq!(
			[
				AccountState::Unavailable,
				AccountState::Unknown,
				AccountState::Available,
				AccountState::Depleted,
				AccountState::AuthFailed,
				AccountState::PluginUnready,
			]
			.len(),
			6
		);
	}

	#[test]
	fn account_identity_rejects_noncanonical_and_provider_shaped_values() {
		for value in [
			"",
			"10000000-0000-4000-8000-00000000000A",
			"private@example.test",
			"sk-proj-0123456789abcdef",
		] {
			assert_eq!(AccountId::new(value), Err(AccountError::InvalidAccountId));
		}
	}
}
