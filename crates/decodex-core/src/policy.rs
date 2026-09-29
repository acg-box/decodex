//! Canonical policy revision references retained by Program and automation domain values.

use std::{
	error::Error,
	fmt::{Display, Formatter},
};

use crate::ProjectId;

/// Stable canonical Policy identity owned only by this module.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PolicyId(String);
impl PolicyId {
	/// Parse one canonical lowercase RFC 9562 UUID version 4 identity.
	pub fn new(value: impl Into<String>) -> Result<Self, PolicyError> {
		let value = value.into();

		if !is_canonical_uuid_v4(&value) {
			return Err(PolicyError::InvalidPolicyId);
		}

		Ok(Self(value))
	}

	/// Borrow the canonical Policy identity.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl Display for PolicyId {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(&self.0)
	}
}

/// Positive immutable revision number within one Policy.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PolicyRevision(u64);
impl PolicyRevision {
	/// Validate one positive policy revision.
	pub const fn new(value: u64) -> Result<Self, PolicyError> {
		if value == 0 { Err(PolicyError::InvalidRevision) } else { Ok(Self(value)) }
	}

	/// Read the positive revision number.
	pub const fn get(self) -> u64 {
		self.0
	}
}

/// Exact Project-owned Policy revision identity imported by downstream domains.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PolicyRevisionId {
	project_id: ProjectId,
	policy_id: PolicyId,
	revision: PolicyRevision,
}
impl PolicyRevisionId {
	/// Bind an exact revision to its authoritative Project and Policy identities.
	pub const fn new(project_id: ProjectId, policy_id: PolicyId, revision: PolicyRevision) -> Self {
		Self { project_id, policy_id, revision }
	}

	/// Owning Project identity.
	pub const fn project_id(&self) -> &ProjectId {
		&self.project_id
	}

	/// Owning Policy identity.
	pub const fn policy_id(&self) -> &PolicyId {
		&self.policy_id
	}

	/// Exact positive revision.
	pub const fn revision(&self) -> PolicyRevision {
		self.revision
	}
}

/// Closed policy identity or revision validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyError {
	/// Policy identity was not one canonical UUID version 4.
	InvalidPolicyId,
	/// Revision was not positive.
	InvalidRevision,
}
impl Error for PolicyError {}
impl Display for PolicyError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::InvalidPolicyId => "invalid Policy identity",
			Self::InvalidRevision => "invalid Policy revision",
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
	use crate::{PolicyError, PolicyId, PolicyRevision};

	#[test]
	fn policy_ids_and_revisions_are_canonical_and_positive() {
		for value in [
			"",
			"30000000-0000-4000-8000-00000000000A",
			"30000000-0000-5000-8000-000000000001",
			"not-a-policy-id",
		] {
			assert_eq!(PolicyId::new(value), Err(PolicyError::InvalidPolicyId));
		}

		assert_eq!(PolicyRevision::new(0), Err(PolicyError::InvalidRevision));
	}
}
