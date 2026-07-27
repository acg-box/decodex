//! Bounded private credential-source reader for enrollment and explicit import.

use std::{
	fs::{File, OpenOptions},
	io::{Read, Take},
	os::unix::fs::{MetadataExt as _, OpenOptionsExt as _},
	path::{Component, Path, PathBuf},
};

use base64::{
	Engine as _,
	engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
};
use decodex_core::{AccountProvider, ProviderIdentity};
use serde::Deserialize;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::host_credentials::CredentialSecretBundle;

const MAX_CREDENTIAL_FILE_BYTES: u64 = 256 * 1024;
const MAX_TOKEN_BYTES: usize = 64 * 1024;
const MAX_PROVIDER_ACCOUNT_ID_BYTES: usize = 512;
const MAX_PLAN_TYPE_BYTES: usize = 64;
const MAX_EMAIL_BYTES: usize = 320;

/// Parsed complete credential input retained only until the Account Service writes Keychain.
pub(crate) struct ImportedCredential {
	pub provider: ProviderIdentity,
	pub bundle: CredentialSecretBundle,
}

/// Read the normal Codex-owned shared auth file without modifying it.
pub(crate) fn read_shared_codex_credential() -> Result<ImportedCredential, CredentialImportError> {
	let home = std::env::var_os("HOME")
		.filter(|value| !value.is_empty())
		.ok_or(CredentialImportError::UnsafeSource)?;
	read_credential_file(&PathBuf::from(home).join(".codex/auth.json"), SourceKind::SharedCodex)
}

/// Read one explicit owner-private source selected by a credential-negative descriptor.
pub(crate) fn read_explicit_credential_file(
	descriptor: &str,
) -> Result<ImportedCredential, CredentialImportError> {
	if descriptor.is_empty() || descriptor.len() > 4096 || descriptor.chars().any(char::is_control)
	{
		return Err(CredentialImportError::InvalidSource);
	}
	let path = Path::new(descriptor);
	if !path.is_absolute() {
		return Err(CredentialImportError::InvalidSource);
	}
	read_credential_file(path, SourceKind::VersionedImport)
}

#[derive(Clone, Copy)]
enum SourceKind {
	SharedCodex,
	VersionedImport,
}

fn read_credential_file(
	path: &Path,
	source_kind: SourceKind,
) -> Result<ImportedCredential, CredentialImportError> {
	validate_components(path)?;
	let mut options = OpenOptions::new();
	options.read(true).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
	let file = options.open(path).map_err(|_| CredentialImportError::Unavailable)?;
	validate_open_file(&file)?;
	let bytes = read_bounded(file)?;
	match source_kind {
		SourceKind::SharedCodex => parse_shared_codex(&bytes),
		SourceKind::VersionedImport => parse_versioned_import(&bytes),
	}
}

fn validate_components(path: &Path) -> Result<(), CredentialImportError> {
	let mut current = PathBuf::new();
	for component in path.components() {
		match component {
			Component::RootDir => current.push(component.as_os_str()),
			Component::Normal(value) => current.push(value),
			_ => return Err(CredentialImportError::UnsafeSource),
		}
		let metadata =
			std::fs::symlink_metadata(&current).map_err(|_| CredentialImportError::Unavailable)?;
		if metadata.file_type().is_symlink() {
			return Err(CredentialImportError::UnsafeSource);
		}
	}
	Ok(())
}

fn validate_open_file(file: &File) -> Result<(), CredentialImportError> {
	let metadata = file.metadata().map_err(|_| CredentialImportError::Unavailable)?;
	// SAFETY: `geteuid` has no arguments and cannot fail.
	let effective_uid = unsafe { libc::geteuid() };
	if !metadata.is_file()
		|| metadata.uid() != effective_uid
		|| metadata.mode() & 0o077 != 0
		|| metadata.len() == 0
		|| metadata.len() > MAX_CREDENTIAL_FILE_BYTES
	{
		return Err(CredentialImportError::UnsafeSource);
	}
	Ok(())
}

fn read_bounded(file: File) -> Result<Zeroizing<Vec<u8>>, CredentialImportError> {
	let mut bytes = Zeroizing::new(Vec::new());
	let mut reader: Take<File> = file.take(MAX_CREDENTIAL_FILE_BYTES + 1);
	reader.read_to_end(&mut bytes).map_err(|_| CredentialImportError::Unavailable)?;
	if bytes.is_empty()
		|| u64::try_from(bytes.len()).ok().is_none_or(|len| len > MAX_CREDENTIAL_FILE_BYTES)
	{
		return Err(CredentialImportError::InvalidCredential);
	}
	Ok(bytes)
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct SharedCodexAuth {
	auth_mode: Option<String>,
	#[serde(rename = "OPENAI_API_KEY")]
	api_key: Option<String>,
	tokens: SharedCodexTokens,
	last_refresh: Option<String>,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct SharedCodexTokens {
	id_token: String,
	access_token: String,
	refresh_token: String,
	account_id: String,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct VersionedImport {
	schema: String,
	provider: String,
	provider_account_id: String,
	provider_email: String,
	access_token: String,
	refresh_token: String,
	id_token: Option<String>,
	plan_type: Option<String>,
	token_type: String,
	access_token_expires_at_unix_micros: i64,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct IdentityClaims {
	email: Option<String>,
	#[serde(rename = "https://api.openai.com/auth")]
	authority: Option<IdentityAuthority>,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct IdentityAuthority {
	chatgpt_account_id: Option<String>,
	chatgpt_plan_type: Option<String>,
}

#[derive(Deserialize)]
struct ExpiryClaims {
	exp: i64,
}

fn parse_shared_codex(bytes: &[u8]) -> Result<ImportedCredential, CredentialImportError> {
	let mut auth: SharedCodexAuth =
		serde_json::from_slice(bytes).map_err(|_| CredentialImportError::InvalidCredential)?;
	if auth.auth_mode.as_deref().is_some_and(|mode| mode != "chatgpt")
		|| auth.api_key.as_ref().is_some_and(|value| !value.is_empty())
	{
		return Err(CredentialImportError::InvalidCredential);
	}
	let mut claims: IdentityClaims = decode_claims(&auth.tokens.id_token)?;
	let mut authority = claims.authority.take().ok_or(CredentialImportError::InvalidCredential)?;
	let claimed_account_id =
		authority.chatgpt_account_id.take().ok_or(CredentialImportError::InvalidCredential)?;
	if claimed_account_id != auth.tokens.account_id {
		return Err(CredentialImportError::ProviderMismatch);
	}
	let email = claims.email.take().ok_or(CredentialImportError::InvalidCredential)?;
	let plan_type = authority.chatgpt_plan_type.take();
	validate_scalar(&auth.tokens.account_id, MAX_PROVIDER_ACCOUNT_ID_BYTES)?;
	validate_scalar(&email, MAX_EMAIL_BYTES)?;
	if !email.contains('@') {
		return Err(CredentialImportError::InvalidCredential);
	}
	if let Some(plan_type) = plan_type.as_ref() {
		validate_scalar(plan_type, MAX_PLAN_TYPE_BYTES)?;
	}
	validate_token(&auth.tokens.access_token)?;
	validate_token(&auth.tokens.refresh_token)?;
	validate_token(&auth.tokens.id_token)?;
	let expiry = decode_expiry_micros(&auth.tokens.access_token)?;
	let provider = ProviderIdentity::new(AccountProvider::Chatgpt, auth.tokens.account_id.clone())
		.map_err(|_| CredentialImportError::InvalidCredential)?;
	let bundle = CredentialSecretBundle::chatgpt(
		std::mem::take(&mut auth.tokens.access_token),
		std::mem::take(&mut auth.tokens.refresh_token),
		Some(std::mem::take(&mut auth.tokens.id_token)),
		plan_type,
		email,
		"bearer".to_owned(),
		expiry,
	)
	.map_err(|_| CredentialImportError::Store)?;
	Ok(ImportedCredential { provider, bundle })
}

fn parse_versioned_import(bytes: &[u8]) -> Result<ImportedCredential, CredentialImportError> {
	let mut import: VersionedImport =
		serde_json::from_slice(bytes).map_err(|_| CredentialImportError::InvalidCredential)?;
	if import.schema != "decodex/account-credential-import/1" || import.provider != "chatgpt" {
		return Err(CredentialImportError::InvalidCredential);
	}
	validate_scalar(&import.provider_account_id, MAX_PROVIDER_ACCOUNT_ID_BYTES)?;
	validate_scalar(&import.provider_email, MAX_EMAIL_BYTES)?;
	validate_token(&import.access_token)?;
	validate_token(&import.refresh_token)?;
	if let Some(id_token) = import.id_token.as_ref() {
		validate_token(id_token)?;
	}
	if let Some(plan_type) = import.plan_type.as_ref() {
		validate_scalar(plan_type, MAX_PLAN_TYPE_BYTES)?;
	}
	let provider =
		ProviderIdentity::new(AccountProvider::Chatgpt, import.provider_account_id.clone())
			.map_err(|_| CredentialImportError::InvalidCredential)?;
	let bundle = CredentialSecretBundle::chatgpt(
		std::mem::take(&mut import.access_token),
		std::mem::take(&mut import.refresh_token),
		import.id_token.take(),
		import.plan_type.take(),
		std::mem::take(&mut import.provider_email),
		std::mem::take(&mut import.token_type),
		import.access_token_expires_at_unix_micros,
	)
	.map_err(|_| CredentialImportError::Store)?;
	Ok(ImportedCredential { provider, bundle })
}

fn decode_claims<T: for<'de> Deserialize<'de>>(token: &str) -> Result<T, CredentialImportError> {
	let mut components = token.split('.');
	let header = components.next();
	let payload = components.next();
	let signature = components.next();
	if header.is_none_or(str::is_empty)
		|| payload.is_none_or(str::is_empty)
		|| signature.is_none_or(str::is_empty)
		|| components.next().is_some()
	{
		return Err(CredentialImportError::InvalidCredential);
	}
	let payload = payload.expect("JWT payload was checked");
	let decoded = if payload.ends_with('=') {
		URL_SAFE.decode(payload)
	} else {
		URL_SAFE_NO_PAD.decode(payload)
	}
	.map_err(|_| CredentialImportError::InvalidCredential)?;
	if decoded.len() > 64 * 1024 {
		return Err(CredentialImportError::InvalidCredential);
	}
	let decoded = Zeroizing::new(decoded);
	serde_json::from_slice(&decoded).map_err(|_| CredentialImportError::InvalidCredential)
}

fn decode_expiry_micros(token: &str) -> Result<i64, CredentialImportError> {
	let claims: ExpiryClaims = decode_claims(token)?;
	claims
		.exp
		.checked_mul(1_000_000)
		.filter(|value| *value > 0)
		.ok_or(CredentialImportError::InvalidCredential)
}

fn validate_token(value: &str) -> Result<(), CredentialImportError> {
	validate_scalar(value, MAX_TOKEN_BYTES)
}

fn validate_scalar(value: &str, maximum: usize) -> Result<(), CredentialImportError> {
	if value.is_empty()
		|| value.len() > maximum
		|| value.trim() != value
		|| value.chars().any(char::is_control)
	{
		Err(CredentialImportError::InvalidCredential)
	} else {
		Ok(())
	}
}

/// Closed private-source failure. No variant contains a path or credential value.
#[derive(Debug)]
pub(crate) enum CredentialImportError {
	InvalidSource,
	UnsafeSource,
	Unavailable,
	InvalidCredential,
	ProviderMismatch,
	Store,
}
