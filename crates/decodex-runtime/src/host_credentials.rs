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
	refresh_token: Option<String>,
	id_token: Option<String>,
	plan_type: Option<String>,
	provider_email: Option<String>,
	token_type: String,
	access_token_expires_at_unix_micros: Option<i64>,
	personal_access_token_user_id: Option<String>,
}
impl CredentialSecretBundle {
	/// Construct the complete ChatGPT bundle needed by Codex login and host refresh.
	pub fn chatgpt(
		access_token: String,
		refresh_token: String,
		id_token: Option<String>,
		plan_type: Option<String>,
		provider_email: Option<String>,
		token_type: String,
		access_token_expires_at_unix_micros: i64,
	) -> Result<Self, CredentialStoreError> {
		if access_token.is_empty()
			|| refresh_token.is_empty()
			|| provider_email.as_ref().is_some_and(|email| {
				email.is_empty() || email.len() > 320 || email.chars().any(char::is_control)
			})
			|| !token_type.eq_ignore_ascii_case("bearer")
			|| access_token_expires_at_unix_micros <= 0
		{
			return Err(CredentialStoreError::InvalidBundle);
		}

		Ok(Self {
			access_token,
			refresh_token: Some(refresh_token),
			id_token,
			plan_type,
			provider_email,
			token_type: "bearer".to_owned(),
			access_token_expires_at_unix_micros: Some(access_token_expires_at_unix_micros),
			personal_access_token_user_id: None,
		})
	}

	/// Construct a PAT bundle after the native whoami endpoint verifies its account and user.
	/// PAT credentials do not have an OAuth refresh token or a JWT expiry.
	pub fn personal_access_token(
		access_token: String,
		user_id: String,
		plan_type: Option<String>,
		provider_email: Option<String>,
	) -> Result<Self, CredentialStoreError> {
		if access_token.is_empty()
			|| access_token.len() > 64 * 1024
			|| access_token.chars().any(char::is_control)
			|| user_id.is_empty()
			|| user_id.len() > 512
			|| user_id.chars().any(char::is_control)
			|| provider_email.as_ref().is_some_and(|email| {
				email.is_empty() || email.len() > 320 || email.chars().any(char::is_control)
			}) {
			return Err(CredentialStoreError::InvalidBundle);
		}
		Ok(Self {
			access_token,
			refresh_token: None,
			id_token: None,
			plan_type,
			provider_email,
			token_type: "bearer".to_owned(),
			access_token_expires_at_unix_micros: None,
			personal_access_token_user_id: Some(user_id),
		})
	}

	/// Whether this bundle uses native personal-access-token authentication.
	pub fn is_personal_access_token(&self) -> bool {
		self.personal_access_token_user_id.is_some()
	}

	/// Borrow the user identity returned by PAT whoami.
	pub fn personal_access_token_user_id(&self) -> Option<&str> {
		self.personal_access_token_user_id.as_deref()
	}

	/// Borrow the access token for one immediate Codex projection.
	pub fn access_token(&self) -> &str {
		&self.access_token
	}

	/// Borrow the refresh token for one serialized provider refresh.
	pub fn refresh_token(&self) -> Option<&str> {
		self.refresh_token.as_deref()
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
	pub fn provider_email(&self) -> Option<&str> {
		self.provider_email.as_deref()
	}

	/// Borrow the bearer token type shared by OAuth and PAT authentication.
	pub fn token_type(&self) -> &str {
		&self.token_type
	}

	/// Return the JWT expiry in Unix microseconds. PAT credentials have no JWT expiry.
	pub const fn access_token_expires_at_unix_micros(&self) -> Option<i64> {
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
		let persisted = PersistedCredential::new(
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
				refresh_token: None,
				id_token: None,
				plan_type: None,
				provider_email: None,
				token_type: String::new(),
				access_token_expires_at_unix_micros: None,
				personal_access_token_user_id: None,
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
struct PersistedCredential {
	schema_version: u16,
	account_id: String,
	credential_version: u64,
	writer_operation_id: String,
	provider: String,
	provider_account_id: String,
	access_token: String,
	refresh_token: Option<String>,
	id_token: Option<String>,
	plan_type: Option<String>,
	provider_email: Option<String>,
	token_type: String,
	access_token_expires_at_unix_micros: Option<i64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	personal_access_token_user_id: Option<String>,
}
impl PersistedCredential {
	fn new(
		account_id: &AccountId,
		writer_operation_id: &AccountOperationId,
		version: CredentialVersion,
		provider: &ProviderIdentity,
		mut bundle: CredentialSecretBundle,
	) -> Self {
		Self {
			schema_version: if bundle.is_personal_access_token() {
				CredentialStoreSchemaVersion::V2.get()
			} else {
				CredentialStoreSchemaVersion::V1.get()
			},
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
			personal_access_token_user_id: bundle.personal_access_token_user_id.take(),
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
		if self.schema_version == CredentialStoreSchemaVersion::V2.get() {
			if self.refresh_token.is_some()
				|| self.id_token.is_some()
				|| self.access_token_expires_at_unix_micros.is_some()
				|| self.token_type != "bearer"
			{
				return Err(CredentialStoreError::CorruptBundle);
			}
			return CredentialSecretBundle::personal_access_token(
				std::mem::take(&mut self.access_token),
				self.personal_access_token_user_id
					.take()
					.ok_or(CredentialStoreError::CorruptBundle)?,
				self.plan_type.take(),
				self.provider_email.take(),
			);
		}
		if self.schema_version != CredentialStoreSchemaVersion::V1.get()
			|| self.personal_access_token_user_id.is_some()
		{
			return Err(CredentialStoreError::CorruptBundle);
		}
		CredentialSecretBundle::chatgpt(
			std::mem::take(&mut self.access_token),
			self.refresh_token.take().ok_or(CredentialStoreError::CorruptBundle)?,
			self.id_token.take(),
			self.plan_type.take(),
			std::mem::take(&mut self.provider_email),
			std::mem::take(&mut self.token_type),
			self.access_token_expires_at_unix_micros.ok_or(CredentialStoreError::CorruptBundle)?,
		)
	}

	fn account_id(&self) -> Result<AccountId, CredentialStoreError> {
		AccountId::new(self.account_id.clone()).map_err(|_| CredentialStoreError::CorruptBundle)
	}
}

fn encode(persisted: &PersistedCredential) -> Result<Zeroizing<Vec<u8>>, CredentialStoreError> {
	let bytes = Zeroizing::new(
		serde_json::to_vec(persisted).map_err(|_| CredentialStoreError::InvalidBundle)?,
	);
	if bytes.len() > MAX_CREDENTIAL_RECORD_BYTES {
		return Err(CredentialStoreError::InvalidBundle);
	}

	Ok(bytes)
}

fn decode(
	bytes: Vec<u8>,
) -> Result<(PersistedCredential, CredentialFingerprint), CredentialStoreError> {
	let bytes = Zeroizing::new(bytes);
	if bytes.len() > MAX_CREDENTIAL_RECORD_BYTES {
		return Err(CredentialStoreError::CorruptBundle);
	}
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

#[cfg(test)]
mod optional_email_tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn stored_credentials_preserve_null_email_and_read_existing_string_email() {
		for email in [None, Some(Value::Null), Some(Value::String("user@example.test".into()))] {
			let mut record = json!({"schema_version":1,"account_id":"account","credential_version":1,"writer_operation_id":"operation","provider":"chatgpt","provider_account_id":"provider","access_token":"synthetic-access","refresh_token":"synthetic-refresh","id_token":null,"plan_type":"pro","token_type":"bearer","access_token_expires_at_unix_micros":100});
			if let Some(email) = email.clone() {
				record["provider_email"] = email;
			}
			let decoded: PersistedCredential = serde_json::from_value(record).unwrap();
			let encoded = serde_json::to_value(&decoded).unwrap();
			let bundle = decoded.into_bundle().unwrap();
			assert_eq!(bundle.provider_email(), email.as_ref().and_then(Value::as_str));
			assert_eq!(encoded["provider_email"], email.unwrap_or(Value::Null));
		}
	}
	use serde_json::Value;
}

#[cfg(test)]
mod personal_access_token_tests {
	use super::*;

	const OAUTH_RECORD: &[u8] = br#"{"schema_version":1,"account_id":"account","credential_version":1,"writer_operation_id":"operation","provider":"chatgpt","provider_account_id":"provider","access_token":"synthetic-access","refresh_token":"synthetic-refresh","id_token":null,"plan_type":"pro","provider_email":null,"token_type":"bearer","access_token_expires_at_unix_micros":100}"#;

	#[test]
	fn oauth_record_keeps_exact_bytes_and_fingerprint() {
		let (record, original_fingerprint) = decode(OAUTH_RECORD.to_vec()).unwrap();
		let encoded = encode(&record).unwrap();
		assert_eq!(encoded.as_slice(), OAUTH_RECORD);
		assert_eq!(fingerprint(&encoded).unwrap(), original_fingerprint);
		let bundle = record.into_bundle().unwrap();
		assert!(!bundle.is_personal_access_token());
		assert_eq!(bundle.refresh_token(), Some("synthetic-refresh"));
		assert_eq!(bundle.access_token_expires_at_unix_micros(), Some(100));
	}

	#[test]
	fn pat_roundtrip_preserves_identity_without_oauth_fields() {
		let mut record: serde_json::Value = serde_json::from_slice(OAUTH_RECORD).unwrap();
		record["account_id"] = "20000000-0000-4000-8000-000000000039".into();
		record["writer_operation_id"] = "30000000-0000-4000-8000-000000000039".into();
		record["schema_version"] = 2.into();
		record["refresh_token"] = serde_json::Value::Null;
		record["access_token_expires_at_unix_micros"] = serde_json::Value::Null;
		record["personal_access_token_user_id"] = "pat-user".into();
		let (decoded, _) = decode(serde_json::to_vec(&record).unwrap()).unwrap();
		let bundle = decoded.into_bundle().unwrap();
		assert!(bundle.is_personal_access_token());
		assert_eq!(bundle.personal_access_token_user_id(), Some("pat-user"));
		assert_eq!(bundle.refresh_token(), None);
		assert_eq!(bundle.access_token_expires_at_unix_micros(), None);
		let persisted = PersistedCredential::new(
			&AccountId::new("20000000-0000-4000-8000-000000000039").unwrap(),
			&AccountOperationId::new("30000000-0000-4000-8000-000000000039").unwrap(),
			CredentialVersion::new(1).unwrap(),
			&ProviderIdentity::new(AccountProvider::Chatgpt, "provider").unwrap(),
			bundle,
		);
		assert_eq!(serde_json::to_value(&persisted).unwrap(), record);
		assert!(!format!("{:?}", persisted.into_bundle().unwrap()).contains("synthetic-access"));
	}

	#[test]
	fn pat_rejects_oauth_material_and_missing_user_identity() {
		let oauth: serde_json::Value = serde_json::from_slice(OAUTH_RECORD).unwrap();
		let mut pat = oauth.clone();
		pat["schema_version"] = 2.into();
		pat["refresh_token"] = serde_json::Value::Null;
		pat["access_token_expires_at_unix_micros"] = serde_json::Value::Null;
		pat["personal_access_token_user_id"] = "pat-user".into();
		for (key, value) in [
			("refresh_token", "unexpected-refresh".into()),
			("id_token", "unexpected-id-token".into()),
			("access_token_expires_at_unix_micros", 100.into()),
			("personal_access_token_user_id", serde_json::Value::Null),
			("schema_version", 1.into()),
		] {
			let mut invalid = pat.clone();
			invalid[key] = value;
			let decoded: PersistedCredential = serde_json::from_value(invalid).unwrap();
			assert!(decoded.into_bundle().is_err(), "{key}");
		}
	}

	#[tokio::test]
	async fn pat_store_reopens_and_allows_exact_oauth_reauthentication() {
		let directory = tempfile::tempdir().unwrap();
		let root =
			decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let database = decodex_database::SqliteStore::open(&root.paths()).unwrap();
		let store = SqliteCredentialStore::new(database.clone());
		let account = AccountId::new("20000000-0000-4000-8000-000000000039").unwrap();
		let operation = AccountOperationId::new("30000000-0000-4000-8000-000000000039").unwrap();
		let provider = ProviderIdentity::new(AccountProvider::Chatgpt, "provider").unwrap();
		let bundle = CredentialSecretBundle::personal_access_token(
			"synthetic-pat".into(),
			"pat-user".into(),
			Some("pro".into()),
			None,
		)
		.unwrap();
		let binding = bundle
			.binding_for(&account, &operation, CredentialVersion::new(1).unwrap(), &provider)
			.unwrap();
		assert_eq!(binding.schema_version, CredentialStoreSchemaVersion::V2);
		database
			.prepare_account_operation(&decodex_database::AccountOperationPreparation {
				operation_id: operation.clone(),
				account_id: account.clone(),
				kind: decodex_core::AccountOperationKind::Enroll,
				display_label: Some("PAT fixture".into()),
				enabled: Some(true),
				expected_account_revision: None,
				expected: None,
				target: Some(binding.clone()),
				provider: provider.clone(),
			})
			.await
			.unwrap();
		store.create(&account, &binding, bundle).unwrap();
		drop(store);
		drop(database);
		let store =
			SqliteCredentialStore::new(decodex_database::SqliteStore::open(&root.paths()).unwrap());
		let restored = store.read_exact(&account, &binding).unwrap();
		assert_eq!(restored.bundle().access_token(), "synthetic-pat");
		assert_eq!(restored.bundle().personal_access_token_user_id(), Some("pat-user"));
		let oauth = CredentialSecretBundle::chatgpt(
			"synthetic-access".into(),
			"synthetic-refresh".into(),
			None,
			None,
			None,
			"bearer".into(),
			100,
		)
		.unwrap();
		let next = oauth
			.binding_for(&account, &operation, CredentialVersion::new(2).unwrap(), &provider)
			.unwrap();
		store.compare_and_swap_rotate(&account, &binding, &next, oauth).unwrap();
		assert!(store.read_exact(&account, &binding).is_err());
		assert!(!store.read_exact(&account, &next).unwrap().bundle().is_personal_access_token());
	}
}
