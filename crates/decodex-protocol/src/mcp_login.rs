//! Ephemeral native MCP sign-in. Authorization URLs never enter command receipts.
use crate::{EntityId, WireScalarTooLong, WireText};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// Bounded private authorization URL, omitted from diagnostics.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct McpAuthorizationUrl(String);
impl McpAuthorizationUrl {
	/// Bound the private link before it enters local transport.
	pub fn new(value: String) -> Result<Self, WireScalarTooLong> {
		if value.len() > 16384 {
			return Err(WireScalarTooLong::new(value.len(), 16384));
		}
		Ok(Self(value))
	}

	/// Borrow the exact link only to open it in the user's browser.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl std::fmt::Debug for McpAuthorizationUrl {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("McpAuthorizationUrl([redacted])")
	}
}
impl<'de> Deserialize<'de> for McpAuthorizationUrl {
	fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
		Self::new(String::deserialize(d)?).map_err(D::Error::custom)
	}
}

/// Explicit same-user sign-in operations. Poll never initiates authentication.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum McpLoginRequest {
	/// Start once or recover the same in-memory request.
	Start {
		/// Unique UI intent identity.
		session_id: EntityId,
		/// Exact task identity.
		work_id: EntityId,
		/// Exact native server name selected by the user.
		server_name: WireText,
	},
	/// Inspect an existing sign-in without replaying a request.
	Poll {
		/// Exact UI intent identity.
		session_id: EntityId,
		/// Exact task identity.
		work_id: EntityId,
	},
}
impl McpLoginRequest {
	/// Read the caller intent identity.
	pub fn session_id(&self) -> &EntityId {
		match self {
			Self::Start { session_id, .. } | Self::Poll { session_id, .. } => session_id,
		}
	}

	/// Read the owning task identity.
	pub fn work_id(&self) -> &EntityId {
		match self {
			Self::Start { work_id, .. } | Self::Poll { work_id, .. } => work_id,
		}
	}
}

/// Native observations are not proof that a tool runtime is connected.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpLoginPhase {
	/// Native discovery or registration is in progress.
	Starting,
	/// A validated authorization URL is ready for explicit browser opening.
	AwaitingUser,
	/// Codex reported successful sign-in; refresh runtime status separately.
	NativeCompleted,
	/// Native sign-in failed or the request was refused.
	Failed,
	/// Request acceptance or completion is uncertain; do not replay automatically.
	Unknown,
	/// The owning native process or thread changed.
	Disconnected,
	/// Local waiting expired; native work may still complete.
	Expired,
}

/// Same-user transient status; no token or PKCE secret is exposed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct McpLoginStatus {
	/// Exact caller intent identity.
	pub session_id: EntityId,
	/// Observed native phase.
	pub phase: McpLoginPhase,
	/// Private authorization link; absent after completion or invalidation.
	pub authorization_url: Option<McpAuthorizationUrl>,
	/// Bounded local explanation, never a raw native authentication error.
	pub message: WireText,
}
