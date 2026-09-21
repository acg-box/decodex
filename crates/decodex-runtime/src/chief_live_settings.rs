//! Source-bound live reviewer inspection and durable, non-replayed publication.
use crate::chief_usage_estimate::Source;
use decodex_codex::app_server_client::{ClientError, LiveReviewer, LiveSettingsOutcome};
use decodex_database::{ChiefLiveReviewerAttempt, SqliteStore};
use decodex_protocol::{ChiefAppReviewer, ChiefLiveReviewerState as State};
use serde_json::json;
use sha2::{Digest as _, Sha256};

struct Inspection {
	state: State,
	previous_id: Option<i64>,
}

async fn inspect(store: &SqliteStore, source: &Source) -> Option<Inspection> {
	let key = &source.key;
	if !store
		.chief_thread_is_owned(
			key.work.clone(),
			key.thread.clone(),
			Some(key.generation.as_str().into()),
		)
		.await
		.ok()?
	{
		return None;
	}
	let work = store.get_chief_work_item(key.work.clone()).await.ok()?;
	if work.codex_thread_id.as_deref() != Some(&key.thread)
		|| work.dispatch_state != decodex_database::ChiefDispatchState::Running
	{
		return None;
	}
	let turn = work.active_turn_id?;
	let prior = store
		.chief_live_reviewer_receipt(key.work.clone(), key.thread.clone(), turn.clone())
		.await
		.ok()?;
	let previous_id = prior.as_ref().map(|p| p.id);
	let can_update = !prior.as_ref().is_some_and(|p| {
		p.outcome == "reserved" && p.generation_id.as_deref() == Some(key.generation.as_str())
	});
	let last_reviewer =
		prior.as_ref().map(|p| serde_json::from_value(json!(p.reviewer))).transpose().ok()?;
	let last_outcome =
		prior.as_ref().map(|p| serde_json::from_value(json!(p.outcome))).transpose().ok()?;
	let facts = json!([
		key.generation.as_str(),
		key.account.as_str(),
		key.revision,
		key.history_revision,
		key.work,
		key.thread,
		turn,
		previous_id,
		last_outcome
	]);
	let token: String =
		Sha256::digest(facts.to_string().as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
	Some(Inspection {
		previous_id,
		state: State::Available {
			thread_id: decodex_protocol::EntityId::new(key.thread.clone()).ok()?,
			turn_id: decodex_protocol::EntityId::new(turn).ok()?,
			review_token: decodex_protocol::WireText::new(token).ok()?,
			can_update,
			last_reviewer,
			last_outcome,
		},
	})
}

pub(crate) async fn read<F, Fut>(store: &SqliteStore, source: F) -> State
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	let Some(before) = source().await else {
		return State::Unavailable;
	};
	let result = inspect(store, &before).await;
	if source().await.is_none_or(|after| after.key != before.key) {
		return State::Unavailable;
	}
	result.map_or(State::Unavailable, |v| v.state)
}

pub(crate) async fn write<F, Fut>(
	store: &SqliteStore,
	source: F,
	turn: &str,
	review: &str,
	reviewer: ChiefAppReviewer,
	attempt: &str,
) -> Result<(), crate::chief_host::ChiefHostError>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	use crate::chief_host::ChiefHostError::{Rejected, Unknown};
	let before = source().await.ok_or(Rejected("Live task source is unavailable."))?;
	let inspected = inspect(store, &before)
		.await
		.ok_or(Rejected("Refresh the live task before changing its reviewer."))?;
	let State::Available { turn_id, review_token, can_update, .. } = &inspected.state else {
		return Err(Rejected("Live task is unavailable."));
	};
	if turn_id.as_str() != turn || review_token.as_str() != review || !can_update {
		return Err(Rejected("The reviewed turn or operation state changed. Refresh it."));
	}
	let guard = before
		.client
		.history_guard(before.key.history_revision)
		.ok_or(Rejected("Native history changed. Refresh the task."))?;
	let (native, value) = match reviewer {
		ChiefAppReviewer::User => (LiveReviewer::User, "user"),
		ChiefAppReviewer::AutoReview => (LiveReviewer::AutoReview, "auto_review"),
	};
	if source().await.is_none_or(|after| after.key != before.key) {
		return Err(Rejected("The task source changed before dispatch."));
	}
	let id = store
		.reserve_chief_live_reviewer(ChiefLiveReviewerAttempt {
			work_id: before.key.work.clone(),
			thread_id: before.key.thread.clone(),
			turn_id: turn.into(),
			generation_id: Some(before.key.generation.as_str().into()),
			review_token: review.into(),
			reviewer: value.into(),
			attempt_id: attempt.into(),
			previous_id: inspected.previous_id,
		})
		.await
		.map_err(|_| {
			Unknown(
				"The operation could not be reserved. Refresh its receipt before further action.",
			)
		})?
		.ok_or(Rejected(
			"This review was already used or another operation is pending. Refresh the task.",
		))?;
	let result = if source().await.is_none_or(|after| after.key != before.key) {
		Err(ClientError::StaleHistory)
	} else {
		before.client.update_live_reviewer(&before.key.thread, turn, native, guard).await
	};
	let changed = source().await.is_none_or(|after| after.key != before.key);
	let outcome = if changed {
		"unknown"
	} else {
		match result {
			Ok(LiveSettingsOutcome::Applied) => "applied",
			Ok(LiveSettingsOutcome::TargetUnavailable) => "target_unavailable",
			Err(ClientError::StaleHistory) => "rejected",
			Err(ClientError::Remote(ref e)) if matches!(e.code, -32602..=-32600) => "rejected",
			_ => "unknown",
		}
	};
	if !store.finish_chief_live_reviewer(id, attempt.into(), outcome.into()).await.unwrap_or(false)
	{
		return Err(Unknown("The operation result could not be saved. It will not be retried."));
	}
	match outcome {
		"applied" => Ok(()),
		"target_unavailable" =>
			Err(Rejected("The reviewed turn is no longer active. No replacement was selected.")),
		"rejected" => Err(Rejected(
			"Native policy or source changes rejected this edit. Existing approvals are unchanged.",
		)),
		_ => Err(Unknown(
			"Reviewer publication is unconfirmed. It will not be retried automatically.",
		)),
	}
}
