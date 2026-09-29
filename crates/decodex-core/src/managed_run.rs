//! Canonical ManagedRun identities retained by execution and evidence references.

use std::{
	error::Error,
	fmt::{Display, Formatter},
};

macro_rules! managed_run_id {
	($name:ident, $label:literal, $error:ident) => {
		#[doc = concat!("Canonical ", $label, " identity.")]
		#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub struct $name(String);
		impl $name {
			#[doc = concat!("Parse one lowercase RFC 9562 UUID-v4 ", $label, " identity.")]
			pub fn new(value: impl Into<String>) -> Result<Self, ManagedRunError> {
				let value = value.into();
				if !is_canonical_uuid_v4(&value) {
					return Err(ManagedRunError::$error);
				}
				Ok(Self(value))
			}

			/// Borrow the canonical identity text.
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

managed_run_id!(ManagedRunId, "ManagedRun", InvalidManagedRunId);

/// Closed ManagedRun validation error without caller-controlled text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedRunError {
	/// ManagedRun identity was not canonical UUID-v4 text.
	InvalidManagedRunId,
}
impl Error for ManagedRunError {}
impl Display for ManagedRunError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidManagedRunId => "invalid ManagedRun identity",
		})
	}
}

fn is_canonical_uuid_v4(value: &str) -> bool {
	let bytes = value.as_bytes();
	if bytes.len() != 36
		|| bytes[8] != b'-'
		|| bytes[13] != b'-'
		|| bytes[18] != b'-'
		|| bytes[23] != b'-'
		|| bytes[14] != b'4'
		|| !matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
	{
		return false;
	}
	bytes.iter().enumerate().all(|(index, byte)| {
		matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')
	})
}
