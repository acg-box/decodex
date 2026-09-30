//! WorkItem identity and state vocabulary retained for historical readback.
use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

macro_rules! stable_id {
	($name:ident, $error:ident, $label:literal) => {
		#[doc = concat!("Stable canonical ", $label, " identity.")]
		#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub struct $name(String);
		impl $name {
			#[doc = concat!("Parse one canonical lowercase RFC 9562 UUID-v4 ", $label, " identity.")]
			pub fn new(value: impl Into<String>) -> Result<Self, WorkItemError> {
				let value = value.into();
				if !is_canonical_uuid_v4(&value) {
					return Err(WorkItemError::$error);
				}
				Ok(Self(value))
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
		impl Serialize for $name {
			fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
			where
				S: Serializer,
			{
				serializer.serialize_str(&self.0)
			}
		}
		impl<'de> Deserialize<'de> for $name {
			fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
			where
				D: Deserializer<'de>,
			{
				Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
			}
		}
	};
}

stable_id!(WorkItemId, InvalidWorkItemId, "WorkItem");

/// Closed WorkItem identity validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkItemError {
	/// WorkItem identity was not canonical UUID-v4 text.
	InvalidWorkItemId,
}
impl Error for WorkItemError {}
impl Display for WorkItemError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("invalid WorkItem identity")
	}
}

/// Complete WorkItem lifecycle vocabulary.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemState {
	/// Untriaged Lead intake.
	Inbox,
	/// Triaged work not yet recorded as ready by the authoritative owner.
	Planned,
	/// Authoritative storage recorded readiness.
	Ready,
	/// Managed execution is active.
	Running,
	/// Output is awaiting or undergoing review.
	Review,
	/// Progress is explicitly prevented.
	Blocked,
	/// Authoritative acceptance and validation recorded success.
	Done,
	/// Work ended without success.
	Canceled,
}
impl WorkItemState {
	/// Canonical persistence spelling.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Inbox => "inbox",
			Self::Planned => "planned",
			Self::Ready => "ready",
			Self::Running => "running",
			Self::Review => "review",
			Self::Blocked => "blocked",
			Self::Done => "done",
			Self::Canceled => "canceled",
		}
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

#[cfg(test)]
mod tests {
	use crate::{WorkItemError, WorkItemId, WorkItemState};

	#[test]
	fn canonical_ids_remain_validated_on_decode() {
		let id = "10000000-0000-4000-8000-000000000001";
		let value = WorkItemId::new(id).unwrap();

		assert_eq!(serde_json::to_value(&value).unwrap(), id);
		assert_eq!(serde_json::from_value::<WorkItemId>(serde_json::json!(id)).unwrap(), value);

		for invalid in
			["10000000-0000-5000-8000-000000000001", "10000000-0000-4000-7000-000000000001"]
		{
			assert_eq!(WorkItemId::new(invalid), Err(WorkItemError::InvalidWorkItemId));
			assert!(serde_json::from_value::<WorkItemId>(serde_json::json!(invalid)).is_err());
		}
	}

	#[test]
	fn historical_state_spellings_remain_compatible() {
		for (state, spelling) in [
			(WorkItemState::Inbox, "inbox"),
			(WorkItemState::Planned, "planned"),
			(WorkItemState::Ready, "ready"),
			(WorkItemState::Running, "running"),
			(WorkItemState::Review, "review"),
			(WorkItemState::Blocked, "blocked"),
			(WorkItemState::Done, "done"),
			(WorkItemState::Canceled, "canceled"),
		] {
			assert_eq!(state.as_str(), spelling);
			assert_eq!(serde_json::to_value(state).unwrap(), spelling);
			assert_eq!(
				serde_json::from_value::<WorkItemState>(serde_json::json!(spelling)).unwrap(),
				state
			);
		}
	}
}
