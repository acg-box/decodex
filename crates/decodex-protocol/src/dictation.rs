//! Transient subscription dictation. Audio and drafts never become command receipts.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::{
	collections::VecDeque,
	fmt::{self, Debug, Formatter},
};

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

use crate::{EntityId, WireScalarTooLong, WireText};

/// Bounded private transcript payload, omitted from diagnostics.
#[derive(Clone, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DictationBuffer(String);

/// Maximum decoded PCM bytes in one dictation audio request.
pub const MAX_DICTATION_PCM_BYTES: usize = 32_768;
/// Capture and subscription format: PCM16 little-endian, mono.
pub const DICTATION_SAMPLE_RATE: u32 = 24_000;
/// Maximum buffered capture while the subscription connection catches up (10 seconds).
pub const MAX_DICTATION_BACKLOG_BYTES: usize = DICTATION_SAMPLE_RATE as usize * 2 * 10;

/// Bounded, complete PCM16 samples at [`DICTATION_SAMPLE_RATE`], supplied as mono audio.
/// The wire representation remains a base64 string.
/// Construction, deserialization and batching all enforce the same audio contract.
#[derive(Clone, Eq, PartialEq)]
pub struct DictationAudio(Vec<u8>);

/// Audio is empty, malformed, oversized, or contains a partial PCM16 sample.
#[derive(Debug)]
pub struct InvalidDictationAudio;
impl fmt::Display for InvalidDictationAudio {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str("invalid dictation PCM16 audio")
	}
}
impl std::error::Error for InvalidDictationAudio {}
impl DictationAudio {
	/// Validate decoded audio without exposing its contents in an error.
	pub fn from_pcm(pcm: Vec<u8>) -> Result<Self, InvalidDictationAudio> {
		if pcm.is_empty() || pcm.len() > MAX_DICTATION_PCM_BYTES || !pcm.len().is_multiple_of(2) {
			return Err(InvalidDictationAudio);
		}
		Ok(Self(pcm))
	}

	/// Decode and validate the string used by the native capture bridge.
	pub fn from_base64(encoded: &str) -> Result<Self, InvalidDictationAudio> {
		if encoded.len() > MAX_DICTATION_PCM_BYTES.div_ceil(3) * 4 {
			return Err(InvalidDictationAudio);
		}
		Self::from_pcm(STANDARD.decode(encoded).map_err(|_| InvalidDictationAudio)?)
	}

	/// Borrow complete PCM16 samples for transport and accounting.
	pub fn pcm(&self) -> &[u8] {
		&self.0
	}

	/// Encode an already validated packet for the subscription transport.
	pub fn to_base64(&self) -> String {
		STANDARD.encode(&self.0)
	}

	/// Drain available frames in order, leaving the next complete packet queued.
	pub fn take_batch(queue: &mut VecDeque<Self>) -> Option<Self> {
		let mut packet = queue.pop_front()?;
		while let Some(next) = queue.front() {
			if packet.0.len() + next.0.len() > MAX_DICTATION_PCM_BYTES {
				break;
			}
			packet.0.extend(queue.pop_front().expect("front exists").0);
		}
		Some(packet)
	}
}
impl Debug for DictationAudio {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str("DictationAudio([redacted])")
	}
}
impl Serialize for DictationAudio {
	fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.serialize_str(&self.to_base64())
	}
}
impl<'de> Deserialize<'de> for DictationAudio {
	fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
		Self::from_base64(&String::deserialize(d)?).map_err(D::Error::custom)
	}
}

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
impl Debug for DictationBuffer {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str("DictationBuffer([redacted])")
	}
}
impl<'de> Deserialize<'de> for DictationBuffer {
	fn deserialize<D>(d: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		Self::new(String::deserialize(d)?).map_err(D::Error::custom)
	}
}
/// Explicit dictation operations. No operation sends a message to an agent.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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
		audio: DictationAudio,
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
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
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
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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
	use crate::DictationBuffer;

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

#[cfg(test)]
mod audio_tests {
	use super::*;
	#[test]
	fn audio_contract_rejects_bad_wire_data_before_it_reaches_the_service() {
		for encoded in [
			String::new(),
			"not base64".into(),
			STANDARD.encode([1]),
			STANDARD.encode(vec![0; MAX_DICTATION_PCM_BYTES + 2]),
		] {
			let request =
				serde_json::json!({"operation":"audio","session_id":"recording","audio":encoded});
			assert!(serde_json::from_value::<DictationRequest>(request).is_err());
		}
		let packet = DictationAudio::from_pcm(vec![0; MAX_DICTATION_PCM_BYTES]).unwrap();
		let encoded = serde_json::to_string(&packet).unwrap();
		assert_eq!(serde_json::from_str::<DictationAudio>(&encoded).unwrap(), packet);
		assert_eq!(format!("{packet:?}"), "DictationAudio([redacted])");
	}
	#[test]
	fn connection_backlog_is_lossless_ordered_and_valid_at_the_service_boundary() {
		let expected: Vec<u8> = (0..48_000).map(|i| (i % 251) as u8).collect();
		let mut queue = expected
			.chunks(960)
			.map(|pcm| DictationAudio::from_pcm(pcm.to_vec()).unwrap())
			.collect();
		let mut delivered = Vec::new();
		let mut packets = 0;
		while let Some(audio) = DictationAudio::take_batch(&mut queue) {
			let request =
				DictationRequest::Audio { session_id: EntityId::new("recording").unwrap(), audio };
			let decoded: DictationRequest =
				serde_json::from_slice(&serde_json::to_vec(&request).unwrap()).unwrap();
			let DictationRequest::Audio { audio, .. } = decoded else { panic!("audio request") };
			assert!(audio.pcm().len() <= MAX_DICTATION_PCM_BYTES);
			delivered.extend_from_slice(audio.pcm());
			packets += 1;
		}
		assert_eq!(delivered, expected);
		assert_eq!(packets, 2);
	}
}
