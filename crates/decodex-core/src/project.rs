//! Canonical Project and repository identities retained by domain references.
use std::{
	error::Error,
	fmt::{Display, Formatter},
};

/// Maximum UTF-8 bytes in one stable repository identity.
pub const MAX_REPOSITORY_IDENTITY_BYTES: usize = 128;

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

/// Stable repository identity independent from its current server-host root.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepositoryIdentity(String);
impl RepositoryIdentity {
	/// Parse bounded canonical lowercase repository identity text.
	pub fn new(value: impl Into<String>) -> Result<Self, ProjectError> {
		let value = value.into();

		if !is_canonical_repository_identity(&value) {
			return Err(ProjectError::InvalidRepositoryIdentity);
		}

		Ok(Self(value))
	}

	/// Borrow the canonical repository identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl Display for RepositoryIdentity {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.0)
	}
}

/// Closed Project or repository identity validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectError {
	/// Project identity was not one canonical UUID version 4.
	InvalidProjectId,
	/// Repository identity was empty, unbounded, or noncanonical.
	InvalidRepositoryIdentity,
}
impl Error for ProjectError {}
impl Display for ProjectError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidProjectId => "invalid Project identity",
			Self::InvalidRepositoryIdentity => "invalid repository identity",
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

fn is_canonical_repository_identity(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= MAX_REPOSITORY_IDENTITY_BYTES
		&& value.bytes().all(|byte| {
			byte.is_ascii_lowercase()
				|| byte.is_ascii_digit()
				|| matches!(byte, b'-' | b'_' | b'.' | b'/')
		})
		&& value.split('/').all(|segment| {
			!segment.is_empty()
				&& !matches!(segment, "." | "..")
				&& segment.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
				&& segment.as_bytes().last().is_some_and(u8::is_ascii_alphanumeric)
		})
}

#[cfg(test)]
mod tests {
	use crate::{ProjectError, ProjectId, RepositoryIdentity};
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

	#[test]
	fn repository_identity_is_canonical() {
		assert_eq!(RepositoryIdentity::new("acg-box/decodex").unwrap().as_str(), "acg-box/decodex");
		for identity in ["", "Acg-Box/decodex", "acg-box//decodex", "../decodex"] {
			assert_eq!(
				RepositoryIdentity::new(identity),
				Err(ProjectError::InvalidRepositoryIdentity)
			);
		}
	}
}
