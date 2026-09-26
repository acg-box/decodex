//! Review, reserve and dispatch widget tool calls through the existing native owner.
use crate::{chief_host::ChiefHostError, chief_usage_estimate::Source};
use decodex_codex::app_server_client::{ClientError, NativeAppUiToolReview};
use decodex_database::{ChiefAppUiCallAttempt, SqliteStore};
use decodex_protocol::{
	ChiefAppUiCall, ChiefAppUiCallReview as Review, EntityId, MAX_CHIEF_APP_UI_CALL_BYTES, WireText,
};
use serde_json::json;
use sha2::{Digest, Sha256};

fn bounded(call: &ChiefAppUiCall) -> bool {
	call.arguments.is_object()
		&& call.source_fingerprint.as_str().len() == 64
		&& call.source_fingerprint.as_str().bytes().all(|b| b.is_ascii_hexdigit())
		&& serde_json::to_vec(call).is_ok_and(|bytes| bytes.len() <= MAX_CHIEF_APP_UI_CALL_BYTES)
}

async fn inspect(
	store: &SqliteStore,
	source: &Source,
	call: &ChiefAppUiCall,
) -> Option<(NativeAppUiToolReview, EntityId)> {
	if !bounded(call)
		|| source.key.work != call.work_id.as_str()
		|| source.key.thread != call.thread_id.as_str()
		|| crate::chief::timeline::app_ui::source_fingerprint(&source.key)
			!= call.source_fingerprint
		|| !store
			.chief_thread_is_owned(
				source.key.work.clone(),
				source.key.thread.clone(),
				Some(source.key.generation.as_str().into()),
			)
			.await
			.ok()?
	{
		return None;
	}
	let review = source
		.client
		.review_mcp_app_tool(
			call.thread_id.as_str(),
			call.turn_id.as_str(),
			call.item_id.as_str(),
			call.tool.as_str(),
			&call.arguments,
		)
		.await
		.ok()??;
	if !review.guard.is_live() {
		return None;
	}
	let bytes =
		serde_json::to_vec(&json!([call, review.server, review.origin, review.descriptor])).ok()?;
	let token = EntityId::new(
		Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
	)
	.ok()?;
	Some((review, token))
}

pub(crate) async fn read<F, Fut>(store: &SqliteStore, source: F, call: &ChiefAppUiCall) -> Review
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	if serde_json::to_vec(call).map_or(true, |bytes| bytes.len() > MAX_CHIEF_APP_UI_CALL_BYTES) {
		return Review::CapacityExceeded;
	}
	let Some(before) = source().await else { return Review::Unavailable };
	let Some((review, token)) =
		tokio::time::timeout(std::time::Duration::from_secs(30), inspect(store, &before, call))
			.await
			.ok()
			.flatten()
	else {
		return Review::Unavailable;
	};
	let Ok(pending) = store.pending_chief_app_ui_call(call.work_id.as_str().into()).await else {
		return Review::Unavailable;
	};
	if source().await.is_none_or(|after| after.key != before.key) || !review.guard.is_live() {
		return Review::Unavailable;
	}
	let Ok(server) = WireText::new(review.server) else { return Review::Unavailable };
	let title = review.descriptor["title"]
		.as_str()
		.and_then(|title| WireText::new(title).ok())
		.unwrap_or_else(|| call.tool.clone());
	Review::Available {
		request: Box::new(call.clone()),
		review_token: token,
		server,
		title,
		pending_operation: pending.and_then(|p| EntityId::new(p.attempt.attempt_id).ok()),
	}
}

pub(crate) async fn execute<F, Fut>(
	store: &SqliteStore,
	source: F,
	call: &ChiefAppUiCall,
	token: &EntityId,
) -> Result<(), ChiefHostError>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	use ChiefHostError::{Rejected, Unknown};
	// Never re-review or replay a call that already has durable dispatch evidence.
	if store
		.chief_app_ui_call_receipt(call.work_id.as_str().into(), call.operation_id.as_str().into())
		.await
		.map_err(|_| Unknown("App call records could not be read."))?
		.is_some()
	{
		return Err(Rejected("This app call was already submitted. Read its saved result."));
	}
	let before = source().await.ok_or(Rejected("App source is unavailable."))?;
	let (review, current) =
		tokio::time::timeout(std::time::Duration::from_secs(30), inspect(store, &before, call))
			.await
			.ok()
			.flatten()
			.ok_or(Rejected("Refresh the app call review."))?;
	if current != *token
		|| !review.guard.is_live()
		|| source().await.is_none_or(|after| after.key != before.key)
	{
		return Err(Rejected("App source, tool or arguments changed. Review the call again."));
	}
	let attempt = ChiefAppUiCallAttempt {
		owner: crate::chief_config_settings::owner(&before),
		turn: call.turn_id.as_str().into(),
		item: call.item_id.as_str().into(),
		server: review.server.clone(),
		tool: call.tool.as_str().into(),
		arguments: call.arguments.clone(),
		source_fingerprint: call.source_fingerprint.as_str().into(),
		review_token: token.as_str().into(),
		attempt_id: call.operation_id.as_str().into(),
	};
	let id = store
		.reserve_chief_app_ui_call(attempt)
		.await
		.map_err(|_| Unknown("App call reservation is unconfirmed. Read its saved status."))?
		.ok_or(Rejected("This review was used or another app call is unresolved."))?;
	let (state, result, outcome) =
		if !review.guard.is_live() || source().await.is_none_or(|after| after.key != before.key) {
			("unsent", None, Err(Rejected("App source changed before dispatch.")))
		} else {
			match before
				.client
				.call_mcp_app_tool(
					call.thread_id.as_str(),
					&review.server,
					call.tool.as_str(),
					call.arguments.clone(),
					review.guard,
				)
				.await
			{
				Ok(result) => ("completed", Some(result), Ok(())),
				Err(ClientError::StaleHistory) =>
					("unsent", None, Err(Rejected("App source changed before dispatch."))),
				Err(_) => (
					"unknown",
					None,
					Err(Unknown("App call outcome is unknown. It will not be retried.")),
				),
			}
		};
	if !store
		.finish_chief_app_ui_call(id, call.operation_id.as_str().into(), state.into(), result)
		.await
		.map_err(|_| Unknown("App call result could not be saved. Read its durable status."))?
	{
		return Err(Unknown("App call result is not confirmed in the journal."));
	}
	outcome
}
