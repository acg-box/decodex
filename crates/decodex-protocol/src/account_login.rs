//! Narrow memory-only account-login service shared by daemon and local clients.

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{
	EntityId, EntityRevision, IdempotencyKey, ProtocolVersion, QueryId, ServerId,
	WireScalarTooLong, WireText,
};

/// Maximum UTF-8 bytes in one provider authorization or verification URL.
///
/// This preserves the established native browser-login acceptance boundary.
pub const MAX_ACCOUNT_LOGIN_URL_BYTES: usize = 8 * 1_024;

/// Bounded provider URL carried only by the ephemeral account-login exchange.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct AccountLoginUrl(String);

impl AccountLoginUrl {
	/// Validate and construct one bounded provider URL.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		let value = value.into();
		if value.len() > MAX_ACCOUNT_LOGIN_URL_BYTES {
			return Err(WireScalarTooLong::new(value.len(), MAX_ACCOUNT_LOGIN_URL_BYTES));
		}
		Ok(Self(value))
	}

	/// Borrow the validated URL text.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl<'de> Deserialize<'de> for AccountLoginUrl {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
	}
}

/// Provider authorization method executed by the daemon and presented by the UI.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountLoginMethod {
	/// PKCE browser authorization through a bounded loopback callback.
	BrowserRedirect,
	/// Structured device-code authorization.
	DeviceCode,
}

/// Daemon-internal AccountService installation selected when login starts.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccountLoginInstallMode {
	/// Enroll a new provisional Account UUID, allowing exact tombstone restoration.
	Enroll {
		/// Durable Account operation identity.
		operation_id: EntityId,
		/// Provisional Account UUID requested by the UI.
		account_id: EntityId,
		/// Initial independent enabled state.
		enabled: bool,
		/// Durable Account command identity used only for final installation.
		idempotency_key: IdempotencyKey,
	},
	/// Replace one exact Account credential under revision and recovery fences.
	Reauthenticate {
		/// Durable Account operation identity.
		operation_id: EntityId,
		/// Existing Account UUID.
		account_id: EntityId,
		/// Exact Account revision observed before login.
		expected_revision: EntityRevision,
		/// Exact ambiguous recovery operation being taken over, when present.
		recovery_operation_id: Option<EntityId>,
		/// Durable Account command identity used only for final installation.
		idempotency_key: IdempotencyKey,
	},
}

/// Complete immutable request identity for one daemon-owned login session.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccountLoginStart {
	/// Ephemeral UUID used to address the one in-memory session.
	pub session_id: EntityId,
	/// Selected provider authorization method.
	pub method: AccountLoginMethod,
	/// Final AccountService installation mode and durable command identity.
	pub install_mode: AccountLoginInstallMode,
}

impl AccountLoginStart {
	/// Validate canonical identities and revision/recovery fences.
	pub fn validate(&self) -> Result<(), AccountLoginContractError> {
		if !is_canonical_uuid(self.session_id.as_str()) {
			return Err(AccountLoginContractError::InvalidIdentity);
		}
		let (operation_id, account_id) = match &self.install_mode {
			AccountLoginInstallMode::Enroll { operation_id, account_id, .. } => {
				(operation_id, account_id)
			},
			AccountLoginInstallMode::Reauthenticate {
				operation_id,
				account_id,
				expected_revision,
				recovery_operation_id,
				..
			} => {
				if expected_revision.0 == 0
					|| recovery_operation_id.as_ref().is_some_and(|value| value == operation_id)
					|| recovery_operation_id
						.as_ref()
						.is_some_and(|value| !is_canonical_uuid(value.as_str()))
				{
					return Err(AccountLoginContractError::InvalidFence);
				}
				(operation_id, account_id)
			},
		};
		if !is_canonical_uuid(operation_id.as_str()) || !is_canonical_uuid(account_id.as_str()) {
			return Err(AccountLoginContractError::InvalidIdentity);
		}
		Ok(())
	}
}

/// One transient Start, Status, or Cancel operation.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccountLoginRequest {
	/// Start or idempotently read one exact session.
	Start {
		/// Complete immutable session request.
		start: Box<AccountLoginStart>,
	},
	/// Read one session's current in-memory status.
	Status {
		/// Ephemeral session UUID.
		session_id: EntityId,
	},
	/// Cancel one session and wait for terminal cleanup.
	Cancel {
		/// Ephemeral session UUID.
		session_id: EntityId,
	},
}

impl AccountLoginRequest {
	/// Borrow the session identity addressed by this operation.
	pub fn session_id(&self) -> &EntityId {
		match self {
			Self::Start { start } => &start.session_id,
			Self::Status { session_id } | Self::Cancel { session_id } => session_id,
		}
	}

	/// Validate the complete request without opening provider or product state.
	pub fn validate(&self) -> Result<(), AccountLoginContractError> {
		match self {
			AccountLoginRequest::Start { start } => start.validate(),
			AccountLoginRequest::Status { session_id }
			| AccountLoginRequest::Cancel { session_id }
				if is_canonical_uuid(session_id.as_str()) => Ok(()),
			AccountLoginRequest::Status { .. } | AccountLoginRequest::Cancel { .. } => {
				Err(AccountLoginContractError::InvalidIdentity)
			},
		}
	}
}

/// One request on the dedicated non-retained account-login exchange.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccountLoginRequestEnvelope {
	/// Exact protocol version.
	pub version: ProtocolVersion,
	/// Request identity scoped to this one-shot socket.
	pub request_id: QueryId,
	/// Transient operation.
	pub request: AccountLoginRequest,
}

/// Structured device prompt presented only by a UI.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccountLoginPrompt {
	/// Provider verification page.
	pub verification_url: AccountLoginUrl,
	/// One-time code copied only by the UI.
	pub user_code: WireText,
}

/// Finite in-memory login state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountLoginState {
	/// Browser authorization is being prepared.
	OpeningBrowser,
	/// A device code is being requested.
	RequestingCode,
	/// The provider is waiting for the user.
	WaitingForBrowser,
	/// AccountService installation is in progress.
	Installing,
	/// Installation completed and names the daemon-resolved Account UUID.
	Completed,
	/// The session ended with one closed failure.
	Failed,
	/// Cancellation completed after exact cleanup.
	Cancelled,
}

/// Closed credential-negative failure projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountLoginFailure {
	/// Provider login did not complete.
	LoginFailed,
	/// The bounded provider deadline elapsed.
	LoginTimedOut,
	/// The provider rejected device authorization.
	DeviceAuthorizationRejected,
	/// The credential belongs to a different provider account.
	AccountMismatch,
	/// The Account revision changed.
	AccountChanged,
	/// The target Account is unavailable for installation.
	AccountUnavailable,
	/// The same provider identity is already enrolled.
	ProviderAlreadyEnrolled,
	/// The exact recovery operation changed.
	RecoveryChanged,
	/// The daemon credential store is unavailable.
	CredentialStoreUnavailable,
	/// The daemon login service is unavailable.
	ServiceUnavailable,
	/// Durable installation may have completed but could not be proved.
	OutcomeUnknown,
	/// No in-memory session matches the requested UUID.
	SessionNotFound,
	/// Another global session is active or the same UUID names different input.
	Busy,
}

/// Current memory-only status for one session.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccountLoginStatus {
	/// Ephemeral session UUID.
	pub session_id: EntityId,
	/// Finite lifecycle state.
	pub state: AccountLoginState,
	/// Structured device prompt, present only while waiting.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub prompt: Option<AccountLoginPrompt>,
	/// Browser authorization URL, present only while waiting.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub authorization_url: Option<AccountLoginUrl>,
	/// Closed terminal failure.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub failure: Option<AccountLoginFailure>,
	/// Exact Account UUID resolved after successful install or restoration.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub resolved_account_id: Option<EntityId>,
}

impl AccountLoginStatus {
	/// Validate state-specific optional fields and all ephemeral identities.
	pub fn validate(&self) -> Result<(), AccountLoginContractError> {
		if !is_canonical_uuid(self.session_id.as_str())
			|| self
				.resolved_account_id
				.as_ref()
				.is_some_and(|value| !is_canonical_uuid(value.as_str()))
		{
			return Err(AccountLoginContractError::InvalidIdentity);
		}
		let waiting_payloads = usize::from(self.prompt.is_some())
			+ usize::from(self.authorization_url.is_some());
		let valid = match self.state {
			AccountLoginState::OpeningBrowser
			| AccountLoginState::RequestingCode
			| AccountLoginState::Installing => {
				waiting_payloads == 0 && self.failure.is_none() && self.resolved_account_id.is_none()
			},
			AccountLoginState::WaitingForBrowser => {
				waiting_payloads == 1 && self.failure.is_none() && self.resolved_account_id.is_none()
			},
			AccountLoginState::Completed => {
				waiting_payloads == 0 && self.failure.is_none() && self.resolved_account_id.is_some()
			},
			AccountLoginState::Failed => {
				waiting_payloads == 0 && self.failure.is_some() && self.resolved_account_id.is_none()
			},
			AccountLoginState::Cancelled => {
				waiting_payloads == 0 && self.failure.is_none() && self.resolved_account_id.is_none()
			},
		};
		if !valid
			|| self.prompt.as_ref().is_some_and(|prompt| {
				prompt.verification_url.as_str().is_empty() || prompt.user_code.as_str().is_empty()
			})
			|| self.authorization_url.as_ref().is_some_and(|url| url.as_str().is_empty())
		{
			return Err(AccountLoginContractError::InvalidStatus);
		}
		Ok(())
	}
}

/// One response on the dedicated non-retained account-login exchange.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccountLoginResponseEnvelope {
	/// Negotiated exact protocol version.
	pub version: ProtocolVersion,
	/// Stable server identity verified by the client.
	pub server_id: ServerId,
	/// Exact request identity echoed from the client.
	pub request_id: QueryId,
	/// Current transient session status.
	pub status: AccountLoginStatus,
}

/// Why one account-login request or status violates the narrow contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountLoginContractError {
	/// One UUID-like identity is not canonical.
	InvalidIdentity,
	/// One revision or recovery relation is invalid.
	InvalidFence,
	/// State-specific optional fields are inconsistent.
	InvalidStatus,
}

fn is_canonical_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'),
		})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn entity(value: &str) -> EntityId {
		EntityId::new(value).expect("fixture entity")
	}

	fn start(method: AccountLoginMethod) -> AccountLoginStart {
		AccountLoginStart {
			session_id: entity("018f0f9e-7b6e-4a31-8f4c-1d2e3f405162"),
			method,
			install_mode: AccountLoginInstallMode::Enroll {
				operation_id: entity("028f0f9e-7b6e-4a31-8f4c-1d2e3f405163"),
				account_id: entity("038f0f9e-7b6e-4a31-8f4c-1d2e3f405164"),
				enabled: true,
				idempotency_key: IdempotencyKey::new("login-fixture")
					.expect("fixture idempotency key"),
			},
		}
	}

	#[test]
	fn both_login_methods_are_valid_start_contracts() {
		for method in [AccountLoginMethod::BrowserRedirect, AccountLoginMethod::DeviceCode] {
			let request = AccountLoginRequest::Start { start: Box::new(start(method)) };
			assert_eq!(request.validate(), Ok(()));
		}
	}

	#[test]
	fn waiting_status_requires_exactly_one_ui_payload() {
		let session_id = entity("018f0f9e-7b6e-4a31-8f4c-1d2e3f405162");
		let invalid = AccountLoginStatus {
			session_id,
			state: AccountLoginState::WaitingForBrowser,
			prompt: None,
			authorization_url: None,
			failure: None,
			resolved_account_id: None,
		};
		assert_eq!(invalid.validate(), Err(AccountLoginContractError::InvalidStatus));
	}

	#[test]
	fn terminal_status_cannot_retain_authorization_material() {
		let status = AccountLoginStatus {
			session_id: entity("018f0f9e-7b6e-4a31-8f4c-1d2e3f405162"),
			state: AccountLoginState::Failed,
			prompt: None,
			authorization_url: Some(
				AccountLoginUrl::new("https://auth.openai.com/fixture").expect("fixture URL"),
			),
			failure: Some(AccountLoginFailure::LoginFailed),
			resolved_account_id: None,
		};
		assert_eq!(status.validate(), Err(AccountLoginContractError::InvalidStatus));
	}

	#[test]
	fn authorization_url_preserves_the_established_eight_kibibyte_boundary() {
		assert!(AccountLoginUrl::new("x".repeat(MAX_ACCOUNT_LOGIN_URL_BYTES)).is_ok());
		assert!(AccountLoginUrl::new("x".repeat(MAX_ACCOUNT_LOGIN_URL_BYTES + 1)).is_err());

		let encoded = serde_json::to_string(
			&AccountLoginUrl::new("x".repeat(MAX_ACCOUNT_LOGIN_URL_BYTES))
				.expect("boundary URL"),
		)
		.expect("encode boundary URL");
		assert!(serde_json::from_str::<AccountLoginUrl>(&encoded).is_ok());
	}
}
