use serde::Serialize;
use sha2::{Digest as _, Sha256};

pub(crate) const MAX_APP_SERVER_FRAME_BYTES: usize = 1_024 * 1_024;

/// Exact Codex CLI build identity used as capability-cache authority.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct BuildId(String);
impl BuildId {
	#[doc(hidden)]
	pub fn from_attestation(
		version: &str,
		executable_digest: &[u8; 32],
	) -> Result<Self, &'static str> {
		if version.trim().is_empty() || version.len() > 256 || version.contains(['\r', '\n', '\0'])
		{
			return Err("Codex build identity is invalid");
		}

		let mut digest = Sha256::new();

		digest.update(version.as_bytes());
		digest.update([0]);
		digest.update(executable_digest);

		Ok(Self(format!("sha256:{}", hex_digest(&digest.finalize()))))
	}

	#[cfg(test)]
	pub(crate) fn for_test(value: &str) -> Self {
		Self::from_attestation(value, &[0; 32]).expect("test build identity must be valid")
	}

	/// Return the opaque exact-build fingerprint.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Opaque Codex thread identifier.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ThreadId(String);
impl ThreadId {
	#[doc(hidden)]
	pub fn from_protocol(value: &str) -> Self {
		Self(Self::normalize(value))
	}

	pub(crate) fn normalize(value: &str) -> String {
		let bytes = value.as_bytes();
		let is_uuid = bytes.len() == 36
			&& [8, 13, 18, 23].into_iter().all(|index| bytes[index] == b'-')
			&& bytes
				.iter()
				.enumerate()
				.all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit());

		if is_uuid {
			value.to_owned()
		} else {
			format!("sha256:{}", hex_digest(&Sha256::digest(value.as_bytes())))
		}
	}

	/// Return the opaque exact identifier.
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

/// Redacted read-only thread-list projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ThreadSummary {
	/// Opaque thread identifier.
	pub id: ThreadId,
	/// Whether Codex reports this thread as archived.
	pub archived: bool,
	/// Opaque parent identifier for a run-local collaboration actor.
	pub parent_thread_id: Option<ThreadId>,
}

fn hex_digest(bytes: &[u8]) -> String {
	bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
