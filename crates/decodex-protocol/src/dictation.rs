//! Transient subscription dictation. Audio and drafts never become command receipts.
use crate::{EntityId, WireScalarTooLong, WireText};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// Bounded private audio or transcript payload, omitted from diagnostics.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DictationBuffer(String);
impl DictationBuffer {
	/// Bound one transient frame.
	pub fn new(value: impl Into<String>) -> Result<Self, WireScalarTooLong> {
		let value = value.into();
		if value.len() > 65_536 {
			return Err(WireScalarTooLong::new(value.len(), 65_536));
		}
		Ok(Self(value))
	}

	/// Borrow the exact in-memory payload.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl std::fmt::Debug for DictationBuffer {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("DictationBuffer([redacted])")
	}
}
impl<'de> Deserialize<'de> for DictationBuffer {
	fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
		Self::new(String::deserialize(d)?).map_err(D::Error::custom)
	}
}
/// Explicit dictation operations. No operation sends a message to an agent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum DictationRequest {
	/// Start one subscription session.
	Start {
		/// Unique caller session identity.
		session_id: EntityId,
	},
	/// Append PCM16 mono audio at 24 kHz.
	Audio {
		/// Exact session.
		session_id: EntityId,
		/// Base64-encoded audio.
		audio: DictationBuffer,
	},
	/// Stop capture and request final correction after all queued audio.
	Finish {
		/// Exact session.
		session_id: EntityId,
	},
	/// Read the latest complete draft projection.
	Poll {
		/// Exact session.
		session_id: EntityId,
	},
	/// Discard this recording without sending a message.
	Cancel {
		/// Exact session.
		session_id: EntityId,
	},
}
impl DictationRequest {
	/// Get the exact caller identity.
	pub fn session_id(&self) -> &EntityId {
		match self {
			Self::Start { session_id }
			| Self::Audio { session_id, .. }
			| Self::Finish { session_id }
			| Self::Poll { session_id }
			| Self::Cancel { session_id } => session_id,
		}
	}
}
/// Dictation lifecycle independent of agent execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationPhase {
	/// Subscription handshake in progress.
	Connecting,
	/// Accepting audio and returning provisional text.
	Listening,
	/// Capture ended; waiting for the final revision.
	Finalizing,
	/// All accepted audio has been finalized, or the caller cancelled.
	Complete,
	/// Recording ended with a visible error; received text remains editable.
	Failed,
}
/// Latest in-memory draft, never persisted as agent input by the service.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DictationStatus {
	/// Exact session.
	pub session_id: EntityId,
	/// Current recording phase.
	pub phase: DictationPhase,
	/// Complete current transcript, replacing older revisions.
	pub text: DictationBuffer,
	/// Safe explanation without raw native errors.
	pub message: Option<WireText>,
}

#[cfg(test)]
mod tests {
	use super::DictationBuffer;

	#[test]
	fn payload_is_bounded_on_decode_and_redacted_from_debug() {
		let private = DictationBuffer::new("private audio or draft").expect("bounded");
		assert_eq!(format!("{private:?}"), "DictationBuffer([redacted])");
		let oversized = serde_json::to_string(&"x".repeat(65_537)).expect("encode");
		assert!(serde_json::from_str::<DictationBuffer>(&oversized).is_err());
		let boundary = DictationBuffer::new("x".repeat(65_536)).expect("boundary");
		assert_eq!(boundary.as_str().len(), 65_536);
	}
}
