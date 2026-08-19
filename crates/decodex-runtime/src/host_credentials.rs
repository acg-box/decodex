//! Narrow versioned host credential storage for daemon-owned account lifecycle.

use std::{
	error::Error,
	fmt::{Debug, Display, Formatter},
};

use decodex_core::{
	AccountId, AccountOperationId, AccountProvider, CredentialBinding, CredentialFingerprint,
	CredentialStoreSchemaVersion, CredentialVersion, ProviderIdentity,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const FINGERPRINT_DOMAIN: &[u8] = b"decodex-host-credential-store-v1\0";
const MAX_CREDENTIAL_RECORD_BYTES: usize = 1024 * 1024;

/// Secret bundle kept only in the host credential store and short-lived daemon memory.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct CredentialSecretBundle {
	access_token: String,
	refresh_token: String,
	id_token: Option<String>,
	plan_type: Option<String>,
	provider_email: String,
	token_type: String,
	access_token_expires_at_unix_micros: i64,
}
impl CredentialSecretBundle {
	/// Construct the complete ChatGPT bundle needed by Codex login and host refresh.
	pub fn chatgpt(
		access_token: String,
		refresh_token: String,
		id_token: Option<String>,
		plan_type: Option<String>,
		provider_email: String,
		token_type: String,
		access_token_expires_at_unix_micros: i64,
	) -> Result<Self, CredentialStoreError> {
		if access_token.is_empty()
			|| refresh_token.is_empty()
			|| provider_email.is_empty()
			|| provider_email.len() > 320
			|| provider_email.chars().any(char::is_control)
			|| !token_type.eq_ignore_ascii_case("bearer")
			|| access_token_expires_at_unix_micros <= 0
		{
			return Err(CredentialStoreError::InvalidBundle);
		}

		Ok(Self {
			access_token,
			refresh_token,
			id_token,
			plan_type,
			provider_email,
			token_type: "bearer".to_owned(),
			access_token_expires_at_unix_micros,
		})
	}

	/// Borrow the access token for one immediate Codex projection.
	pub fn access_token(&self) -> &str {
		&self.access_token
	}

	/// Borrow the refresh token for one serialized provider refresh.
	pub fn refresh_token(&self) -> &str {
		&self.refresh_token
	}

	/// Borrow the optional ID token.
	pub fn id_token(&self) -> Option<&str> {
		self.id_token.as_deref()
	}

	/// Borrow the non-secret plan hint carried with the secret bundle.
	pub fn plan_type(&self) -> Option<&str> {
		self.plan_type.as_deref()
	}

	/// Borrow the provider email used for exact post-login account readback.
	pub fn provider_email(&self) -> &str {
		&self.provider_email
	}

	/// Borrow the closed OAuth token type.
	pub fn token_type(&self) -> &str {
		&self.token_type
	}

	/// Return the exact access-token expiry in Unix microseconds.
	pub const fn access_token_expires_at_unix_micros(&self) -> i64 {
		self.access_token_expires_at_unix_micros
	}

	/// Compute the canonical non-secret binding before a cross-store effect begins.
	pub fn binding_for(
		&self,
		account_id: &AccountId,
		writer_operation_id: &AccountOperationId,
		version: CredentialVersion,
		provider: &ProviderIdentity,
	) -> Result<CredentialBinding, CredentialStoreError> {
		let persisted = PersistedCredentialV1::new(
			account_id,
			writer_operation_id,
			version,
			provider,
			self.clone(),
		);
		let bytes = encode(&persisted)?;

		persisted.binding(fingerprint(&bytes)?)
	}
}
impl Debug for CredentialSecretBundle {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("CredentialSecretBundle([REDACTED])")
	}
}

/// Exact host-store read containing secret material and its canonical non-secret binding.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct StoredCredential {
	#[zeroize(skip)]
	binding: CredentialBinding,
	bundle: CredentialSecretBundle,
}
impl StoredCredential {
	/// Borrow the canonical credential binding.
	pub fn binding(&self) -> &CredentialBinding {
		&self.binding
	}

	/// Borrow the secret bundle for one immediate daemon operation.
	pub fn bundle(&self) -> &CredentialSecretBundle {
		&self.bundle
	}

	/// Consume the read and return its secret bundle.
	pub fn into_bundle(mut self) -> CredentialSecretBundle {
		std::mem::replace(
			&mut self.bundle,
			CredentialSecretBundle {
				access_token: String::new(),
				refresh_token: String::new(),
				id_token: None,
				plan_type: None,
				provider_email: String::new(),
				token_type: String::new(),
				access_token_expires_at_unix_micros: 0,
			},
		)
	}
}
impl Debug for StoredCredential {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("StoredCredential")
			.field("binding", &self.binding)
			.field("bundle", &"[REDACTED]")
			.finish()
	}
}

/// Narrow host credential-store contract. Implementations must make each method atomic.
pub trait HostCredentialStore: Send + Sync {
	/// Create version one. Existing material is an exact typed conflict.
	fn create(
		&self,
		account_id: &AccountId,
		target: &CredentialBinding,
		bundle: CredentialSecretBundle,
	) -> Result<(), CredentialStoreError>;

	/// Recreate an absent bundle only at the immediate successor of the last deleted binding.
	fn restore_absent(
		&self,
		account_id: &AccountId,
		previous: &CredentialBinding,
		target: &CredentialBinding,
		bundle: CredentialSecretBundle,
	) -> Result<(), CredentialStoreError>;

	/// Read only when schema, version, fingerprint, and provider all agree.
	fn read_exact(
		&self,
		account_id: &AccountId,
		expected: &CredentialBinding,
	) -> Result<StoredCredential, CredentialStoreError>;

	/// Rotate only from the exact expected binding to its immediate successor.
	fn compare_and_swap_rotate(
		&self,
		account_id: &AccountId,
		expected: &CredentialBinding,
		target: &CredentialBinding,
		bundle: CredentialSecretBundle,
	) -> Result<(), CredentialStoreError>;

	/// Delete only the exact expected version and fingerprint.
	fn delete(
		&self,
		account_id: &AccountId,
		expected: &CredentialBinding,
	) -> Result<(), CredentialStoreError>;
}

/// Closed store failure that cannot carry credential material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialStoreError {
	/// The protected store or its serialization boundary is unavailable.
	Unavailable,
	/// No exact account item exists.
	NotFound,
	/// Create found an existing account item.
	AlreadyExists,
	/// The current or target credential version is incompatible.
	VersionConflict,
	/// The current serialized bundle digest differs.
	FingerprintMismatch,
	/// The current provider identity differs.
	ProviderMismatch,
	/// Another account record already owns the same provider identity.
	DuplicateProvider,
	/// The serialized account identity differs.
	AccountMismatch,
	/// The current writer operation differs.
	WriterMismatch,
	/// The serialized store schema is not supported.
	UnsupportedSchema,
	/// A caller supplied an invalid secret bundle.
	InvalidBundle,
	/// A stored bundle is malformed or internally inconsistent.
	CorruptBundle,
}
impl Error for CredentialStoreError {}
impl Display for CredentialStoreError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::Unavailable => "host credential store unavailable",
			Self::NotFound => "host credential item not found",
			Self::AlreadyExists => "host credential item already exists",
			Self::VersionConflict => "host credential version conflict",
			Self::FingerprintMismatch => "host credential fingerprint mismatch",
			Self::ProviderMismatch => "host credential provider mismatch",
			Self::DuplicateProvider => "host credential provider already exists",
			Self::AccountMismatch => "host credential account mismatch",
			Self::WriterMismatch => "host credential writer operation mismatch",
			Self::UnsupportedSchema => "host credential schema unsupported",
			Self::InvalidBundle => "host credential bundle invalid",
			Self::CorruptBundle => "host credential bundle corrupt",
		})
	}
}

#[derive(Deserialize, Serialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct PersistedCredentialV1 {
	schema_version: u16,
	account_id: String,
	credential_version: u64,
	writer_operation_id: String,
	provider: String,
	provider_account_id: String,
	access_token: String,
	refresh_token: String,
	id_token: Option<String>,
	plan_type: Option<String>,
	provider_email: String,
	token_type: String,
	access_token_expires_at_unix_micros: i64,
}
impl PersistedCredentialV1 {
	fn new(
		account_id: &AccountId,
		writer_operation_id: &AccountOperationId,
		version: CredentialVersion,
		provider: &ProviderIdentity,
		mut bundle: CredentialSecretBundle,
	) -> Self {
		Self {
			schema_version: CredentialStoreSchemaVersion::V1.get(),
			account_id: account_id.as_str().to_owned(),
			credential_version: version.get(),
			writer_operation_id: writer_operation_id.as_str().to_owned(),
			provider: provider_text(provider.provider()).to_owned(),
			provider_account_id: provider.account_id().to_owned(),
			access_token: std::mem::take(&mut bundle.access_token),
			refresh_token: std::mem::take(&mut bundle.refresh_token),
			id_token: bundle.id_token.take(),
			plan_type: bundle.plan_type.take(),
			provider_email: std::mem::take(&mut bundle.provider_email),
			token_type: std::mem::take(&mut bundle.token_type),
			access_token_expires_at_unix_micros: bundle.access_token_expires_at_unix_micros,
		}
	}

	fn binding(
		&self,
		fingerprint: CredentialFingerprint,
	) -> Result<CredentialBinding, CredentialStoreError> {
		let schema_version = CredentialStoreSchemaVersion::new(self.schema_version)
			.map_err(|_| CredentialStoreError::UnsupportedSchema)?;
		let version = CredentialVersion::new(self.credential_version)
			.map_err(|_| CredentialStoreError::CorruptBundle)?;
		let provider_kind = match self.provider.as_str() {
			"chatgpt" => AccountProvider::Chatgpt,
			_ => return Err(CredentialStoreError::CorruptBundle),
		};
		let provider = ProviderIdentity::new(provider_kind, self.provider_account_id.clone())
			.map_err(|_| CredentialStoreError::CorruptBundle)?;
		let writer_operation_id = AccountOperationId::new(self.writer_operation_id.clone())
			.map_err(|_| CredentialStoreError::CorruptBundle)?;

		Ok(CredentialBinding {
			schema_version,
			version,
			fingerprint,
			provider,
			writer_operation_id,
		})
	}

	fn into_bundle(mut self) -> Result<CredentialSecretBundle, CredentialStoreError> {
		CredentialSecretBundle::chatgpt(
			std::mem::take(&mut self.access_token),
			std::mem::take(&mut self.refresh_token),
			self.id_token.take(),
			self.plan_type.take(),
			std::mem::take(&mut self.provider_email),
			std::mem::take(&mut self.token_type),
			self.access_token_expires_at_unix_micros,
		)
	}

	fn account_id(&self) -> Result<AccountId, CredentialStoreError> {
		AccountId::new(self.account_id.clone()).map_err(|_| CredentialStoreError::CorruptBundle)
	}
}

fn encode(persisted: &PersistedCredentialV1) -> Result<Zeroizing<Vec<u8>>, CredentialStoreError> {
	let bytes = serde_json::to_vec(persisted).map_err(|_| CredentialStoreError::InvalidBundle)?;
	if bytes.len() > MAX_CREDENTIAL_RECORD_BYTES {
		return Err(CredentialStoreError::InvalidBundle);
	}

	Ok(Zeroizing::new(bytes))
}

fn decode(
	bytes: Vec<u8>,
) -> Result<(PersistedCredentialV1, CredentialFingerprint), CredentialStoreError> {
	if bytes.len() > MAX_CREDENTIAL_RECORD_BYTES {
		return Err(CredentialStoreError::CorruptBundle);
	}
	let bytes = Zeroizing::new(bytes);
	let fingerprint = fingerprint(&bytes)?;
	let persisted =
		serde_json::from_slice(&bytes).map_err(|_| CredentialStoreError::CorruptBundle)?;

	Ok((persisted, fingerprint))
}

fn fingerprint(bytes: &[u8]) -> Result<CredentialFingerprint, CredentialStoreError> {
	let mut digest = Sha256::new();
	digest.update(FINGERPRINT_DOMAIN);
	digest.update(bytes);
	CredentialFingerprint::new(
		digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
	)
	.map_err(|_| CredentialStoreError::CorruptBundle)
}

const fn provider_text(provider: AccountProvider) -> &'static str {
	match provider {
		AccountProvider::Chatgpt => "chatgpt",
	}
}

fn enforce_exact(
	actual: &CredentialBinding,
	expected: &CredentialBinding,
) -> Result<(), CredentialStoreError> {
	if actual.schema_version != expected.schema_version {
		return Err(CredentialStoreError::UnsupportedSchema);
	}
	if actual.version != expected.version {
		return Err(CredentialStoreError::VersionConflict);
	}
	if actual.fingerprint != expected.fingerprint {
		return Err(CredentialStoreError::FingerprintMismatch);
	}
	if actual.provider != expected.provider {
		return Err(CredentialStoreError::ProviderMismatch);
	}
	if actual.writer_operation_id != expected.writer_operation_id {
		return Err(CredentialStoreError::WriterMismatch);
	}

	Ok(())
}

/// Seal one exact host-store read after canonical reconstruction and typed comparison.
pub(crate) fn seal_exact_read(
	account_id: &AccountId,
	actual: &CredentialBinding,
	expected: &CredentialBinding,
	bundle: CredentialSecretBundle,
) -> Result<StoredCredential, CredentialStoreError> {
	let recomputed = bundle.binding_for(
		account_id,
		&actual.writer_operation_id,
		actual.version,
		&actual.provider,
	)?;
	enforce_exact(&recomputed, actual)?;
	enforce_exact(actual, expected)?;

	Ok(StoredCredential { binding: actual.clone(), bundle })
}

mod sqlite_store;

pub use sqlite_store::SqliteCredentialStore;
