//! Transient, same-user voice signaling. SDP is never a command receipt or log field.
use crate::{EntityId, WireScalarTooLong, WireText};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// Bounded private WebRTC session description, redacted from diagnostics.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct VoiceSdp(String);
impl VoiceSdp {
	/// Validate the maximum native SDP frame size.
	pub fn new(value: String) -> Result<Self, WireScalarTooLong> {
		if value.len() > 65_536 {
			return Err(WireScalarTooLong::new(value.len(), 65_536));
		}

		Ok(Self(value))
	}

	/// Borrow signaling data for the exact in-memory session only.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl std::fmt::Debug for VoiceSdp {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("VoiceSdp([redacted])")
	}
}
impl<'de> Deserialize<'de> for VoiceSdp {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
	}
}

/// Optional settings for one call. Omitted fields retain native configuration and defaults.
#[derive(Clone, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentVoiceOptions {
	/// Native realtime model name for this call only.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub model: Option<WireText>,
	/// Instructions for the backing Agent when voice starts; distinct from the voice prompt.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub start_instructions: Option<crate::HistoryText>,
	/// Instructions for the backing Agent when voice ends.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub end_instructions: Option<crate::HistoryText>,
}
impl std::fmt::Debug for AgentVoiceOptions {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("AgentVoiceOptions").finish_non_exhaustive()
	}
}

/// Explicit ephemeral media operations. Lost responses never authorize a new call.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentVoiceRequest {
	/// Authorize live voice on an existing Agent. The service persists the call identity only.
	Start {
		/// Caller-generated unique call identity.
		session_id: EntityId,
		/// Exact existing Agent identity.
		work_id: EntityId,
		/// Private local WebRTC offer.
		offer: VoiceSdp,
		/// Immutable options captured before this call starts.
		#[serde(default)]
		options: AgentVoiceOptions,
	},
	/// Read selected reply text through an existing call. Send once; do not replay on timeout.
	Speak {
		/// Exact call identity.
		session_id: EntityId,
		/// Displayed assistant text chosen by the user, bounded to one history frame.
		text: crate::HistoryText,
	},
	/// Observe the exact call and renew its UI-presence lease.
	Poll {
		/// Exact call identity, never an implicit current session.
		session_id: EntityId,
	},
	/// End media. This does not interrupt an already-running agent task.
	Stop {
		/// Exact call identity.
		session_id: EntityId,
	},
}
impl AgentVoiceRequest {
	/// Get the caller's stable call identity.
	pub fn session_id(&self) -> &EntityId {
		match self {
			Self::Start { session_id, .. }
			| Self::Speak { session_id, .. }
			| Self::Poll { session_id }
			| Self::Stop { session_id } => session_id,
		}
	}
}

/// Service-side signaling state; the native media host separately proves audio connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentVoicePhase {
	/// Waiting for native subscription signaling.
	Connecting,
	/// A remote SDP answer is available.
	Ready,
	/// Native voice has positively ended.
	Ended,
	/// Voice did not start or its connection was lost. Never replay input automatically.
	Failed,
}

/// Bounded transient call readback. No account token or audio is carried here.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentVoiceStatus {
	/// Exact caller identity.
	pub session_id: EntityId,
	/// Current signaling state.
	pub phase: AgentVoicePhase,
	/// Private remote session description, only while this call is active.
	pub answer: Option<VoiceSdp>,
	/// Safe user-facing explanation, never a raw provider error.
	pub message: Option<WireText>,
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn signaling_is_bounded_and_redacted() {
		let sdp = VoiceSdp::new("private-ice-password".into()).unwrap();

		assert!(!format!("{sdp:?}").contains("private-ice"));
		assert!(VoiceSdp::new("x".repeat(65_537)).is_err());
		assert!(serde_json::from_value::<VoiceSdp>(serde_json::json!("x".repeat(65_537))).is_err());
	}
}
