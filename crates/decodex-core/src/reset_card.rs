use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use crate::AccountState;

/// Maximum records in one reset-card account projection or complete card inventory.
pub const MAX_RESET_CARD_ITEMS: usize = 64;
/// Account-metadata field that immutably binds one vNext UUID to its provider identity.
///
/// The value is a credential-negative, domain-separated SHA-256 fingerprint. Provider identity
/// and credential values must never be stored in this field.
pub const RESET_CARD_PROVIDER_BINDING_METADATA_FIELD: &str = "reset_card_provider_binding_sha256";

/// Exact non-negative Unix timestamp reported for a reset card.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ResetCardTimestamp(i64);
impl ResetCardTimestamp {
	/// Validate one exact Unix-second timestamp without rounding or unit conversion.
	pub const fn from_unix_seconds(value: i64) -> Result<Self, ResetCardError> {
		if value < 0 { Err(ResetCardError::NegativeTimestamp) } else { Ok(Self(value)) }
	}

	/// Read the exact Unix seconds reported by the source.
	pub const fn unix_seconds(self) -> i64 {
		self.0
	}
}

/// Public reset-card identity that contains no provider credit identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ResetCardDescriptor {
	granted_at: ResetCardTimestamp,
	expires_at: ResetCardTimestamp,
}
impl ResetCardDescriptor {
	/// Construct a descriptor only when expiry is strictly after grant.
	pub const fn new(
		granted_at: ResetCardTimestamp,
		expires_at: ResetCardTimestamp,
	) -> Result<Self, ResetCardError> {
		if expires_at.0 <= granted_at.0 {
			Err(ResetCardError::ExpirationNotAfterGrant)
		} else {
			Ok(Self { granted_at, expires_at })
		}
	}

	/// Read the exact grant timestamp.
	pub const fn granted_at(self) -> ResetCardTimestamp {
		self.granted_at
	}

	/// Read the exact expiry timestamp.
	pub const fn expires_at(self) -> ResetCardTimestamp {
		self.expires_at
	}
}

/// Terminal outcome returned by a reset-card provider operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetCardConsumeOutcome {
	/// The provider reset the account rate limits.
	Reset,
	/// The account had no rate limit to reset.
	NothingToReset,
	/// The provider could not find an applicable credit.
	NoCredit,
	/// The exact credit was already redeemed.
	AlreadyRedeemed,
}

/// Closed reset-card value-validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetCardError {
	/// A source timestamp was before the Unix epoch.
	NegativeTimestamp,
	/// Expiry was equal to or earlier than grant.
	ExpirationNotAfterGrant,
}
impl Error for ResetCardError {}

impl Display for ResetCardError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::NegativeTimestamp => "reset-card timestamp is before the Unix epoch",
			Self::ExpirationNotAfterGrant => "reset-card expiry is not after its grant",
		})
	}
}

/// Why an account cannot enter the manual reset-card operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManualResetCardAdmissionError {
	/// The account or its host boundary is unavailable.
	AccountUnavailable,
	/// Current account readiness is unknown.
	AccountStateUnknown,
	/// Authentication is absent or was rejected.
	AuthenticationFailed,
	/// Required plugin readiness was not established.
	PluginUnready,
	/// The account was administratively disabled.
	AccountDisabled,
}
impl Error for ManualResetCardAdmissionError {}

impl Display for ManualResetCardAdmissionError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::AccountUnavailable => "account is unavailable for manual reset-card use",
			Self::AccountStateUnknown => "account readiness is unknown for manual reset-card use",
			Self::AuthenticationFailed => "account authentication failed for manual reset-card use",
			Self::PluginUnready => "account plugin is not ready for manual reset-card use",
			Self::AccountDisabled => "account is disabled for manual reset-card use",
		})
	}
}

/// Admit only a manually selected account that can safely host a reset-card operation.
///
/// A depleted account is deliberately admitted because reset-card use exists to recover its
/// exhausted rate limit. This function grants no routing or automatic-selection authority.
pub const fn admit_manual_reset_card_use(
	account_state: AccountState,
) -> Result<(), ManualResetCardAdmissionError> {
	match account_state {
		AccountState::Available | AccountState::Depleted => Ok(()),
		AccountState::Unavailable => Err(ManualResetCardAdmissionError::AccountUnavailable),
		AccountState::Unknown => Err(ManualResetCardAdmissionError::AccountStateUnknown),
		AccountState::AuthFailed => Err(ManualResetCardAdmissionError::AuthenticationFailed),
		AccountState::PluginUnready => Err(ManualResetCardAdmissionError::PluginUnready),
		AccountState::Disabled => Err(ManualResetCardAdmissionError::AccountDisabled),
	}
}

#[cfg(test)]
mod tests {
	use crate::{
		AccountState, MAX_RESET_CARD_ITEMS, ManualResetCardAdmissionError, ResetCardConsumeOutcome,
		ResetCardDescriptor, ResetCardError, ResetCardTimestamp, admit_manual_reset_card_use,
	};

	fn timestamp(value: i64) -> ResetCardTimestamp {
		ResetCardTimestamp::from_unix_seconds(value).unwrap()
	}

	#[test]
	fn timestamp_preserves_exact_nonnegative_unix_seconds() {
		assert_eq!(timestamp(0).unix_seconds(), 0);
		assert_eq!(timestamp(i64::MAX).unix_seconds(), i64::MAX);
		assert_eq!(
			ResetCardTimestamp::from_unix_seconds(-1),
			Err(ResetCardError::NegativeTimestamp)
		);
	}

	#[test]
	fn descriptor_requires_strict_chronology_and_exposes_only_public_times() {
		let descriptor = ResetCardDescriptor::new(timestamp(100), timestamp(200)).unwrap();

		assert_eq!(descriptor.granted_at().unix_seconds(), 100);
		assert_eq!(descriptor.expires_at().unix_seconds(), 200);
		assert_eq!(
			ResetCardDescriptor::new(timestamp(100), timestamp(100)),
			Err(ResetCardError::ExpirationNotAfterGrant)
		);
		assert_eq!(
			ResetCardDescriptor::new(timestamp(200), timestamp(100)),
			Err(ResetCardError::ExpirationNotAfterGrant)
		);
	}

	#[test]
	fn manual_admission_includes_depleted_but_rejects_each_unsafe_state_precisely() {
		assert_eq!(admit_manual_reset_card_use(AccountState::Available), Ok(()));
		assert_eq!(admit_manual_reset_card_use(AccountState::Depleted), Ok(()));
		assert_eq!(
			admit_manual_reset_card_use(AccountState::Unavailable),
			Err(ManualResetCardAdmissionError::AccountUnavailable)
		);
		assert_eq!(
			admit_manual_reset_card_use(AccountState::Unknown),
			Err(ManualResetCardAdmissionError::AccountStateUnknown)
		);
		assert_eq!(
			admit_manual_reset_card_use(AccountState::AuthFailed),
			Err(ManualResetCardAdmissionError::AuthenticationFailed)
		);
		assert_eq!(
			admit_manual_reset_card_use(AccountState::PluginUnready),
			Err(ManualResetCardAdmissionError::PluginUnready)
		);
		assert_eq!(
			admit_manual_reset_card_use(AccountState::Disabled),
			Err(ManualResetCardAdmissionError::AccountDisabled)
		);
	}

	#[test]
	fn provider_terminal_outcome_domain_is_closed() {
		assert_eq!(
			[
				ResetCardConsumeOutcome::Reset,
				ResetCardConsumeOutcome::NothingToReset,
				ResetCardConsumeOutcome::NoCredit,
				ResetCardConsumeOutcome::AlreadyRedeemed,
			]
			.len(),
			4
		);
		assert_eq!(MAX_RESET_CARD_ITEMS, 64);
	}
}
