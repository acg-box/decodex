//! Bounded offline restoration of credential-negative local account authority.

use std::fmt::{Display, Formatter};

use decodex_core::DecodexRoot;
#[cfg(any(target_os = "macos", test))]
use decodex_core::{
	AccountId, AccountOperationId, AccountProvider, AccountRoutingControl, AccountSelectionMode,
	CredentialBinding, CredentialFingerprint, CredentialStoreSchemaVersion, CredentialVersion,
	ProviderIdentity, contains_credential_material,
};
#[cfg(any(target_os = "macos", all(test, unix)))]
use decodex_core::{DecodexConfig, DecodexPaths, PostgresIdentityConfig, ServerProfile};
#[cfg(any(target_os = "macos", all(test, unix)))]
use decodex_postgres::{BootstrapFailure, LocalAccountAuthorityRestoreFailure, PostgresStore};
#[cfg(any(target_os = "macos", test))]
use decodex_postgres::{LocalAccountAuthorityAccount, LocalAccountAuthorityRestore};
#[cfg(any(target_os = "macos", all(test, unix)))]
use decodex_protocol::{LocalTransportAuthority, LocalTransportListener, LocalTransportRefusal};
#[cfg(any(target_os = "macos", test))] use serde::Deserialize;
#[cfg(any(target_os = "macos", all(test, unix)))] use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(any(target_os = "macos", test))] use std::{collections::BTreeSet, io::Read};
#[cfg(any(target_os = "macos", test))] use zeroize::Zeroizing;

#[cfg(any(target_os = "macos", test))] use crate::account_service::stable_account_alias;
#[cfg(target_os = "macos")] use crate::host_credentials::RedbCredentialStore;
#[cfg(any(target_os = "macos", all(test, unix)))]
use crate::{
	bootstrap::credential,
	host_credentials::{CredentialStoreError, HostCredentialStore},
};

#[cfg(any(target_os = "macos", test))]
const RESTORE_DOCUMENT_SCHEMA: &str = "decodex/local-account-authority-restore/1";
#[cfg(any(target_os = "macos", test))]
const MAX_RESTORE_DOCUMENT_BYTES: usize = 512 * 1024;
#[cfg(any(target_os = "macos", test))]
const MAX_RESTORE_ACCOUNTS: usize = 512;

const RESTORED_CLASSIFICATION: &str = "restored";
const CONFIGURATION_REFUSED_CLASSIFICATION: &str = "configuration_refused";
const HOST_REFUSED_CLASSIFICATION: &str = "host_refused";
#[cfg(any(target_os = "macos", all(test, unix)))]
const INPUT_REFUSED_CLASSIFICATION: &str = "input_refused";
#[cfg(any(target_os = "macos", all(test, unix)))]
const DAEMON_ACTIVE_REFUSED_CLASSIFICATION: &str = "daemon_active_refused";
#[cfg(any(target_os = "macos", all(test, unix)))]
const OFFLINE_BOUNDARY_REFUSED_CLASSIFICATION: &str = "offline_boundary_refused";
#[cfg(any(target_os = "macos", test))]
const CREDENTIAL_REFUSED_CLASSIFICATION: &str = "credential_refused";
#[cfg(any(target_os = "macos", all(test, unix)))]
const AUTHENTICATION_REFUSED_CLASSIFICATION: &str = "authentication_refused";
#[cfg(any(target_os = "macos", all(test, unix)))]
const TARGET_REFUSED_CLASSIFICATION: &str = "target_refused";
#[cfg(any(target_os = "macos", all(test, unix)))]
const READBACK_REFUSED_CLASSIFICATION: &str = "readback_refused";
#[cfg(any(target_os = "macos", all(test, unix)))]
const DATABASE_REFUSED_CLASSIFICATION: &str = "database_refused";
#[cfg(any(target_os = "macos", all(test, unix)))]
const AUTHORITY_REFUSED_CLASSIFICATION: &str = "authority_refused";

/// The hidden command's complete bounded output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalAccountAuthorityRestoreReport {
	classification: &'static str,
	account_count: usize,
}
impl LocalAccountAuthorityRestoreReport {
	const fn new(classification: &'static str, account_count: usize) -> Self {
		Self { classification, account_count }
	}

	/// Construct the root-validation refusal before transient input is consumed.
	#[doc(hidden)]
	pub const fn configuration_refused() -> Self {
		Self::new(CONFIGURATION_REFUSED_CLASSIFICATION, 0)
	}

	/// Construct the explicit non-macOS host refusal without consuming stdin.
	#[doc(hidden)]
	pub const fn host_refused() -> Self {
		Self::new(HOST_REFUSED_CLASSIFICATION, 0)
	}

	/// Return whether the exact restore committed.
	pub fn succeeded(self) -> bool {
		self.classification == RESTORED_CLASSIFICATION
	}
}
impl Display for LocalAccountAuthorityRestoreReport {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		write!(
			formatter,
			"{{\"account_count\":{},\"classification\":\"{}\"}}",
			self.account_count, self.classification,
		)
	}
}

#[cfg(any(target_os = "macos", test))]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreDocument {
	schema: String,
	accounts: Vec<RestoreAccount>,
	routing: RestoreRouting,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreAccount {
	account_id: String,
	enabled: bool,
	revision: i64,
	provider_kind: RestoreProviderKind,
	provider_account_id: String,
	credential_store_schema_version: u16,
	credential_version: u64,
	credential_fingerprint: String,
	credential_writer_operation_id: String,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RestoreProviderKind {
	Chatgpt,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreRouting {
	revision: i64,
	mode: RestoreRoutingMode,
	fixed_account_id: RequiredNullableAccountId,
	account_order: Vec<String>,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RestoreRoutingMode {
	Fixed,
	Balanced,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Deserialize)]
#[serde(transparent)]
struct RequiredNullableAccountId(Option<String>);

#[cfg(any(target_os = "macos", test))]
fn read_restore_document<R: Read>(input: R) -> Result<LocalAccountAuthorityRestore, ()> {
	let mut bytes = Zeroizing::new(Vec::with_capacity(MAX_RESTORE_DOCUMENT_BYTES.min(64 * 1024)));
	input
		.take(u64::try_from(MAX_RESTORE_DOCUMENT_BYTES + 1).map_err(|_| ())?)
		.read_to_end(&mut bytes)
		.map_err(|_| ())?;
	if bytes.is_empty() || bytes.len() > MAX_RESTORE_DOCUMENT_BYTES {
		return Err(());
	}
	let document: RestoreDocument = serde_json::from_slice(&bytes).map_err(|_| ())?;
	if document.schema != RESTORE_DOCUMENT_SCHEMA
		|| document.accounts.len() > MAX_RESTORE_ACCOUNTS
		|| document.routing.revision < 1
		|| document.routing.account_order.len() != document.accounts.len()
	{
		return Err(());
	}

	let mut accounts = Vec::with_capacity(document.accounts.len());
	let mut account_ids = BTreeSet::new();
	let mut provider_bindings = BTreeSet::new();
	let mut writer_operation_ids = BTreeSet::new();
	for raw in document.accounts {
		if raw.revision < 1
			|| raw.provider_account_id.trim() != raw.provider_account_id
			|| contains_credential_material(&raw.provider_account_id)
			|| raw.credential_version > i64::MAX as u64
		{
			return Err(());
		}
		let account_id = AccountId::new(raw.account_id).map_err(|_| ())?;
		let provider = ProviderIdentity::new(
			match raw.provider_kind {
				RestoreProviderKind::Chatgpt => AccountProvider::Chatgpt,
			},
			raw.provider_account_id,
		)
		.map_err(|_| ())?;
		let writer_operation_id =
			AccountOperationId::new(raw.credential_writer_operation_id).map_err(|_| ())?;
		let credential = CredentialBinding {
			schema_version: CredentialStoreSchemaVersion::new(raw.credential_store_schema_version)
				.map_err(|_| ())?,
			version: CredentialVersion::new(raw.credential_version).map_err(|_| ())?,
			fingerprint: CredentialFingerprint::new(raw.credential_fingerprint).map_err(|_| ())?,
			provider: provider.clone(),
			writer_operation_id: writer_operation_id.clone(),
		};
		if !account_ids.insert(account_id.clone())
			|| !provider_bindings.insert((
				match provider.provider() {
					AccountProvider::Chatgpt => "chatgpt",
				},
				provider.account_id().to_owned(),
			)) || !writer_operation_ids.insert(writer_operation_id)
		{
			return Err(());
		}
		accounts.push(LocalAccountAuthorityAccount {
			account_id,
			display_label: stable_account_alias(&provider),
			enabled: raw.enabled,
			revision: raw.revision,
			credential,
		});
	}

	let order = document
		.routing
		.account_order
		.into_iter()
		.map(AccountId::new)
		.collect::<Result<Vec<_>, _>>()
		.map_err(|_| ())?;
	if order.iter().cloned().collect::<BTreeSet<_>>().len() != order.len()
		|| accounts.iter().map(|account| &account.account_id).ne(order.iter())
	{
		return Err(());
	}
	let fixed_account_id =
		document.routing.fixed_account_id.0.map(AccountId::new).transpose().map_err(|_| ())?;
	let mode = match (document.routing.mode, fixed_account_id) {
		(RestoreRoutingMode::Balanced, None) => AccountSelectionMode::Balanced,
		(RestoreRoutingMode::Fixed, Some(account_id)) if account_ids.contains(&account_id) =>
			AccountSelectionMode::Fixed(account_id),
		_ => return Err(()),
	};

	Ok(LocalAccountAuthorityRestore {
		accounts,
		routing: AccountRoutingControl { revision: document.routing.revision, mode, order },
	})
}

#[cfg(any(target_os = "macos", all(test, unix)))]
#[derive(Clone, Copy)]
#[repr(u8)]
enum PrecommitRefusal {
	OfflineBoundary = 1,
	Credential = 2,
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn prove_exact_host_bindings(
	credentials: &dyn HostCredentialStore,
	restore: &LocalAccountAuthorityRestore,
) -> bool {
	for account in &restore.accounts {
		let stored = match credentials.read_exact(&account.account_id, &account.credential) {
			Ok(stored) => stored,
			Err(_) => return false,
		};
		if stored.binding() != &account.credential {
			return false;
		}
		drop(stored);
	}
	true
}

#[cfg(any(target_os = "macos", all(test, unix)))]
struct PreparedLocalAccountAuthorityRestore {
	restore: LocalAccountAuthorityRestore,
	account_count: usize,
	paths: DecodexPaths,
	config: DecodexConfig,
	schema_owner: PostgresIdentityConfig,
	schema_owner_credential: Option<Zeroizing<String>>,
	transport: LocalTransportAuthority,
}

#[cfg(any(target_os = "macos", all(test, unix)))]
struct BoundLocalAccountAuthorityRestore<C> {
	prepared: PreparedLocalAccountAuthorityRestore,
	listener: LocalTransportListener,
	credentials: C,
}

/// Run the macOS-only offline local account authority restore.
#[cfg(target_os = "macos")]
pub(crate) async fn restore_local_account_authority<R: Read>(
	root: DecodexRoot,
	schema_owner_user: String,
	schema_owner_credential_env_var: Option<String>,
	input: R,
) -> LocalAccountAuthorityRestoreReport {
	restore_local_account_authority_with_store(
		root,
		schema_owner_user,
		schema_owner_credential_env_var,
		input,
		RedbCredentialStore::new,
	)
	.await
}

#[cfg(any(target_os = "macos", all(test, unix)))]
async fn restore_local_account_authority_with_store<R, C, F>(
	root: DecodexRoot,
	schema_owner_user: String,
	schema_owner_credential_env_var: Option<String>,
	input: R,
	open_credentials: F,
) -> LocalAccountAuthorityRestoreReport
where
	R: Read,
	C: HostCredentialStore,
	F: FnOnce(&DecodexPaths) -> Result<C, CredentialStoreError>,
{
	let prepared = match prepare_local_account_authority_restore(
		root,
		schema_owner_user,
		schema_owner_credential_env_var,
		input,
	) {
		Ok(prepared) => prepared,
		Err(report) => return report,
	};
	let bound = match bind_local_account_authority_restore(prepared, open_credentials).await {
		Ok(bound) => bound,
		Err(report) => return report,
	};
	commit_local_account_authority_restore(bound).await
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn prepare_local_account_authority_restore<R: Read>(
	root: DecodexRoot,
	schema_owner_user: String,
	schema_owner_credential_env_var: Option<String>,
	input: R,
) -> Result<PreparedLocalAccountAuthorityRestore, LocalAccountAuthorityRestoreReport> {
	let restore =
		read_restore_document(input).map_err(|()| report(INPUT_REFUSED_CLASSIFICATION, 0))?;
	let account_count = restore.accounts.len();
	let paths = root.paths();
	let config = DecodexConfig::load(&paths)
		.map_err(|_| report(CONFIGURATION_REFUSED_CLASSIFICATION, account_count))?;
	let local_profile = match config.active_profile() {
		ServerProfile::Local(profile) => profile,
		ServerProfile::Remote(_) => {
			return Err(report(CONFIGURATION_REFUSED_CLASSIFICATION, account_count));
		},
	};
	let schema_owner =
		PostgresIdentityConfig::new(schema_owner_user, schema_owner_credential_env_var)
			.map_err(|_| report(CONFIGURATION_REFUSED_CLASSIFICATION, account_count))?;
	let runtime = config.postgres().runtime();
	if schema_owner.user() == runtime.user()
		|| schema_owner.credential_env_var().is_some()
			&& schema_owner.credential_env_var() == runtime.credential_env_var()
	{
		return Err(report(CONFIGURATION_REFUSED_CLASSIFICATION, account_count));
	}
	let schema_owner_credential = credential(&schema_owner)
		.map_err(|()| report(AUTHENTICATION_REFUSED_CLASSIFICATION, account_count))?
		.map(Zeroizing::new);
	let transport = LocalTransportAuthority::new(
		paths.clone(),
		local_profile.policy(),
		local_profile.service_owner_uid(),
	)
	.map_err(|_| report(OFFLINE_BOUNDARY_REFUSED_CLASSIFICATION, account_count))?;

	Ok(PreparedLocalAccountAuthorityRestore {
		restore,
		account_count,
		paths,
		config,
		schema_owner,
		schema_owner_credential,
		transport,
	})
}

#[cfg(any(target_os = "macos", all(test, unix)))]
async fn bind_local_account_authority_restore<C, F>(
	prepared: PreparedLocalAccountAuthorityRestore,
	open_credentials: F,
) -> Result<BoundLocalAccountAuthorityRestore<C>, LocalAccountAuthorityRestoreReport>
where
	C: HostCredentialStore,
	F: FnOnce(&DecodexPaths) -> Result<C, CredentialStoreError>,
{
	let account_count = prepared.account_count;
	let listener = match prepared.transport.bind().await {
		Ok(listener) => listener,
		Err(LocalTransportRefusal::EndpointInUse) => {
			return Err(report(DAEMON_ACTIVE_REFUSED_CLASSIFICATION, account_count));
		},
		Err(_) => return Err(report(OFFLINE_BOUNDARY_REFUSED_CLASSIFICATION, account_count)),
	};
	let credentials = match open_credentials(&prepared.paths) {
		Ok(credentials) => credentials,
		Err(_) => {
			drop(listener);
			return Err(report(CREDENTIAL_REFUSED_CLASSIFICATION, account_count));
		},
	};
	if listener.revalidate().is_err() {
		drop(listener);
		return Err(report(OFFLINE_BOUNDARY_REFUSED_CLASSIFICATION, account_count));
	}
	if !prove_exact_host_bindings(&credentials, &prepared.restore) {
		drop(listener);
		return Err(report(CREDENTIAL_REFUSED_CLASSIFICATION, account_count));
	}

	Ok(BoundLocalAccountAuthorityRestore { prepared, listener, credentials })
}

#[cfg(any(target_os = "macos", all(test, unix)))]
async fn commit_local_account_authority_restore<C>(
	bound: BoundLocalAccountAuthorityRestore<C>,
) -> LocalAccountAuthorityRestoreReport
where
	C: HostCredentialStore,
{
	let BoundLocalAccountAuthorityRestore { prepared, listener, credentials } = bound;
	let precommit_refusal = AtomicU8::new(0);
	let listener_fence = &listener;
	let credential_fence = &credentials;
	let restore_fence = &prepared.restore;
	let refusal_fence = &precommit_refusal;
	let result = PostgresStore::restore_local_account_authority_explicit(
		prepared.config.postgres(),
		&prepared.schema_owner,
		prepared.schema_owner_credential.as_ref().map(|credential| credential.as_str()),
		&prepared.restore,
		move || {
			if listener_fence.revalidate().is_err() {
				refusal_fence.store(PrecommitRefusal::OfflineBoundary as u8, Ordering::SeqCst);
				return false;
			}
			if !prove_exact_host_bindings(credential_fence, restore_fence) {
				refusal_fence.store(PrecommitRefusal::Credential as u8, Ordering::SeqCst);
				return false;
			}
			true
		},
	)
	.await;
	drop(listener);
	classify_local_account_authority_restore(
		result,
		precommit_refusal.load(Ordering::SeqCst),
		prepared.account_count,
	)
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn classify_local_account_authority_restore(
	result: Result<(), LocalAccountAuthorityRestoreFailure>,
	precommit_refusal: u8,
	account_count: usize,
) -> LocalAccountAuthorityRestoreReport {
	match result {
		Ok(()) => report(RESTORED_CLASSIFICATION, account_count),
		Err(LocalAccountAuthorityRestoreFailure::InvalidInput) =>
			report(INPUT_REFUSED_CLASSIFICATION, account_count),
		Err(LocalAccountAuthorityRestoreFailure::TargetNotFresh) =>
			report(TARGET_REFUSED_CLASSIFICATION, account_count),
		Err(LocalAccountAuthorityRestoreFailure::PrecommitFence) => report(
			if precommit_refusal == PrecommitRefusal::OfflineBoundary as u8 {
				OFFLINE_BOUNDARY_REFUSED_CLASSIFICATION
			} else {
				CREDENTIAL_REFUSED_CLASSIFICATION
			},
			account_count,
		),
		Err(LocalAccountAuthorityRestoreFailure::ReadbackMismatch) =>
			report(READBACK_REFUSED_CLASSIFICATION, account_count),
		Err(LocalAccountAuthorityRestoreFailure::Database(failure)) =>
			report(database_classification(failure), account_count),
	}
}

/// Explicitly reject this host-specific command without reading stdin on other platforms.
#[cfg(not(target_os = "macos"))]
pub(crate) async fn restore_local_account_authority<R: std::io::Read>(
	_root: DecodexRoot,
	_schema_owner_user: String,
	_schema_owner_credential_env_var: Option<String>,
	_input: R,
) -> LocalAccountAuthorityRestoreReport {
	LocalAccountAuthorityRestoreReport::host_refused()
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn database_classification(failure: BootstrapFailure) -> &'static str {
	match failure {
		BootstrapFailure::Authentication => AUTHENTICATION_REFUSED_CLASSIFICATION,
		BootstrapFailure::UnsafeAuthority | BootstrapFailure::UnsafeHostPath =>
			AUTHORITY_REFUSED_CLASSIFICATION,
		BootstrapFailure::Unreachable | BootstrapFailure::Incompatible =>
			DATABASE_REFUSED_CLASSIFICATION,
	}
}

#[cfg(any(target_os = "macos", test))]
const fn report(
	classification: &'static str,
	account_count: usize,
) -> LocalAccountAuthorityRestoreReport {
	LocalAccountAuthorityRestoreReport::new(classification, account_count)
}

#[cfg(test)]
mod tests {
	use serde_json::{Value, json};

	use super::*;

	fn account(
		account_id: &str,
		provider_account_id: &str,
		writer_operation_id: &str,
		fingerprint: char,
	) -> Value {
		json!({
			"account_id": account_id,
			"enabled": true,
			"revision": 3,
			"provider_kind": "chatgpt",
			"provider_account_id": provider_account_id,
			"credential_store_schema_version": 1,
			"credential_version": 4,
			"credential_fingerprint": fingerprint.to_string().repeat(64),
			"credential_writer_operation_id": writer_operation_id,
		})
	}

	fn document() -> Value {
		json!({
			"schema": RESTORE_DOCUMENT_SCHEMA,
			"accounts": [
				account(
					"10000000-0000-4000-8000-000000000001",
					"first@example.invalid",
					"20000000-0000-4000-8000-000000000001",
					'a',
				),
				account(
					"10000000-0000-4000-8000-000000000002",
					"second@example.invalid",
					"20000000-0000-4000-8000-000000000002",
					'b',
				),
			],
			"routing": {
				"revision": 8,
				"mode": "fixed",
				"fixed_account_id": "10000000-0000-4000-8000-000000000002",
				"account_order": [
					"10000000-0000-4000-8000-000000000001",
					"10000000-0000-4000-8000-000000000002",
				],
			},
		})
	}

	fn parse(value: &Value) -> Result<LocalAccountAuthorityRestore, ()> {
		read_restore_document(serde_json::to_vec(value).expect("serialize fixture").as_slice())
	}

	#[test]
	fn strict_document_derives_aliases_and_complete_fixed_routing() {
		let restore = parse(&document()).expect("strict restore document");

		assert_eq!(restore.accounts.len(), 2);
		assert!(restore.accounts.iter().all(|account| {
			!account.display_label.is_empty()
				&& !account.display_label.chars().any(char::is_whitespace)
		}));
		assert!(matches!(
			restore.routing.mode,
			AccountSelectionMode::Fixed(ref account_id)
				if account_id.as_str() == "10000000-0000-4000-8000-000000000002"
		));
		assert_eq!(
			restore.accounts.iter().map(|account| account.account_id.as_str()).collect::<Vec<_>>(),
			restore.routing.order.iter().map(AccountId::as_str).collect::<Vec<_>>(),
		);
	}

	#[test]
	fn strict_document_rejects_unknown_credential_and_incomplete_routing_shapes() {
		let mut unknown = document();
		unknown["accounts"][0]["display_label"] = json!("Mutable");
		assert!(parse(&unknown).is_err());

		let mut credential = document();
		credential["accounts"][0]["provider_account_id"] = json!("Bearer abcdefghijklmnop");
		assert!(parse(&credential).is_err());

		let mut incomplete = document();
		incomplete["routing"]["account_order"] =
			json!(
				["10000000-0000-4000-8000-000000000002", "10000000-0000-4000-8000-000000000001",]
			);
		assert!(parse(&incomplete).is_err());

		let mut duplicate = document();
		duplicate["routing"]["account_order"][1] = json!("10000000-0000-4000-8000-000000000001");
		assert!(parse(&duplicate).is_err());

		let mut missing_nullable = document();
		missing_nullable["routing"]
			.as_object_mut()
			.expect("routing fixture is an object")
			.remove("fixed_account_id");
		assert!(parse(&missing_nullable).is_err());

		assert!(
			read_restore_document(vec![b' '; MAX_RESTORE_DOCUMENT_BYTES + 1].as_slice()).is_err()
		);
	}

	#[test]
	fn report_is_exactly_bounded_and_credential_negative() {
		let output = report(CREDENTIAL_REFUSED_CLASSIFICATION, 2).to_string();

		assert_eq!(output, "{\"account_count\":2,\"classification\":\"credential_refused\"}",);
		assert!(!contains_credential_material(&output));
	}

	#[cfg(unix)]
	struct ReadExactOnlyStore {
		account_id: AccountId,
		binding: CredentialBinding,
		bundle: crate::CredentialSecretBundle,
		reads: std::sync::Arc<std::sync::atomic::AtomicUsize>,
		refuse_on_read: Option<usize>,
	}
	#[cfg(unix)]
	impl HostCredentialStore for ReadExactOnlyStore {
		fn create(
			&self,
			_account_id: &AccountId,
			_target: &CredentialBinding,
			_bundle: crate::CredentialSecretBundle,
		) -> Result<(), crate::CredentialStoreError> {
			panic!("local restore must not create credentials")
		}

		fn read_exact(
			&self,
			account_id: &AccountId,
			expected: &CredentialBinding,
		) -> Result<crate::StoredCredential, crate::CredentialStoreError> {
			assert_eq!(account_id, &self.account_id);
			assert_eq!(expected, &self.binding);
			let read =
				self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst).saturating_add(1);
			if self.refuse_on_read == Some(read) {
				return Err(crate::CredentialStoreError::FingerprintMismatch);
			}

			crate::host_credentials::seal_exact_read(
				account_id,
				&self.binding,
				expected,
				self.bundle.clone(),
			)
		}

		fn compare_and_swap_rotate(
			&self,
			_account_id: &AccountId,
			_expected: &CredentialBinding,
			_target: &CredentialBinding,
			_bundle: crate::CredentialSecretBundle,
		) -> Result<(), crate::CredentialStoreError> {
			panic!("local restore must not rotate credentials")
		}

		fn delete(
			&self,
			_account_id: &AccountId,
			_expected: &CredentialBinding,
		) -> Result<(), crate::CredentialStoreError> {
			panic!("local restore must not delete credentials")
		}
	}

	#[cfg(unix)]
	fn command_document(account_id: &AccountId, binding: &CredentialBinding) -> Value {
		json!({
			"schema": RESTORE_DOCUMENT_SCHEMA,
			"accounts": [{
				"account_id": account_id.as_str(),
				"enabled": false,
				"revision": 11,
				"provider_kind": "chatgpt",
				"provider_account_id": binding.provider.account_id(),
				"credential_store_schema_version": binding.schema_version.get(),
				"credential_version": binding.version.get(),
				"credential_fingerprint": binding.fingerprint.as_str(),
				"credential_writer_operation_id": binding.writer_operation_id.as_str(),
			}],
			"routing": {
				"revision": 9,
				"mode": "fixed",
				"fixed_account_id": account_id.as_str(),
				"account_order": [account_id.as_str()],
			},
		})
	}

	#[cfg(unix)]
	#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
	#[ignore = "requires the canonical gate's fresh PostgreSQL 18 target"]
	async fn local_account_restore_command_proves_two_exact_credential_fences_and_readback()
	-> Result<(), Box<dyn std::error::Error>> {
		let root = DecodexRoot::new(std::env::var("DECODEX_TEST_ROOT")?)?;
		let schema_owner_user = std::env::var("DECODEX_TEST_SCHEMA_OWNER_USER")?;
		let config = DecodexConfig::load(&root.paths())?;
		let account_id = AccountId::new("74000000-0000-4000-8000-000000001276")?;
		let provider =
			ProviderIdentity::new(AccountProvider::Chatgpt, "command-restore@example.invalid")?;
		let writer_operation_id = AccountOperationId::new("75000000-0000-4000-8000-000000001276")?;
		let bundle = crate::CredentialSecretBundle::chatgpt(
			"fixture-access-material".to_owned(),
			"fixture-refresh-material".to_owned(),
			Some("fixture-identity-material".to_owned()),
			Some("fixture-plan".to_owned()),
			provider.account_id().to_owned(),
			"bearer".to_owned(),
			4_102_444_800_000_000,
		)?;
		let binding = bundle.binding_for(
			&account_id,
			&writer_operation_id,
			CredentialVersion::new(7)?,
			&provider,
		)?;
		let input = serde_json::to_vec(&command_document(&account_id, &binding))?;
		let expected =
			read_restore_document(input.as_slice()).expect("command fixture is strict input");

		let mismatch_reads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
		let mismatch_store = ReadExactOnlyStore {
			account_id: account_id.clone(),
			binding: binding.clone(),
			bundle: bundle.clone(),
			reads: std::sync::Arc::clone(&mismatch_reads),
			refuse_on_read: Some(2),
		};
		let mismatch = restore_local_account_authority_with_store(
			root.clone(),
			schema_owner_user.clone(),
			None,
			input.as_slice(),
			move |_| Ok(mismatch_store),
		)
		.await;
		assert_eq!(mismatch, report(CREDENTIAL_REFUSED_CLASSIFICATION, 1));
		assert_eq!(
			mismatch.to_string(),
			"{\"account_count\":1,\"classification\":\"credential_refused\"}",
		);
		assert_eq!(
			mismatch_reads.load(std::sync::atomic::Ordering::SeqCst),
			2,
			"the mismatch must occur at the precommit read",
		);

		let rollback_store =
			PostgresStore::connect_runtime_explicit(config.postgres(), None).await?;
		let (rollback_accounts, rollback_routing) =
			rollback_store.read_account_registry_snapshot(512).await?;
		assert!(rollback_accounts.is_empty());
		assert_eq!(rollback_routing.revision, 1);
		assert!(matches!(rollback_routing.mode, AccountSelectionMode::Balanced));
		assert!(rollback_routing.order.is_empty());
		rollback_store.close();

		let successful_reads = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
		let successful_store = ReadExactOnlyStore {
			account_id: account_id.clone(),
			binding: binding.clone(),
			bundle,
			reads: std::sync::Arc::clone(&successful_reads),
			refuse_on_read: None,
		};
		let successful = restore_local_account_authority_with_store(
			root,
			schema_owner_user,
			None,
			input.as_slice(),
			move |_| Ok(successful_store),
		)
		.await;
		assert_eq!(successful, report(RESTORED_CLASSIFICATION, 1));
		let successful_output = successful.to_string();
		assert_eq!(successful_output, "{\"account_count\":1,\"classification\":\"restored\"}",);
		assert!(!contains_credential_material(&successful_output));
		assert_eq!(successful_reads.load(std::sync::atomic::Ordering::SeqCst), 2);

		let restored_store =
			PostgresStore::connect_runtime_explicit(config.postgres(), None).await?;
		let (accounts, routing) = restored_store.read_account_registry_snapshot(512).await?;
		assert_eq!(accounts.len(), 1);
		assert_eq!(accounts[0].account_id, expected.accounts[0].account_id);
		assert_eq!(accounts[0].label, expected.accounts[0].display_label);
		assert_eq!(accounts[0].enabled, expected.accounts[0].enabled);
		assert_eq!(accounts[0].revision, expected.accounts[0].revision);
		assert_eq!(accounts[0].credential.as_ref(), Some(&expected.accounts[0].credential));
		assert!(!accounts[0].tombstoned);
		assert_eq!(routing, expected.routing);
		restored_store.close();

		Ok(())
	}
}
