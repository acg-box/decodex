//! Shared hook edits with exact native review, durable receipts and no replay.
use crate::chief_usage_estimate::Source;
use decodex_codex::app_server_client::{
	ClientError, HistoryGuard, HookSettingsChange, HookSettingsReview, HookSettingsWrite,
};
use decodex_database::{
	ChiefHookAttempt, ChiefHookObservation, ChiefHookOwner, ChiefHookReceipt, SqliteStore,
};
use decodex_protocol::{
	ChiefHookChange as Change, ChiefHookDto, ChiefHookEditReceipt, ChiefHookSettingsState as State,
	EntityId, WireText,
};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
struct Review {
	native: HookSettingsReview,
	guard: HistoryGuard,
	scope: String,
	prior: Option<ChiefHookReceipt>,
	state: State,
}
fn digest(value: &str) -> String {
	Sha256::digest(value.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}
fn owner(source: &Source) -> ChiefHookOwner {
	let k = &source.key;
	ChiefHookOwner {
		work: k.work.clone(),
		thread: k.thread.clone(),
		generation: k.generation.as_str().into(),
		account: k.account.as_str().into(),
	}
}
fn pending(receipt: &ChiefHookReceipt) -> bool {
	matches!(receipt.state.as_str(), "reserved" | "unknown")
}
fn field(change: Change) -> &'static str {
	match change {
		Change::Trust => "trusted_hash",
		Change::Enabled(_) => "enabled",
	}
}
fn raw(native: &HookSettingsReview, key: &str, field: &str) -> Option<Value> {
	native.saved_hook(key).and_then(|s| s.get(field)).cloned()
}
async fn inspect<F, Fut>(store: &SqliteStore, source: &F) -> Option<(Source, Review)>
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
	let native = before.client.hook_settings(cwd).await.ok()?;
	let scope = digest(native.config_file());
	if !guard.is_live() || source().await.is_none_or(|after| after.key != before.key) {
		return None;
	}
	if let Some(prior) = store.chief_hook_receipt(scope.clone()).await.ok()?
		&& pending(&prior)
	{
		store
			.observe_chief_hook_setting(
				prior.id,
				ChiefHookObservation {
					owner: owner(&before),
					scope: scope.clone(),
					hook: prior.attempt.hook.clone(),
					field: prior.attempt.field.clone(),
					value: raw(&native, &prior.attempt.hook, &prior.attempt.field),
					config_version: native.config_version().into(),
				},
			)
			.await
			.ok()?;
	}
	let prior = store.chief_hook_receipt(scope.clone()).await.ok()?;
	let work = store.get_chief_work_item(k.work.clone()).await.ok()?;
	let can_update = work.status != decodex_database::ChiefWorkStatus::Resolved
		&& !prior.as_ref().is_some_and(pending);
	// Consent follows native config and content, not a temporary task-settings guard lifetime.
	let token = digest(
		&json!([
			k.work,
			k.thread,
			k.generation.as_str(),
			k.account.as_str(),
			k.revision,
			k.history_revision,
			native.config_file(),
			native.config_version(),
			native.inventory,
			prior.as_ref().map(|r| (r.id, &r.state)),
			can_update
		])
		.to_string(),
	);
	let hooks = native.inventory["hooks"]
		.as_array()?
		.iter()
		.map(|h| project_hook(h, &native))
		.collect::<Option<Vec<_>>>()?;
	let notices = ["warnings", "errors"]
		.into_iter()
		.flat_map(|field| native.inventory[field].as_array().into_iter().flatten())
		.map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()))
		.collect();
	let last_edit = match &prior {
		Some(r) => Some(ChiefHookEditReceipt {
			outcome: r.state.clone(),
			hook: WireText::new(r.attempt.hook.clone()).ok()?,
			work_id: EntityId::new(r.attempt.owner.work.clone()).ok()?,
			account_id: EntityId::new(r.attempt.owner.account.clone()).ok()?,
		}),
		None => None,
	};
	let state = State::Available {
		work_id: EntityId::new(k.work.clone()).ok()?,
		thread_id: EntityId::new(k.thread.clone()).ok()?,
		review_token: WireText::new(token).ok()?,
		config_file: WireText::new(native.config_file()).ok()?,
		hooks,
		notices,
		can_update,
		last_edit: last_edit.map(Box::new),
	};
	if !guard.is_live() || source().await.is_none_or(|after| after.key != before.key) {
		return None;
	}
	Some((before, Review { native, guard, scope, prior, state }))
}
fn project_hook(h: &Value, native: &HookSettingsReview) -> Option<ChiefHookDto> {
	let key = h["key"].as_str()?;
	Some(ChiefHookDto {
		key: WireText::new(key).ok()?,
		trust_status: h["trustStatus"].as_str()?.into(),
		enabled: h["enabled"].as_bool()?,
		managed: h["isManaged"].as_bool()?,
		current_hash: WireText::new(h["currentHash"].as_str()?).ok()?,
		saved_enabled: raw(native, key, "enabled").and_then(|v| v.as_bool()),
		saved_hash: raw(native, key, "trusted_hash").and_then(|v| v.as_str().map(str::to_owned)),
		details: serde_json::to_string_pretty(h).ok()?,
	})
}
pub(crate) async fn read<F, Fut>(store: &SqliteStore, source: F) -> State
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	tokio::time::timeout(std::time::Duration::from_secs(35), inspect(store, &source))
		.await
		.ok()
		.flatten()
		.map_or(State::Unavailable, |(_, review)| review.state)
}
pub(crate) struct Selection<'a> {
	pub thread: &'a str,
	pub review: &'a str,
	pub hook: &'a str,
	pub change: Change,
	pub attempt_id: &'a str,
}
pub(crate) async fn write<F, Fut>(
	store: &SqliteStore,
	source: F,
	selection: Selection<'_>,
) -> Result<(), crate::chief_host::ChiefHostError>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	use crate::chief_host::ChiefHostError::{Rejected, Unknown};
	let (before, review) =
		tokio::time::timeout(std::time::Duration::from_secs(35), inspect(store, &source))
			.await
			.ok()
			.flatten()
			.ok_or(Rejected("Current hook settings are unavailable."))?;
	let State::Available { review_token, can_update: true, .. } = &review.state else {
		return Err(Rejected("A shared hook edit remains unconfirmed."));
	};
	if before.key.thread != selection.thread || review_token.as_str() != selection.review {
		return Err(Rejected("The reviewed hook source changed. Refresh hook settings."));
	}
	let change = match selection.change {
		Change::Trust => HookSettingsChange::Trust,
		Change::Enabled(enabled) => HookSettingsChange::Enabled(enabled),
	};
	let params = review
		.native
		.change(selection.hook, change)
		.map_err(|_| Rejected("The selected hook cannot be changed."))?;
	let field = field(selection.change);
	let value = params["edits"][0]["value"].clone();
	let previous_value = raw(&review.native, selection.hook, field);
	if previous_value.as_ref() == Some(&value) {
		return Err(Rejected("The requested override is already saved."));
	}
	if !review.guard.is_live() || source().await.is_none_or(|after| after.key != before.key) {
		return Err(Rejected("The native source changed before the write."));
	}
	let attempt = ChiefHookAttempt {
		owner: owner(&before),
		scope: review.scope,
		hook: selection.hook.into(),
		field: field.into(),
		value,
		previous_value,
		config_version: review.native.config_version().into(),
		review_token: selection.review.into(),
		attempt_id: selection.attempt_id.into(),
		previous_id: review.prior.map(|r| r.id),
	};
	let id = store
		.reserve_chief_hook_setting(attempt)
		.await
		.map_err(|_| Unknown("The hook reservation is unconfirmed. Refresh saved state."))?
		.ok_or(Rejected("This review was consumed or another shared edit is pending."))?;
	let response =
		if !review.guard.is_live() || source().await.is_none_or(|after| after.key != before.key) {
			Err(ClientError::StaleHistory)
		} else {
			before.client.write_hook_settings(params, review.guard).await
		};
	let state = match response {
		Ok(HookSettingsWrite::Saved) => "saved",
		Ok(HookSettingsWrite::Overridden) => "overridden",
		Err(
			ClientError::StaleHistory
			| ClientError::RequestTooLarge
			| ClientError::RequestQueueFull,
		) => "rejected",
		Err(ClientError::Remote(ref error)) if matches!(error.code, -32602..=-32600) => "rejected",
		_ => "unknown",
	};
	if !store
		.finish_chief_hook_setting(id, selection.attempt_id.into(), state.into())
		.await
		.unwrap_or(false)
	{
		return Err(Unknown("The hook result could not be saved. It will not be replayed."));
	}
	match state {
		"saved" | "overridden" => Ok(()),
		"rejected" => Err(Rejected("Native policy or a changed config rejected this edit.")),
		_ => Err(Unknown("The hook write is unconfirmed. It will not be retried.")),
	}
}
