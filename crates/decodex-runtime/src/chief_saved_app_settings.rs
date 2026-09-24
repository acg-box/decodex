//! Manage existing native config entries without retaining or inventing a cloud connection list.
use super::{EditRequest, Review, SettingsGuard, shared, submit};
use crate::chief_usage_estimate::Source;
use decodex_codex::app_server_client::{AppLinkSettings, AppLinkSettingsCatalog, HistoryGuard};
use decodex_database::{ChiefAppSettingsReceipt, ChiefConfigReceipt, SqliteStore};
use decodex_protocol::{
	ChiefAppSettingEdit, ChiefAppSettingsResult, ChiefSavedAppConnection,
	ChiefSavedAppSettingsResult as State,
};
use serde_json::json;
struct Catalog {
	source: Source,
	guard: HistoryGuard,
	native: AppLinkSettingsCatalog,
	scope: String,
	prior: Option<ChiefAppSettingsReceipt>,
	shared: Option<ChiefConfigReceipt>,
	can_update: bool,
}
async fn inspect<F, Fut>(store: &SqliteStore, source: &F) -> Option<Catalog>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	let before = source().await?;
	let k = &before.key;
	if !store
		.chief_thread_is_owned(k.work.clone(), k.thread.clone(), Some(k.generation.as_str().into()))
		.await
		.ok()?
	{
		return None;
	}
	let guard = before.client.thread_settings_guard(&k.thread)?;
	let thread =
		before.client.thread_read(json!({"threadId":k.thread,"includeTurns":false})).await.ok()?;
	if thread["thread"]["id"] != k.thread {
		return None;
	}
	let cwd = thread["thread"]["cwd"].as_str()?;
	let native = before.client.saved_app_link_settings(cwd).await.ok()?;
	let scope = shared::digest(native.config_file());
	if !guard.is_live() || source().await.is_none_or(|after| after.key != before.key) {
		return None;
	}
	shared::reconcile(store, &before, cwd, &scope).await;
	let last = store.chief_config_receipt(scope.clone()).await.ok()?;
	let prior = store.chief_app_settings_receipt(scope.clone()).await.ok()?;
	let can_update = store.get_chief_work_item(k.work.clone()).await.ok()?.status
		!= decodex_database::ChiefWorkStatus::Resolved
		&& !last.as_ref().is_some_and(shared::pending);
	if !guard.is_live() || source().await.is_none_or(|after| after.key != before.key) {
		return None;
	}
	Some(Catalog { source: before, guard, native, scope, prior, shared: last, can_update })
}
fn project(catalog: &Catalog, native: &AppLinkSettings) -> ChiefSavedAppConnection {
	let k = &catalog.source.key;
	let token = shared::digest(
		&json!([
			"saved-app",
			k.work,
			k.thread,
			k.generation.as_str(),
			k.account.as_str(),
			k.revision,
			k.history_revision,
			native.review_fingerprint(),
			catalog.prior.as_ref().map(|r| (r.id, &r.state)),
			catalog.shared.as_ref().map(shared::project)
		])
		.to_string(),
	);
	ChiefSavedAppConnection {
		connector_id: native.app_id().into(),
		link_id: native.link_id().into(),
		review_token: token,
		user_mode: native.user_mode.clone(),
		user_reviewer: native.user_reviewer.clone(),
		effective_mode: native.effective_mode.clone(),
		effective_reviewer: native.effective_reviewer.clone(),
	}
}
pub(crate) async fn read_saved<F, Fut>(store: &SqliteStore, source: F) -> State
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	let Some(catalog) =
		tokio::time::timeout(std::time::Duration::from_secs(40), inspect(store, &source))
			.await
			.ok()
			.flatten()
	else {
		return State::Unavailable;
	};
	State::Available {
		work_id: catalog.source.key.work.clone(),
		thread_id: catalog.source.key.thread.clone(),
		config_file: catalog.native.config_file().into(),
		connections: catalog.native.entries.iter().map(|e| project(&catalog, e)).collect(),
		can_update: catalog.can_update,
		last_edit: catalog.shared.as_ref().map(shared::project).map(Box::new),
	}
}
pub(crate) struct SavedSelection<'a> {
	pub thread: &'a str,
	pub connector: &'a str,
	pub link: &'a str,
	pub review: &'a str,
	pub edit: &'a ChiefAppSettingEdit,
	pub attempt_id: &'a str,
}
pub(crate) async fn write_saved<F, Fut>(
	store: &SqliteStore,
	source: F,
	selection: SavedSelection<'_>,
) -> Result<(), crate::chief_host::ChiefHostError>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	use crate::chief_host::ChiefHostError::Rejected;
	let mut catalog =
		tokio::time::timeout(std::time::Duration::from_secs(40), inspect(store, &source))
			.await
			.ok()
			.flatten()
			.ok_or(Rejected("Current saved app settings are unavailable."))?;
	if catalog.source.key.thread != selection.thread {
		return Err(Rejected("The task's native thread changed."));
	}
	let index = catalog
		.native
		.entries
		.iter()
		.position(|e| e.app_id() == selection.connector && e.link_id() == selection.link)
		.ok_or(Rejected("This connection no longer has a saved override. Refresh settings."))?;
	let native = catalog.native.entries.remove(index);
	let row = project(&catalog, &native);
	let state = ChiefAppSettingsResult::Available {
		connector_id: row.connector_id,
		link_id: row.link_id,
		review_token: row.review_token,
		user_mode: row.user_mode,
		user_reviewer: row.user_reviewer,
		effective_mode: row.effective_mode,
		effective_reviewer: row.effective_reviewer,
		config_file: catalog.native.config_file().into(),
		can_update: catalog.can_update,
		last_edit: catalog.shared.as_ref().map(shared::project).map(Box::new),
	};
	let review = Review {
		state,
		native,
		guard: Some(SettingsGuard::Saved(catalog.guard)),
		prior: catalog.prior,
		scope: catalog.scope,
	};
	submit(
		store,
		&source,
		catalog.source,
		review,
		EditRequest {
			event: None,
			review: selection.review,
			edit: selection.edit,
			attempt_id: selection.attempt_id,
		},
	)
	.await
}
