//! Inert Automation firing input, delivery intent, and delivery receipt values.
//!
//! These values retain exact identities, content, and chronology. They do not schedule,
//! persist, deliver, acknowledge, decide, retry, or create downstream work.
//!
//! Transactional persistence must enforce equal-value replay, one intent per firing, one
//! receipt per intent, and rejection of identity/content conflicts. A delivery retry must
//! use the same intent identity. These are persistence obligations, not claims made by
//! constructing a pure-core value.
//!
//! A missing receipt means that delivery is unresolved. It does not mean failed,
//! acknowledged, or completed.

use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use crate::{AutomationFiring, AutomationTimestamp, BlobHash};

macro_rules! stable_delivery_id {
	($name:ident, $error:ident, $label:literal) => {
		#[doc = concat!("Stable canonical ", $label, " identity.")]
		#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub struct $name(String);
		impl $name {
			#[doc = concat!("Parse one canonical lowercase RFC 9562 UUID-v4 ", $label, " identity.")]
			pub fn new(value: impl AsRef<str>) -> Result<Self, AutomationDeliveryError> {
				let value = value.as_ref();

				if !is_canonical_uuid_v4(value) {
					return Err(AutomationDeliveryError::$error);
				}

				Ok(Self(value.to_owned()))
			}

			#[doc = concat!("Borrow the canonical ", $label, " identity.")]
			pub fn as_str(&self) -> &str {
				&self.0
			}
		}
		impl Display for $name {
			fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
				formatter.write_str(&self.0)
			}
		}
	};
}

stable_delivery_id!(AutomationDeliveryIntentId, InvalidIntentId, "Automation delivery intent");
stable_delivery_id!(AutomationDeliveryReceiptId, InvalidReceiptId, "Automation delivery receipt");

/// Closed Automation delivery validation failure without caller-controlled text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutomationDeliveryError {
	/// Delivery intent identity was not canonical lowercase UUID-v4 text.
	InvalidIntentId,
	/// Delivery receipt identity was not canonical lowercase UUID-v4 text.
	InvalidReceiptId,
	/// The delivery intent time was before its firing due time.
	IntentBeforeFiring,
	/// The inbox acceptance time was before the delivery intent time.
	ReceiptBeforeIntent,
}
impl Error for AutomationDeliveryError {}

impl Display for AutomationDeliveryError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidIntentId => "invalid Automation delivery intent identity",
			Self::InvalidReceiptId => "invalid Automation delivery receipt identity",
			Self::IntentBeforeFiring => "Automation delivery intent is before its firing",
			Self::ReceiptBeforeIntent => "Automation delivery receipt is before its intent",
		})
	}
}

/// Immutable exact firing and content-addressed payload input.
///
/// This value retains no payload bytes, does not interpret the definition payload schema,
/// and does not make a materiality decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationFiringInput {
	firing: AutomationFiring,
	payload_hash: BlobHash,
}
impl AutomationFiringInput {
	/// Bind one exact firing to one canonical payload content identity.
	pub fn new(firing: AutomationFiring, payload_hash: BlobHash) -> Self {
		Self { firing, payload_hash }
	}

	/// Exact immutable Automation firing.
	pub const fn firing(&self) -> &AutomationFiring {
		&self.firing
	}

	/// Canonical SHA-256 identity of the exact payload.
	pub const fn payload_hash(&self) -> BlobHash {
		self.payload_hash
	}
}

/// Immutable inert intent to deliver one complete firing input to its exact target inbox.
///
/// Constructing this value does not persist or deliver it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationDeliveryIntent {
	id: AutomationDeliveryIntentId,
	input: AutomationFiringInput,
	intended_at: AutomationTimestamp,
}
impl AutomationDeliveryIntent {
	/// Bind a stable intent identity to one complete input and finite intent time.
	pub fn new(
		id: AutomationDeliveryIntentId,
		input: AutomationFiringInput,
		intended_at: AutomationTimestamp,
	) -> Result<Self, AutomationDeliveryError> {
		if intended_at < input.firing().due_at() {
			return Err(AutomationDeliveryError::IntentBeforeFiring);
		}

		Ok(Self { id, input, intended_at })
	}

	/// Stable delivery intent identity.
	pub const fn id(&self) -> &AutomationDeliveryIntentId {
		&self.id
	}

	/// Complete exact firing input.
	pub const fn input(&self) -> &AutomationFiringInput {
		&self.input
	}

	/// Finite time when delivery was intended.
	pub const fn intended_at(&self) -> AutomationTimestamp {
		self.intended_at
	}
}

/// Immutable inert record that the exact target inbox durably accepted one exact intent.
///
/// This receipt is structural data and readback only. Constructing it cannot grant
/// persistence or delivery authority. It is not recipient acknowledgement, response,
/// decision, resulting work, or completion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationDeliveryReceipt {
	id: AutomationDeliveryReceiptId,
	intent: AutomationDeliveryIntent,
	accepted_at: AutomationTimestamp,
}
impl AutomationDeliveryReceipt {
	/// Bind a stable receipt identity to one exact intent and finite inbox acceptance time.
	pub fn new(
		id: AutomationDeliveryReceiptId,
		intent: AutomationDeliveryIntent,
		accepted_at: AutomationTimestamp,
	) -> Result<Self, AutomationDeliveryError> {
		if accepted_at < intent.intended_at() {
			return Err(AutomationDeliveryError::ReceiptBeforeIntent);
		}

		Ok(Self { id, intent, accepted_at })
	}

	/// Stable delivery receipt identity.
	pub const fn id(&self) -> &AutomationDeliveryReceiptId {
		&self.id
	}

	/// Exact immutable delivery intent accepted by the target inbox.
	pub const fn intent(&self) -> &AutomationDeliveryIntent {
		&self.intent
	}

	/// Finite time when the exact target inbox accepted the intent.
	pub const fn accepted_at(&self) -> AutomationTimestamp {
		self.accepted_at
	}
}

fn is_canonical_uuid_v4(value: &str) -> bool {
	let bytes = value.as_bytes();

	bytes.len() == 36
		&& bytes[8] == b'-'
		&& bytes[13] == b'-'
		&& bytes[18] == b'-'
		&& bytes[23] == b'-'
		&& bytes[14] == b'4'
		&& matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
		&& bytes.iter().enumerate().all(|(index, byte)| {
			matches!(index, 8 | 13 | 18 | 23)
				|| byte.is_ascii_digit()
				|| matches!(byte, b'a'..=b'f')
		})
}
