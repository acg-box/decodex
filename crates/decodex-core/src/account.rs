use std::{
	error::Error,
	fmt::{Display, Formatter},
};

/// Stable non-secret Decodex account identity.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
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
	/// The account was administratively disabled.
	Disabled,
}

/// Closed account-domain validation failure without caller-provided text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountError {
	/// The account identity was not one canonical lower-case UUID.
	InvalidAccountId,
}
impl Error for AccountError {}

impl Display for AccountError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("invalid account identity")
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
				AccountState::Disabled,
			]
			.len(),
			7
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
