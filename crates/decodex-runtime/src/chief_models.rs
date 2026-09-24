//! Persist transport-current model settings for selection and recovery.
use decodex_database::SqliteStore;
use serde_json::json;
use sha2::{Digest as _, Sha256};

/// Save only transport-current facts. Missing facts invalidate the saved observation without
/// settling a selection. Receipt settlement records an observation, not request causation.
pub(crate) async fn persist_current(
	store: &SqliteStore,
	client: &decodex_codex::app_server_client::AppServerClient,
	thread: &str,
	generation: Option<String>,
) -> Result<(), decodex_database::StoreError> {
	let observed = client.configured_task_models(thread);
	let current = observed.as_ref().is_some_and(|(_, guard)| guard.is_live());
	let settings_revision = observed.as_ref().and_then(|(_, guard)| guard.settings_revision());
	let settings = observed.map(|(facts, _)| facts);
	let encoded = settings.as_ref().map(|facts| serde_json::to_string(facts).expect("model facts"));
	let identity =
		json!([generation, thread, client.history_revision(), settings_revision, settings]);
	let digest: String = Sha256::digest(identity.to_string().as_bytes())
		.iter()
		.map(|b| format!("{b:02x}"))
		.collect();
	if current {
		store
			.record_chief_task_models_publication(thread.into(), generation, encoded, digest)
			.await?;
	} else {
		store.record_chief_task_models(thread.into(), generation, None, digest).await?;
	}
	Ok(())
}
