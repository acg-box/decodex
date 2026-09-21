//! Chief process admission and safe account rotation without replacing conversations.

use decodex_core::{
	AccountId, ProcessGenerationAccountBinding, ProcessGenerationId, ProcessGenerationIntent,
};
use rusqlite::{OptionalExtension as _, TransactionBehavior, params};
use sha2::{Digest as _, Sha256};

use crate::{
	DatabaseError, PrepareProcessGenerationOutcome, ProcessGenerationMutation,
	ProcessGenerationRejection, SqliteStore, StoreError,
	account_lifecycle::sql_error,
	process_generations::{empty_mutation, prepare_bound_generation, read_generation},
	unix_micros,
};

/// Latest durable process admission for one Chief root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChiefProcessBinding {
	pub root_id: String,
	pub account_id: AccountId,
	pub operation_key: String,
	pub generation_id: ProcessGenerationId,
}

impl SqliteStore {
	/// Check the current exact native thread binding and process subtree ownership.
	pub async fn chief_thread_is_owned(
		&self,
		work: String,
		thread: String,
		generation: Option<String>,
	) -> Result<bool, StoreError> {
		self.run(move |connection| {
			let transaction = connection.transaction().map_err(sql_error)?;
			let bound: bool = transaction
				.query_row(
					"SELECT EXISTS(SELECT 1 FROM chief_work_items WHERE id=?1 AND codex_thread_id=?2)",
					params![work, thread],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			let owned = bound && owns_work(&transaction, &work, generation.as_deref())?;
			transaction.commit().map_err(sql_error)?;
			Ok(owned)
		})
		.await
	}

	/// Retain one caller-validated non-secret root configuration before process admission.
	pub async fn bind_chief_root_settings(
		&self,
		root_id: &str,
		config_json: &str,
	) -> Result<(), StoreError> {
		if config_json.len() > 16384
			|| !serde_json::from_str::<serde_json::Value>(config_json)
				.is_ok_and(|value| value.is_object())
		{
			return Err(StoreError::InvalidInput(
				"Chief root settings must be a bounded JSON object",
			));
		}
		let (root_id, config_json) = (root_id.to_owned(), config_json.to_owned());
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql_error)?;
			let current: Option<String> = transaction.query_row("SELECT config_json FROM chief_root_settings WHERE root_id = ?1", [&root_id], |row| row.get(0)).optional().map_err(sql_error)?;
			if let Some(current) = current { return if current == config_json { Ok(()) } else { Err(StoreError::IdempotencyConflict) }; }
			transaction.execute("INSERT INTO chief_root_settings (root_id, config_json, created_at_micros) VALUES (?1, ?2, ?3)", params![root_id, config_json, unix_micros()?]).map_err(sql_error)?;
			transaction.commit().map_err(sql_error)?;
			Ok(())
		}).await
	}

	pub async fn read_chief_root_settings(
		&self,
		root_id: &str,
	) -> Result<Option<String>, StoreError> {
		let root_id = root_id.to_owned();
		self.run(move |connection| {
			connection
				.query_row(
					"SELECT config_json FROM chief_root_settings WHERE root_id = ?1",
					[&root_id],
					|row| row.get(0),
				)
				.optional()
				.map_err(sql_error)
		})
		.await
	}

	pub async fn read_chief_process_binding(
		&self,
		root_id: &str,
	) -> Result<Option<ChiefProcessBinding>, StoreError> {
		let root_id = root_id.to_owned();
		self.run(move |connection| {
			let row = connection.query_row("SELECT root_id, account_id, operation_key, generation_id FROM chief_process_bindings WHERE root_id = ?1 ORDER BY created_at_micros DESC, rowid DESC LIMIT 1", [&root_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?))).optional().map_err(sql_error)?;
			row.map(|(root_id, account_id, operation_key, generation_id)| Ok(ChiefProcessBinding {
				root_id, account_id: AccountId::new(account_id).map_err(|_| DatabaseError::Corrupt)?, operation_key,
				generation_id: ProcessGenerationId::new(generation_id).map_err(|_| DatabaseError::Corrupt)?,
			})).transpose()
		}).await
	}

	/// Persist root/account/operation ownership and the process fence in one transaction.
	/// Exact replays never return a fresh fence and cannot authorize a second spawn.
	pub async fn prepare_chief_bound_process_generation(
		&self,
		intent: &ProcessGenerationIntent,
		binding: &ProcessGenerationAccountBinding,
		root_id: &str,
		operation_key: &str,
	) -> Result<PrepareProcessGenerationOutcome, StoreError> {
		if root_id.trim().is_empty()
			|| root_id.len() > 512
			|| operation_key.trim().is_empty()
			|| operation_key.len() > 256
		{
			return Err(StoreError::InvalidInput("Chief process admission identity is invalid"));
		}
		let digest = admission_digest(intent, binding, root_id, operation_key)?;
		let (intent, binding, root_id, operation_key) =
			(intent.clone(), binding.clone(), root_id.to_owned(), operation_key.to_owned());
		self.run(move |connection| {
			let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql_error)?;
			let root_exists: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM chief_work_items WHERE id = ?1 AND kind = 'goal' AND parent_goal_id IS NULL)", [&root_id], |row| row.get(0)).map_err(sql_error)?;
			if !root_exists { return Err(StoreError::InvalidInput("Chief process requires a root goal")); }
			let previous: Option<(String, String)> = transaction.query_row("SELECT generation_id, request_sha256 FROM chief_process_bindings WHERE operation_key = ?1", [&operation_key], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(sql_error)?;
			if let Some((generation_id, recorded_digest)) = previous {
				if recorded_digest != digest { return Ok(rejected(ProcessGenerationRejection::IdentityConflict)); }
				let generation = read_generation(&transaction, &generation_id)?.ok_or(DatabaseError::Corrupt)?;
				return Ok(PrepareProcessGenerationOutcome::Replayed(ProcessGenerationMutation { revision: generation.revision, state: generation.state, recorded_at_micros: generation.updated_at_micros }));
			}
			let affinity_conflict: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM chief_process_bindings b JOIN process_generations g ON g.generation_id=b.generation_id WHERE b.root_id = ?1 AND b.account_id <> ?2 AND g.state <> 'dead')", params![root_id, intent.account_id.as_str()], |row| row.get(0)).map_err(sql_error)?;
            let changing_account: bool = transaction.query_row("SELECT coalesce((SELECT account_id <> ?2 FROM chief_process_bindings WHERE root_id=?1 ORDER BY created_at_micros DESC,rowid DESC LIMIT 1),0)",params![root_id,intent.account_id.as_str()],|row|row.get(0)).map_err(sql_error)?;
            if changing_account {
                let busy: bool = transaction.query_row("WITH RECURSIVE family(id) AS (SELECT ?1 UNION SELECT w.id FROM chief_work_items w JOIN family f ON w.parent_goal_id=f.id) SELECT EXISTS(SELECT 1 FROM chief_work_items WHERE id IN (SELECT id FROM family) AND dispatch_state <> 'idle')",[&root_id],|row|row.get(0)).map_err(sql_error)?;
                if busy { return Ok(rejected(ProcessGenerationRejection::IdentityConflict)); }
            }
			if affinity_conflict || read_generation(&transaction, intent.generation_id.as_str())?.is_some() {
				return Ok(rejected(ProcessGenerationRejection::IdentityConflict));
			}
			let outcome = prepare_bound_generation(&transaction, &intent, &binding, None, None)?;
			if !matches!(outcome, PrepareProcessGenerationOutcome::Fresh(_)) { return Ok(outcome); }
			transaction.execute("INSERT INTO chief_process_bindings (operation_key, root_id, account_id, generation_id, request_sha256, created_at_micros) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![operation_key, root_id, intent.account_id.as_str(), intent.generation_id.as_str(), digest, unix_micros()?]).map_err(sql_error)?;
			transaction.commit().map_err(sql_error)?;
			Ok(outcome)
		}).await
	}
}

fn rejected(rejection: ProcessGenerationRejection) -> PrepareProcessGenerationOutcome {
	PrepareProcessGenerationOutcome::Rejected { rejection, actual: empty_mutation() }
}

fn admission_digest(
	intent: &ProcessGenerationIntent,
	binding: &ProcessGenerationAccountBinding,
	root_id: &str,
	operation_key: &str,
) -> Result<String, StoreError> {
	let request = serde_json::json!([
		root_id,
		operation_key,
		intent.generation_id.as_str(),
		intent.account_id.as_str(),
		intent.execution_authorization.epoch_id.as_str(),
		intent.execution_authorization.authorization_digest,
		intent.runner_identity.as_str(),
		intent.intended_boot_id.as_str(),
		intent.control_kind.as_sql(),
		intent.isolation_kind.as_sql(),
		binding.account_revision,
		binding.credential.schema_version.get(),
		binding.credential.version.get(),
		binding.credential.fingerprint.as_str(),
		binding.credential.writer_operation_id.as_str(),
		format!("{:?}", binding.credential.provider.provider()),
		binding.credential.provider.account_id(),
		binding.refresh_callback_profile_sha256,
	]);
	let encoded = serde_json::to_vec(&request)
		.map_err(|_| StoreError::InvalidInput("Chief process admission cannot be encoded"))?;
	Ok(Sha256::digest(encoded).iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(crate) fn owns_work(
	connection: &rusqlite::Connection,
	work: &str,
	generation: Option<&str>,
) -> Result<bool, StoreError> {
	if let Some(generation) = generation {
		connection.query_row("WITH RECURSIVE owned(id,root_id) AS (
			SELECT b.root_id,b.root_id FROM chief_process_bindings b JOIN process_generations g ON g.generation_id=b.generation_id
			WHERE b.generation_id=?1 AND g.state='ready' AND b.rowid=(SELECT rowid FROM chief_process_bindings WHERE root_id=b.root_id ORDER BY created_at_micros DESC,rowid DESC LIMIT 1)
			UNION SELECT w.id,owned.root_id FROM chief_work_items w JOIN owned ON w.parent_goal_id=owned.id
			WHERE NOT EXISTS(SELECT 1 FROM chief_managers WHERE work_id=w.id))
			SELECT EXISTS(SELECT 1 FROM owned WHERE id=?2)", params![generation,work], |r|r.get(0)).map_err(sql_error)
	} else {
		// Direct coordinator transports have no durable process host. They cannot
		// bypass ownership once any native process admission exists in this store.
		connection
			.query_row("SELECT NOT EXISTS(SELECT 1 FROM chief_process_bindings)", [], |r| r.get(0))
			.map_err(sql_error)
	}
}

#[cfg(test)]
mod tests {
	mod auth_recovery;
	mod response_usage;
	use super::*;
	use crate::{
		ChiefDispatchState, ChiefWorkItem, ChiefWorkKind, ChiefWorkStatus,
		CodexAccountCapabilityAttestation,
	};
	use decodex_core::{
		AccountOperationId, AccountProvider, CredentialBinding, CredentialFingerprint,
		CredentialStoreSchemaVersion, CredentialVersion, ProcessBootIdentity, ProcessControlKind,
		ProcessDeathEvidence, ProcessDeathEvidenceId, ProcessDeathEvidenceKind,
		ProcessExecutionAuthorization, ProcessExecutionEpochId, ProcessIsolationKind,
		ProcessRunnerIdentity, ProviderIdentity,
	};

	const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
	const OTHER_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

	fn account_id(number: u8) -> AccountId {
		AccountId::new(format!("10000000-0000-4000-8000-{number:012}")).unwrap()
	}
	fn operation_id(number: u8) -> AccountOperationId {
		AccountOperationId::new(format!("20000000-0000-4000-8000-{number:012}")).unwrap()
	}
	fn generation_id(number: u8) -> ProcessGenerationId {
		ProcessGenerationId::new(format!("30000000-0000-4000-8000-{number:012}")).unwrap()
	}
	fn binding(number: u8) -> ProcessGenerationAccountBinding {
		ProcessGenerationAccountBinding::new(
			1,
			CredentialBinding {
				schema_version: CredentialStoreSchemaVersion::V1,
				version: CredentialVersion::new(1).unwrap(),
				fingerprint: CredentialFingerprint::new(DIGEST).unwrap(),
				provider: ProviderIdentity::new(
					AccountProvider::Chatgpt,
					format!("provider-{number}"),
				)
				.unwrap(),
				writer_operation_id: operation_id(number),
			},
			DIGEST,
		)
		.unwrap()
	}
	fn intent(account: u8, generation: u8) -> ProcessGenerationIntent {
		ProcessGenerationIntent {
			generation_id: generation_id(generation),
			account_id: account_id(account),
			runner_identity: ProcessRunnerIdentity::new(format!("sha256:{DIGEST}")).unwrap(),
			intended_boot_id: ProcessBootIdentity::new("fixture-boot").unwrap(),
			control_kind: ProcessControlKind::StdioOnlyBestEffortEof,
			isolation_kind: ProcessIsolationKind::Session,
			execution_authorization: ProcessExecutionAuthorization::new(
				ProcessExecutionEpochId::new("40000000-0000-4000-8000-000000000001").unwrap(),
				DIGEST,
			)
			.unwrap(),
		}
	}
	async fn seed(store: &SqliteStore) {
		store.with_connection(|connection| {
			for number in [1, 2] {
				connection.execute("INSERT INTO account_identities VALUES (?1, 1)", [account_id(number).as_str()]).map_err(crate::error::sqlite_error)?;
				connection.execute("INSERT INTO account_operations (operation_id, account_id, kind, phase, provider, provider_account_id, requested_display_label, requested_enabled, created_at_micros, updated_at_micros, completed_at_micros) VALUES (?1, ?2, 'enroll', 'committed', 'chatgpt', ?3, 'Fixture', 1, 1, 1, 1)", params![operation_id(number).as_str(), account_id(number).as_str(), format!("provider-{number}")]).map_err(crate::error::sqlite_error)?;
				connection.execute("INSERT INTO accounts VALUES (?1, 'Fixture', 1, 'available', 1, 'chatgpt', ?2, 'exact', 1, 1, NULL)", params![account_id(number).as_str(), format!("provider-{number}")]).map_err(crate::error::sqlite_error)?;
				connection.execute("INSERT INTO account_credentials VALUES (?1, 1, 1, ?2, ?3, 'chatgpt', ?4, X'01020304', 1)", params![account_id(number).as_str(), DIGEST, operation_id(number).as_str(), format!("provider-{number}")]).map_err(crate::error::sqlite_error)?;
			}
			Ok(())
		}).unwrap();
		store
			.attest_codex_account_capability(&CodexAccountCapabilityAttestation {
				build_identity: "fixture".into(),
				executable_sha256: DIGEST.into(),
				schema_sha256: DIGEST.into(),
				callback_profile_sha256: DIGEST.into(),
				login_chatgpt_auth_tokens: true,
				refresh_callback: true,
			})
			.await
			.unwrap();
		for id in ["root", "second-root"] {
			store
				.create_chief_work_item(ChiefWorkItem {
					id: id.into(),
					parent_goal_id: None,
					kind: ChiefWorkKind::Goal,
					title: id.into(),
					instructions: "Fixture root".into(),
					codex_thread_id: None,
					dispatch_state: ChiefDispatchState::Idle,
					active_turn_id: None,
					status: ChiefWorkStatus::Open,
					next_check_at_micros: None,
					created_at_micros: 1,
					updated_at_micros: 1,
				})
				.await
				.unwrap();
		}
	}

	#[tokio::test]
	async fn chief_process_admission_checks_authority_and_preserves_affinity_without_phantom_conversations()
	 {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("chief.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		seed(&store).await;
		let mut stale = binding(1);
		stale.account_revision = 2;
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(&intent(1, 1), &stale, "root", "stale")
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Rejected {
				rejection: ProcessGenerationRejection::AccountLifecycleUnready,
				..
			}
		));
		let mut wrong_callback = binding(1);
		wrong_callback.refresh_callback_profile_sha256 = OTHER_DIGEST.into();
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(
					&intent(1, 1),
					&wrong_callback,
					"root",
					"callback"
				)
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Rejected {
				rejection: ProcessGenerationRejection::CallbackCapabilityUnready,
				..
			}
		));
		store
			.bind_chief_root_settings("root", r#"{"model":"fixture-model","cwd":"/tmp"}"#)
			.await
			.unwrap();
		store
			.bind_chief_root_settings("root", r#"{"model":"fixture-model","cwd":"/tmp"}"#)
			.await
			.unwrap();
		assert!(store.bind_chief_root_settings("root", r#"{"model":"different"}"#).await.is_err());
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(&intent(1, 1), &binding(1), "root", "first")
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Fresh(_)
		));
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(&intent(1, 1), &binding(1), "root", "first")
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Replayed(_)
		));
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(
					&intent(1, 2),
					&binding(1),
					"second-root",
					"concurrent-root"
				)
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Rejected {
				rejection: ProcessGenerationRejection::AccountQuarantined,
				..
			}
		));
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(
					&intent(2, 2),
					&binding(2),
					"root",
					"switch-account"
				)
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Rejected {
				rejection: ProcessGenerationRejection::IdentityConflict,
				..
			}
		));
		let mut bad_epoch = intent(2, 2);
		bad_epoch.execution_authorization.authorization_digest = OTHER_DIGEST.into();
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(
					&bad_epoch,
					&binding(2),
					"second-root",
					"epoch-conflict"
				)
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Rejected {
				rejection: ProcessGenerationRejection::RestoreAuthorityUnavailable,
				..
			}
		));
		drop(store);
		assert_restart_affinity(&path).await;
	}

	async fn assert_restart_affinity(path: &std::path::Path) {
		let store = SqliteStore::open_test(path).unwrap();
		let recorded = store.read_chief_process_binding("root").await.unwrap().unwrap();
		assert_eq!(recorded.account_id, account_id(1));
		assert_eq!(recorded.generation_id, generation_id(1));
		assert_eq!(
			store.read_chief_root_settings("root").await.unwrap().as_deref(),
			Some(r#"{"model":"fixture-model","cwd":"/tmp"}"#)
		);
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(&intent(1, 1), &binding(1), "root", "first")
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Replayed(_)
		));
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(
					&intent(1, 2),
					&binding(1),
					"root",
					"restart"
				)
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Rejected {
				rejection: ProcessGenerationRejection::AccountQuarantined,
				..
			}
		));
		store.with_connection(|connection| {
			let counts: (i64, i64, i64) = connection.query_row("SELECT (SELECT count(*) FROM conversations), (SELECT count(*) FROM runtime_sessions), (SELECT count(*) FROM process_generations WHERE runtime_session_id IS NULL AND quick_task_admission_key IS NULL)", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).map_err(crate::error::sqlite_error)?;
			assert_eq!(counts, (0, 0, 1));
			Ok(())
		}).unwrap();
		let evidence = ProcessDeathEvidence::new(
			ProcessDeathEvidenceId::new("50000000-0000-4000-8000-000000000001").unwrap(),
			generation_id(1),
			ProcessDeathEvidenceKind::SpawnNotCreated,
			ProcessBootIdentity::new("fixture-boot").unwrap(),
			None,
			DIGEST,
		)
		.unwrap();
		store.record_process_generation_death(1, &evidence).await.unwrap();
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(
					&intent(1, 2),
					&binding(1),
					"root",
					"after-positive-death"
				)
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Fresh(_)
		));
		assert_eq!(
			store.read_chief_process_binding("root").await.unwrap().unwrap().generation_id,
			generation_id(2)
		);
		store.revalidate().await.unwrap();
	}
	#[tokio::test]
	async fn account_rotation_requires_dead_process_and_idle_work_and_preserves_thread() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("rotation.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		seed(&store).await;
		store.bind_chief_thread("root".into(), "original-thread".into()).await.unwrap();
		store
			.prepare_chief_bound_process_generation(&intent(1, 1), &binding(1), "root", "first")
			.await
			.unwrap();
		let evidence = ProcessDeathEvidence::new(
			ProcessDeathEvidenceId::new("50000000-0000-4000-8000-000000000001").unwrap(),
			generation_id(1),
			ProcessDeathEvidenceKind::SpawnNotCreated,
			ProcessBootIdentity::new("fixture-boot").unwrap(),
			None,
			DIGEST,
		)
		.unwrap();
		store.record_process_generation_death(1, &evidence).await.unwrap();
		store.begin_chief_dispatch("root".into()).await.unwrap();
		store.mark_chief_dispatch_unknown("root".into()).await.unwrap();
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(&intent(2, 2), &binding(2), "root", "busy")
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Rejected {
				rejection: ProcessGenerationRejection::IdentityConflict,
				..
			}
		));
		store.reconcile_chief_dispatch("root".into(), "finished".into()).await.unwrap();
		store.complete_chief_turn("root".into(), "finished".into()).await.unwrap();
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(
					&intent(2, 2),
					&binding(2),
					"root",
					"rotate"
				)
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Fresh(_)
		));
		drop(store);
		let reopened = SqliteStore::open_test(&path).unwrap();
		assert_eq!(
			reopened.read_chief_process_binding("root").await.unwrap().unwrap().account_id,
			account_id(2)
		);
		assert_eq!(
			reopened.get_chief_work_item("root".into()).await.unwrap().codex_thread_id.as_deref(),
			Some("original-thread")
		);
		reopened.revalidate().await.unwrap();
	}
	#[tokio::test]
	async fn admission_ignores_only_superseded_credential_recovery() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("superseded.sqlite3")).unwrap();
		seed(&store).await;
		store.with_connection(|connection| {
            connection.execute("INSERT INTO account_operations(operation_id,account_id,kind,phase,provider,provider_account_id,recovery_code,created_at_micros,updated_at_micros) VALUES(?1,?2,'refresh','recovery_required','chatgpt','provider-2','fixture',1,1)",params![operation_id(9).as_str(),account_id(2).as_str()]).map_err(crate::error::sqlite_error)?;
            Ok(())
        }).unwrap();
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(
					&intent(2, 1),
					&binding(2),
					"root",
					"pending"
				)
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Rejected {
				rejection: ProcessGenerationRejection::AccountLifecycleUnready,
				..
			}
		));
		store
			.with_connection(|connection| {
				connection
					.execute(
						"UPDATE account_operations SET superseded_by_operation_id=?1 WHERE operation_id=?2",
						params![operation_id(2).as_str(), operation_id(9).as_str()],
					)
					.map_err(crate::error::sqlite_error)?;
				connection
					.execute(
						"UPDATE account_operations SET recovery_operation_id=?1 WHERE operation_id=?2",
						params![operation_id(9).as_str(), operation_id(2).as_str()],
					)
					.map_err(crate::error::sqlite_error)?;
				Ok(())
			})
			.unwrap();
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(
					&intent(2, 1),
					&binding(2),
					"root",
					"recovered"
				)
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Fresh(_)
		));
	}
	#[tokio::test]
	async fn config_warning_receipts_are_process_owned_bounded_and_durable() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("warnings.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		seed(&store).await;
		store
			.prepare_chief_bound_process_generation(
				&intent(1, 1),
				&binding(1),
				"root",
				"warning-process",
			)
			.await
			.unwrap();
		let generation = generation_id(1).as_str().to_owned();
		store
			.record_chief_config_warning(
				"root".into(),
				generation.clone(),
				DIGEST.into(),
				"not-ready".into(),
			)
			.await
			.unwrap();
		let identity = decodex_core::ProcessIdentity::new(
			ProcessBootIdentity::new("fixture-boot").unwrap(),
			1234,
			decodex_core::ProcessStartIdentity::new("fixture-start").unwrap(),
			1234,
			1234,
		)
		.unwrap();
		store.bind_process_generation_identity(&generation_id(1), 1, &identity).await.unwrap();
		store.mark_process_generation_ready(&generation_id(1), 2).await.unwrap();
		store
			.record_chief_config_warning(
				"second-root".into(),
				generation.clone(),
				DIGEST.into(),
				"wrong-root".into(),
			)
			.await
			.unwrap();
		store
			.record_chief_config_warning(
				"root".into(),
				generation_id(2).as_str().into(),
				DIGEST.into(),
				"wrong-process".into(),
			)
			.await
			.unwrap();
		for _ in 0..2 {
			store
				.record_chief_config_warning(
					"root".into(),
					generation.clone(),
					DIGEST.into(),
					"Original warning".into(),
				)
				.await
				.unwrap();
		}
		let (events, _) = store.read_chief_transcript("root".into(), None, 100).await.unwrap();
		assert_eq!(events.len(), 1);
		assert!(events[0].payload.contains("Original warning"));
		assert!(
			store
				.read_chief_transcript("second-root".into(), None, 100)
				.await
				.unwrap()
				.0
				.is_empty()
		);
		for i in 0..70 {
			store
				.record_chief_config_warning(
					"root".into(),
					generation.clone(),
					format!("{i:064x}"),
					format!("Warning {i}"),
				)
				.await
				.unwrap();
		}
		assert!(store.list_chief_wake_events("root".into(), 100).await.unwrap().is_empty());
		drop(store);
		let store = SqliteStore::open_test(&path).unwrap();
		let (events, _) = store.read_chief_transcript("root".into(), None, 100).await.unwrap();
		assert_eq!(events.len(), 65);
		assert_eq!(
			events.iter().filter(|e| e.payload.contains("exceeded the display limit")).count(),
			1
		);
	}

	fn guardian_observation(
		work: &str,
		generation: Option<String>,
	) -> crate::ChiefGuardianObservation {
		crate::ChiefGuardianObservation {
			thread_id:format!("thread-{work}"),turn_id:"turn".into(),review_id:"review".into(),connection_id:"connection".into(),generation_id:generation,
			event_json:serde_json::json!({"threadId":format!("thread-{work}"),"turnId":"turn","reviewId":"review","startedAtMs":1,"completedAtMs":2,"review":{"status":"denied"},"action":{"type":"command","source":"shell","command":"echo fixture","cwd":"/tmp"}}).to_string()
    }
	}

	async fn assert_guardian_observation_ownership(
		store: &SqliteStore,
	) -> decodex_core::ProcessIdentity {
		let mut manager = store.get_chief_work_item("root".into()).await.unwrap();
		manager.id = "manager".into();
		manager.parent_goal_id = Some("root".into());
		store.create_chief_manager(manager, None).await.unwrap();
		for work in ["root", "second-root", "manager"] {
			store.bind_chief_thread(work.into(), format!("thread-{work}")).await.unwrap();
			store.begin_chief_dispatch(work.into()).await.unwrap();
			store.acknowledge_chief_dispatch(work.into(), "turn".into()).await.unwrap();
		}
		store
			.prepare_chief_bound_process_generation(
				&intent(1, 1),
				&binding(1),
				"root",
				"guardian-process",
			)
			.await
			.unwrap();

		store
			.record_chief_guardian_review(guardian_observation(
				"root",
				Some(generation_id(1).as_str().into()),
			))
			.await
			.unwrap();
		assert!(
			store.read_chief_guardian_reviews("root".into(), None, 1).await.unwrap().is_empty()
		);
		let identity = decodex_core::ProcessIdentity::new(
			ProcessBootIdentity::new("fixture-boot").unwrap(),
			1234,
			decodex_core::ProcessStartIdentity::new("fixture-start").unwrap(),
			1234,
			1234,
		)
		.unwrap();
		store.bind_process_generation_identity(&generation_id(1), 1, &identity).await.unwrap();
		store.mark_process_generation_ready(&generation_id(1), 2).await.unwrap();
		for (work, thread, expected) in [
			("root", "thread-root", true),
			("root", "foreign", false),
			("second-root", "thread-second-root", false),
			("manager", "thread-manager", false),
		] {
			assert_eq!(
				store
					.chief_thread_is_owned(
						work.into(),
						thread.into(),
						Some(generation_id(1).as_str().into())
					)
					.await
					.unwrap(),
				expected
			);
			assert!(!store.chief_thread_is_owned(work.into(), thread.into(), None).await.unwrap());
		}
		for work in ["root", "second-root", "manager"] {
			store.record_chief_guardian_review(guardian_observation(work, None)).await.unwrap();
			assert!(
				store.read_chief_guardian_reviews(work.into(), None, 1).await.unwrap().is_empty()
			);
			store
				.record_chief_guardian_review(guardian_observation(
					work,
					Some(generation_id(1).as_str().into()),
				))
				.await
				.unwrap();
			assert_eq!(
				store.read_chief_guardian_reviews(work.into(), None, 1).await.unwrap().len(),
				usize::from(work == "root")
			);
		}
		identity
	}

	async fn assert_guardian_submission_ownership(store: &SqliteStore) {
		let saved =
			store.read_chief_guardian_reviews("root".into(), None, 1).await.unwrap().remove(0);
		for generation in [None, Some(generation_id(2).as_str().into())] {
			assert!(
				store
					.begin_chief_guardian_approval(
						"root".into(),
						saved.id,
						saved.digest(),
						"connection".into(),
						generation,
						"wrong-process".into()
					)
					.await
					.is_err()
			);
		}
		assert!(
			store
				.begin_chief_guardian_approval(
					"second-root".into(),
					saved.id,
					saved.digest(),
					"connection".into(),
					Some(generation_id(1).as_str().into()),
					"wrong-root".into()
				)
				.await
				.is_err()
		);
		let claim = store
			.begin_chief_guardian_approval(
				"root".into(),
				saved.id,
				saved.digest(),
				"connection".into(),
				Some(generation_id(1).as_str().into()),
				"explicit".into(),
			)
			.await
			.unwrap();
		store.finish_chief_guardian_approval(claim, true).await.unwrap();
		assert_eq!(
			store
				.chief_guardian_review("root".into(), saved.id)
				.await
				.unwrap()
				.unwrap()
				.approval_state
				.as_deref(),
			Some("submitted")
		);
	}

	#[tokio::test]
	async fn guardian_observation_and_submission_require_current_process_and_root_ownership() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("guardian.sqlite3")).unwrap();
		seed(&store).await;
		let identity = assert_guardian_observation_ownership(&store).await;
		assert_guardian_submission_ownership(&store).await;
		let mut retained = guardian_observation("root", Some(generation_id(1).as_str().into()));
		retained.review_id = "after-restart".into();
		let mut event: serde_json::Value = serde_json::from_str(&retained.event_json).unwrap();
		event["reviewId"] = serde_json::json!(retained.review_id);
		retained.event_json = event.to_string();
		store.record_chief_guardian_review(retained).await.unwrap();
		let retained =
			store.read_chief_guardian_reviews("root".into(), None, 1).await.unwrap().remove(0);
		let death = ProcessDeathEvidence::new(
			ProcessDeathEvidenceId::new("50000000-0000-4000-8000-000000000001").unwrap(),
			generation_id(1),
			ProcessDeathEvidenceKind::OwnedChildExit,
			ProcessBootIdentity::new("fixture-boot").unwrap(),
			Some(identity),
			DIGEST,
		)
		.unwrap();
		assert!(matches!(
			store.record_process_generation_death(3, &death).await.unwrap(),
			crate::ProcessGenerationMutationOutcome::Applied(_)
		));
		drop(store);
		let store = SqliteStore::open_test(&directory.path().join("guardian.sqlite3")).unwrap();
		assert!(matches!(
			store
				.prepare_chief_bound_process_generation(
					&intent(1, 2),
					&binding(1),
					"root",
					"guardian-reconnect"
				)
				.await
				.unwrap(),
			PrepareProcessGenerationOutcome::Fresh(_)
		));
		let identity = decodex_core::ProcessIdentity::new(
			ProcessBootIdentity::new("fixture-boot").unwrap(),
			5678,
			decodex_core::ProcessStartIdentity::new("reconnected-start").unwrap(),
			5678,
			5678,
		)
		.unwrap();
		store.bind_process_generation_identity(&generation_id(2), 1, &identity).await.unwrap();
		store.mark_process_generation_ready(&generation_id(2), 2).await.unwrap();
		assert!(
			store
				.begin_chief_guardian_approval(
					"root".into(),
					retained.id,
					retained.digest(),
					"old-connection".into(),
					Some(generation_id(1).as_str().into()),
					"stale-process".into()
				)
				.await
				.is_err()
		);
		let claim = store
			.begin_chief_guardian_approval(
				"root".into(),
				retained.id,
				retained.digest(),
				"new-connection".into(),
				Some(generation_id(2).as_str().into()),
				"restored-user-decision".into(),
			)
			.await
			.unwrap();
		store.finish_chief_guardian_approval(claim, true).await.unwrap();
		let restored =
			store.chief_guardian_review("root".into(), retained.id).await.unwrap().unwrap();
		assert_eq!(restored.event_json, retained.event_json);
		assert_eq!(restored.approval_key.as_deref(), Some("restored-user-decision"));
	}

	#[tokio::test]
	async fn native_voice_turns_are_owned_deduplicated_and_never_requeued() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("voice.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();
		seed(&store).await;
		store.bind_chief_thread("root".into(), "voice-thread".into()).await.unwrap();
		store
			.prepare_chief_bound_process_generation(
				&intent(1, 1),
				&binding(1),
				"root",
				"voice-process",
			)
			.await
			.unwrap();
		let identity = decodex_core::ProcessIdentity::new(
			ProcessBootIdentity::new("fixture-boot").unwrap(),
			1234,
			decodex_core::ProcessStartIdentity::new("fixture-start").unwrap(),
			1234,
			1234,
		)
		.unwrap();
		store.bind_process_generation_identity(&generation_id(1), 1, &identity).await.unwrap();
		store.mark_process_generation_ready(&generation_id(1), 2).await.unwrap();
		let call = crate::ChiefVoiceCall {
			session_id: "voice-1".into(),
			work_id: "root".into(),
			thread_id: "voice-thread".into(),
			generation_id: generation_id(1).as_str().into(),
			baseline_turn_id: Some("baseline".into()),
		};
		store.begin_chief_voice_call(call.clone()).await.unwrap();
		let mut other = call.clone();
		other.session_id = "voice-2".into();
		assert!(store.begin_chief_voice_call(other.clone()).await.is_err());
		assert!(
			!store
				.observe_chief_voice_turn(
					generation_id(2).as_str().into(),
					"voice-thread".into(),
					"new-turn".into()
				)
				.await
				.unwrap()
		);
		assert!(
			!store
				.observe_chief_voice_turn(
					generation_id(1).as_str().into(),
					"voice-thread".into(),
					"baseline".into()
				)
				.await
				.unwrap()
		);
		assert!(
			store
				.observe_chief_voice_turn(
					generation_id(1).as_str().into(),
					"voice-thread".into(),
					"new-turn".into()
				)
				.await
				.unwrap()
		);
		store.close_chief_voice_call("voice-1".into()).await.unwrap();
		assert_eq!(
			store.get_chief_work_item("root".into()).await.unwrap().dispatch_state,
			ChiefDispatchState::Running
		);
		store.complete_chief_turn("root".into(), "new-turn".into()).await.unwrap();
		assert!(
			!store
				.observe_chief_voice_turn(
					generation_id(1).as_str().into(),
					"voice-thread".into(),
					"new-turn".into()
				)
				.await
				.unwrap()
		);
		assert_eq!(
			store.get_chief_work_item("root".into()).await.unwrap().dispatch_state,
			ChiefDispatchState::Idle
		);
		store
			.record_chief_voice_transcript(
				"voice-1".into(),
				1,
				"user".into(),
				"A spoken request".into(),
			)
			.await
			.unwrap();
		assert!(store.list_chief_wake_events("root".into(), 100).await.unwrap().is_empty());
		store.begin_chief_voice_call(other.clone()).await.unwrap();
		drop(store);
		let reopened = SqliteStore::open_test(&path).unwrap();
		assert_eq!(reopened.open_chief_voice_calls().await.unwrap(), vec![other]);
	}
}
