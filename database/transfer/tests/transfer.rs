#![cfg(unix)]
//! End-to-end contract for one exact, replayable, source-retaining account transfer.

use std::{
	fs::{self, Permissions},
	io::Write as _,
	os::unix::fs::PermissionsExt as _,
	path::Path,
	process::{Command, Stdio},
};

use clap as _;
use libc as _;
use redb::{Database, Durability, TableDefinition};
use serde as _;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tokio::runtime::Runtime;
use zeroize as _;

use decodex_core::{AccountId, DecodexRoot};
use decodex_database::SqliteStore;

const CREDENTIALS: TableDefinition<&str, &[u8]> = TableDefinition::new("account_credentials_v1");
const FINGERPRINT_DOMAIN: &[u8] = b"decodex-host-credential-store-v1\0";

#[test]
fn exact_transfer_is_atomic_replayable_and_retains_the_source() {
	let temporary = tempfile::tempdir().expect("temporary transfer root");
	let root = temporary.path().canonicalize().expect("canonical root");
	let server = root.join("server");

	fs::create_dir(&server).expect("create server directory");
	fs::set_permissions(&root, Permissions::from_mode(0o700)).expect("scope root");
	fs::set_permissions(&server, Permissions::from_mode(0o700)).expect("scope server");

	let account_id = "10000000-0000-4000-8000-000000000001";
	let writer_operation_id = "20000000-0000-4000-8000-000000000001";
	let provider_account_id = "30000000-0000-4000-8000-000000000001";
	let payload = fixture_credential(account_id, writer_operation_id, provider_account_id);
	let fingerprint = credential_fingerprint(&payload);
	let vault = server.join("credentials.redb");

	create_retired_vault(&vault, account_id, &payload);

	let source_digest = file_sha256(&vault);
	let manifest = serde_json::to_vec(&serde_json::json!({
		"schema": "decodex/cli-account/1",
		"command": "list",
		"outcome": "success",
		"result": {
			"outcome": "available",
			"data": {
				"accounts": [{
					"account_id": account_id,
					"alias": "Fixture",
					"enabled": true,
					"account_revision": 9,
					"observed_state": "available",
					"lifecycle_readiness": "ready",
					"credential_binding": {
						"schema_version": 1,
						"version": 7,
						"fingerprint_sha256": fingerprint,
						"provider": "chatgpt",
						"provider_account_id": provider_account_id,
					},
					"five_hour_quota": {
						"duration_minutes": 300,
						"observed_at_unix_micros": null,
						"result": {"state": "unknown"},
					},
					"seven_day_quota": {
						"duration_minutes": 10_080,
						"observed_at_unix_micros": 1_000_000,
						"result": {
							"state": "current",
							"data": {
								"used_percent": 17,
								"resets_at_unix_micros": 4_102_444_800_000_000_i64,
							},
						},
					},
				}],
				"routing": {
					"revision": 11,
					"mode": {"mode": "fixed", "account_id": account_id},
					"order": [account_id],
				},
			},
		},
	}))
	.expect("account manifest");
	let first = run_transfer(&root, &manifest);

	assert_eq!(first["outcome"], "imported");
	assert_eq!(first["account_count"], 1);
	assert_eq!(first["source_vault_retained"], true);
	assert_eq!(file_sha256(&vault), source_digest);
	assert!(vault.exists());

	let second = run_transfer(&root, &manifest);

	assert_eq!(second["outcome"], "replayed");
	assert_eq!(second["account_count"], 1);
	assert_eq!(file_sha256(&vault), source_digest);

	let root = DecodexRoot::new(root).expect("typed root");
	let paths = root.paths();
	let store = SqliteStore::open(&paths).expect("open transferred database");
	let runtime = Runtime::new().expect("test runtime");
	let (accounts, routing) = runtime
		.block_on(store.read_account_registry_snapshot(16))
		.expect("read transferred registry");

	assert_eq!(accounts.len(), 1);
	assert_eq!(accounts[0].account_id, AccountId::new(account_id).expect("account id"));
	assert_eq!(accounts[0].label, "Fixture");
	assert_eq!(accounts[0].revision, 9);
	assert_eq!(routing.revision, 11);
	assert_eq!(routing.order, vec![AccountId::new(account_id).expect("account id")]);

	let stored = store.read_credential(account_id).expect("read transferred credential");

	assert_eq!(stored.key.fingerprint, fingerprint);
	assert_eq!(credential_fingerprint(stored.payload.as_slice()), fingerprint);

	store.close();

	assert_eq!(
		fs::metadata(paths.product_database_file())
			.expect("database metadata")
			.permissions()
			.mode()
			& 0o777,
		0o600,
	);
}

fn fixture_credential(
	account_id: &str,
	writer_operation_id: &str,
	provider_account_id: &str,
) -> Vec<u8> {
	serde_json::to_vec(&serde_json::json!({
		"schema_version": 1,
		"account_id": account_id,
		"credential_version": 7,
		"writer_operation_id": writer_operation_id,
		"provider": "chatgpt",
		"provider_account_id": provider_account_id,
		"access_token": "fixture-access-token",
		"refresh_token": "fixture-refresh-token",
		"id_token": "fixture-id-token",
		"plan_type": "pro",
		"provider_email": "fixture@example.test",
		"token_type": "bearer",
		"access_token_expires_at_unix_micros": 4_102_444_800_000_000_i64,
	}))
	.expect("credential payload")
}

fn create_retired_vault(vault: &Path, account_id: &str, payload: &[u8]) {
	let database = Database::create(vault).expect("create retired vault fixture");
	let mut transaction = database.begin_write().expect("begin retired vault transaction");

	transaction.set_durability(Durability::Immediate).expect("durability");

	{
		let mut table = transaction.open_table(CREDENTIALS).expect("open credential table");

		table.insert(account_id, payload).expect("insert credential");
	}

	transaction.commit().expect("commit retired vault fixture");

	drop(database);

	fs::set_permissions(vault, Permissions::from_mode(0o600)).expect("scope vault");
}

fn run_transfer(root: &Path, manifest: &[u8]) -> Value {
	let mut child = Command::new(env!("CARGO_BIN_EXE_decodex-database-transfer"))
		.arg("--root")
		.arg(root)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.expect("start transfer tool");

	child.stdin.take().expect("transfer stdin").write_all(manifest).expect("write manifest");

	let output = child.wait_with_output().expect("wait transfer tool");

	assert!(
		output.status.success(),
		"transfer failed: {}",
		String::from_utf8_lossy(&output.stderr)
	);
	assert!(output.stderr.is_empty());

	serde_json::from_slice(&output.stdout).expect("transfer result")
}

fn credential_fingerprint(payload: &[u8]) -> String {
	let mut digest = Sha256::new();

	digest.update(FINGERPRINT_DOMAIN);
	digest.update(payload);

	digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn file_sha256(path: &Path) -> String {
	let bytes = fs::read(path).expect("read fixture file");
	let mut digest = Sha256::new();

	digest.update(bytes);

	digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}
