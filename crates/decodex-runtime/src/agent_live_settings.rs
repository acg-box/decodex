//! Source-bound live settings inspection and durable, non-replayed publication.
use crate::agent_usage_estimate::Source;
use decodex_codex::app_server_client::{
	ClientError, LiveModelUpdate, LiveReviewer, LiveSettingsOutcome,
};
use decodex_database::{AgentLiveSettingsAttempt, AgentLiveSettingsEdit, SqliteStore};
use decodex_protocol::{AgentLiveReviewerState as State, AgentReviewer};
use serde_json::json;
use sha2::{Digest as _, Sha256};

pub(crate) enum LiveEdit {
	Reviewer(AgentReviewer),
	Model {
		model: decodex_protocol::ConversationModel,
		effort: decodex_protocol::ConversationReasoningEffort,
	},
}

impl From<AgentReviewer> for LiveEdit {
	fn from(reviewer: AgentReviewer) -> Self {
		Self::Reviewer(reviewer)
	}
}

struct Inspection {
	state: State,
	previous_id: Option<i64>,
}

async fn inspect(store: &SqliteStore, source: &Source) -> Option<Inspection> {
	let key = &source.key;
	if !store
		.agent_thread_is_owned(
			key.work.clone(),
			key.thread.clone(),
			Some(key.generation.as_str().into()),
		)
		.await
		.ok()?
	{
		return None;
	}
	let work = store.get_agent_work_item(key.work.clone()).await.ok()?;
	if work.codex_thread_id.as_deref() != Some(&key.thread)
		|| work.dispatch_state != decodex_database::AgentDispatchState::Running
	{
		return None;
	}
	let turn = work.active_turn_id?;
	let prior = store
		.agent_live_settings_receipt(key.work.clone(), key.thread.clone(), turn.clone())
		.await
		.ok()?;
	let previous_id = prior.as_ref().map(|p| p.id);
	let can_update = !prior.as_ref().is_some_and(|p| {
		p.outcome == "reserved" && p.generation_id.as_deref() == Some(key.generation.as_str())
	});
	let last_reviewer = prior
		.as_ref()
		.and_then(|p| match &p.edit {
			AgentLiveSettingsEdit::Reviewer { reviewer } =>
				Some(serde_json::from_value(json!(reviewer))),
			AgentLiveSettingsEdit::Model { .. } => None,
		})
		.transpose()
		.ok()?;
	let last_model = prior
		.as_ref()
		.and_then(|p| match &p.edit {
			AgentLiveSettingsEdit::Model { model, effort } => Some((model, effort)),
			_ => None,
		})
		.and_then(|(model, effort)| {
			Some(decodex_protocol::AgentLiveModelSelection {
				model: decodex_protocol::ConversationModel::new(model.clone()).ok()?,
				effort: serde_json::from_value(json!(effort)).ok()?,
			})
		});
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
			last_model,
			model_choices: None,
			last_outcome,
		},
	})
}

#[cfg(test)]
pub(crate) async fn read<F, Fut>(store: &SqliteStore, source: F) -> State
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	read_options(store, false, source).await
}

pub(crate) async fn read_options<F, Fut>(
	store: &SqliteStore,
	include_models: bool,
	source: F,
) -> State
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	let Some(before) = source().await else {
		return State::Unavailable;
	};
	let mut result = inspect(store, &before).await;
	if include_models && let Some(ref mut inspected) = result {
		let choices = tokio::time::timeout(std::time::Duration::from_secs(16), async {
			if crate::agent_capabilities::feature_enabled(
				&before.client,
				"step_model_switching",
				Some(&before.key.thread),
			)
			.await
				!= Some(true)
			{
				return None;
			}
			match crate::agent_capabilities::read(&before.client).await {
				decodex_protocol::AgentCapabilitiesResult::Available { models, .. } => Some(
					models
						.into_iter()
						.filter(|m| m.model.as_str() != "gpt-reserve" && !m.efforts.is_empty())
						.collect(),
				),
				_ => None,
			}
		})
		.await
		.ok()
		.flatten();
		if let State::Available { model_choices, .. } = &mut inspected.state {
			*model_choices = choices;
		}
	}
	if source().await.is_none_or(|after| after.key != before.key) {
		return State::Unavailable;
	}
	if include_models {
		let current = inspect(store, &before).await;
		let unchanged = matches!((&result,&current), (Some(a),Some(b)) if matches!((&a.state,&b.state),
            (State::Available { review_token:a,.. },State::Available { review_token:b,.. }) if a==b));
		if !unchanged {
			return State::Unavailable;
		}
	}
	result.map_or(State::Unavailable, |v| v.state)
}

pub(crate) async fn write<F, Fut>(
	store: &SqliteStore,
	source: F,
	turn: &str,
	review: &str,
	edit: LiveEdit,
	attempt: &str,
) -> Result<(), crate::agent_host::AgentHostError>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	use crate::agent_host::AgentHostError::{Rejected, Unknown};
	let before = source().await.ok_or(Rejected("Live task source is unavailable."))?;
	let inspected = inspect(store, &before)
		.await
		.ok_or(Rejected("Refresh the live task before changing its settings."))?;
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
	let model_update = prepare_model_update(&before, turn, &edit).await?;
	let stored_edit = match &edit {
		LiveEdit::Reviewer(reviewer) => AgentLiveSettingsEdit::Reviewer {
			reviewer: match reviewer {
				AgentReviewer::User => "user",
				AgentReviewer::AutoReview => "auto_review",
			}
			.into(),
		},
		LiveEdit::Model { model, effort } => AgentLiveSettingsEdit::Model {
			model: model.as_str().into(),
			effort: serde_json::to_value(effort)
				.ok()
				.and_then(|v| v.as_str().map(str::to_owned))
				.ok_or(Rejected("Invalid reasoning effort."))?,
		},
	};
	if source().await.is_none_or(|after| after.key != before.key) {
		return Err(Rejected("The task source changed before dispatch."));
	}
	let id = store
		.reserve_agent_live_settings(AgentLiveSettingsAttempt {
			work_id: before.key.work.clone(),
			thread_id: before.key.thread.clone(),
			turn_id: turn.into(),
			generation_id: Some(before.key.generation.as_str().into()),
			review_token: review.into(),
			edit: stored_edit,
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
		match edit {
			LiveEdit::Reviewer(reviewer) =>
				before
					.client
					.update_live_reviewer(
						&before.key.thread,
						turn,
						match reviewer {
							AgentReviewer::User => LiveReviewer::User,
							AgentReviewer::AutoReview => LiveReviewer::AutoReview,
						},
						guard,
					)
					.await,
			LiveEdit::Model { .. } =>
				before
					.client
					.update_live_model(model_update.as_ref().expect("validated model edit"), guard)
					.await,
		}
	};
	let changed = source().await.is_none_or(|after| after.key != before.key);
	let known_unsent = matches!(
		&result,
		Err(ClientError::StaleHistory
			| ClientError::RequestTooLarge
			| ClientError::RequestQueueFull)
	);
	let outcome = if known_unsent {
		"rejected"
	} else if changed {
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
	if !store.finish_agent_live_settings(id, attempt.into(), outcome.into()).await.unwrap_or(false)
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
			"Settings publication is unconfirmed. It will not be retried automatically.",
		)),
	}
}

async fn prepare_model_update(
	before: &Source,
	turn: &str,
	edit: &LiveEdit,
) -> Result<Option<LiveModelUpdate>, crate::agent_host::AgentHostError> {
	use crate::agent_host::AgentHostError::Rejected;
	if let LiveEdit::Model { model, effort } = &edit {
		let capabilities = crate::agent_capabilities::read(&before.client).await;
		let supported = matches!(capabilities, decodex_protocol::AgentCapabilitiesResult::Available { models, .. }
            if models.iter().any(|entry| entry.model == *model && entry.efforts.contains(effort)));
		let enabled = tokio::time::timeout(
			std::time::Duration::from_secs(8),
			crate::agent_capabilities::feature_enabled(
				&before.client,
				"step_model_switching",
				Some(&before.key.thread),
			),
		)
		.await
		.ok()
		.flatten();
		if !supported || enabled != Some(true) {
			return Err(Rejected(
				"The current native catalog or task feature does not allow this model selection.",
			));
		}
		let effort = serde_json::to_value(effort)
			.ok()
			.and_then(|v| v.as_str().map(str::to_owned))
			.ok_or(Rejected("Invalid reasoning effort."))?;
		Ok(Some(
			LiveModelUpdate::new(&before.key.thread, turn, model.as_str(), &effort)
				.map_err(|_| Rejected("Invalid live model selection."))?,
		))
	} else {
		Ok(None)
	}
}
