//! Bounded repository revision values shared by context and validation evidence.
use std::{
	error::Error,
	fmt::{Display, Formatter},
};

/// Opaque exact repository content revision. This value grants no Git write authority.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepositoryContentRevision(String);
impl RepositoryContentRevision {
	/// Parse one nonempty canonical revision of at most 256 UTF-8 bytes.
	pub fn new(value: impl Into<String>) -> Result<Self, RepositoryRevisionError> {
		let value = value.into();
		if value.is_empty()
			|| value.len() > 256
			|| value.trim() != value
			|| value.chars().any(char::is_control)
		{
			return Err(RepositoryRevisionError);
		}
		Ok(Self(value))
	}

	/// Borrow the exact revision.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}
impl Display for RepositoryContentRevision {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		f.write_str(&self.0)
	}
}
/// A repository revision is empty, oversized, or noncanonical.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryRevisionError;
impl Display for RepositoryRevisionError {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		f.write_str("invalid repository content revision")
	}
}
impl Error for RepositoryRevisionError {}

#[cfg(test)]
mod tests {
	use super::RepositoryContentRevision;
	#[test]
	fn revision_preserves_exact_bytes_and_rejects_ambiguous_or_unbounded_values() {
		for value in ["", " leading", "trailing ", "line\nbreak", "nul\0byte"] {
			assert!(RepositoryContentRevision::new(value).is_err());
		}
		assert!(RepositoryContentRevision::new("x".repeat(257)).is_err());
		assert!(RepositoryContentRevision::new("é".repeat(129)).is_err());
		for value in ["repository-v1".to_owned(), "x".repeat(256)] {
			let revision = RepositoryContentRevision::new(value.clone()).expect("valid revision");
			assert_eq!(revision.as_str(), value);
			assert_eq!(revision.to_string(), value);
		}
	}
}
