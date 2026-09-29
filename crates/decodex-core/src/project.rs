//! Canonical Project identities retained by domain references.
use std::{
	error::Error,
	fmt::{Display, Formatter},
};

/// Stable canonical Project identity.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProjectId(String);
impl ProjectId {
	/// Parse one canonical lowercase RFC 9562 UUID version 4 identity.
	pub fn new(value: impl Into<String>) -> Result<Self, ProjectError> {
		let value = value.into();

		if !is_canonical_uuid_v4(&value) {
			return Err(ProjectError::InvalidProjectId);
		}

		Ok(Self(value))
	}

	/// Borrow the canonical Project identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl Display for ProjectId {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.0)
	}
}

/// Closed Project identity validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectError {
	/// Project identity was not one canonical UUID version 4.
	InvalidProjectId,
}
impl Error for ProjectError {}
impl Display for ProjectError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidProjectId => "invalid Project identity",
		})
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
	use crate::{ProjectError, ProjectId};
	#[test]
	fn project_ids_reject_noncanonical_uuid_shapes_and_versions() {
		for value in [
			"",
			"10000000-0000-4000-8000-00000000000A",
			"10000000000040008000000000000001",
			"10000000-0000-5000-8000-000000000001",
			"10000000-0000-4000-7000-000000000001",
			"not-a-canonical-project-id",
		] {
			assert_eq!(ProjectId::new(value), Err(ProjectError::InvalidProjectId));
		}
	}
}
