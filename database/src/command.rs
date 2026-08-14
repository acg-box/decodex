use sha2::{Digest as _, Sha256};

use crate::StoreError;

/// Exact idempotency key and digest of one canonical logical request.
#[derive(Clone)]
pub struct CommandIdentity {
	pub(crate) key: String,
	pub(crate) request_hash: String,
}

impl CommandIdentity {
	pub fn new(key: impl Into<String>, request: &[u8]) -> Result<Self, StoreError> {
		let key = key.into();
		if key.is_empty() || key.len() > 256 {
			return Err(StoreError::InvalidInput("idempotency key must contain 1..=256 bytes"));
		}
		if decodex_core::contains_credential_material(&key) {
			return Err(StoreError::CredentialRejected);
		}
		let request_hash =
			Sha256::digest(request).iter().map(|byte| format!("{byte:02x}")).collect();
		Ok(Self { key, request_hash })
	}
}
