//! Read and edit the native goal without creating a second persistent goal owner.
use std::{
	future::Future,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::time;

use crate::{
	agent::native_subagents,
	agent_host::AgentHostError::{self, Rejected, Unknown},
	agent_usage_estimate::Source,
};
use decodex_codex::app_server_client::{
	ClientError, NativeGoalUpdate, NativeThreadGoal, NativeThreadGoalStatus,
};
use decodex_database::SqliteStore;
use decodex_protocol::{AgentGoalEdit, AgentNativeGoalResult, EntityId, WireText};

pub(crate) async fn read<F, Fut>(
	store: &SqliteStore,
	source: F,
	thread: &str,
) -> AgentNativeGoalResult
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	let Some(before) = source().await else { return AgentNativeGoalResult::Unavailable };
	let operation = async {
		if thread != before.key.thread {
			let owner =
				native_subagents::request_owner(store, &before.client, thread).await.ok()?;

			if owner.id != before.key.work {
				return None;
			}
		}

		Some(before.client.thread_goal(thread).await)
	};
	let response = time::timeout(Duration::from_secs(30), operation).await;

	if source().await.is_none_or(|after| after.key != before.key) {
		return AgentNativeGoalResult::Unavailable;
	}

	match response {
		Ok(Some(Ok(goal))) => {
			let review_token = WireText::new(review_token(&before, thread, goal.as_ref())).ok();
			let goal = goal
				.map(|goal| {
					let sensitive = decodex_core::contains_credential_material(&goal.objective);
					let end = goal
						.objective
						.char_indices()
						.nth(4_000)
						.map_or(goal.objective.len(), |(index, _)| index);
					let truncated = sensitive || end < goal.objective.len();
					let mut value = serde_json::to_value(&goal).ok()?;

					value["objective"] = serde_json::json!(if sensitive {
						"[Private content omitted]"
					} else {
						&goal.objective[..end]
					});
					value["objectiveTruncated"] = serde_json::json!(truncated);

					serde_json::from_value(value).ok()
				})
				.map_or(Some(None), |goal| goal.map(Some));
			let Some(goal) = goal else { return AgentNativeGoalResult::Unavailable };
			let (Ok(work_id), Ok(thread_id)) =
				(EntityId::new(before.key.work), EntityId::new(thread.to_owned()))
			else {
				return AgentNativeGoalResult::Unavailable;
			};
			let Some(observed_at_micros) = SystemTime::now()
				.duration_since(UNIX_EPOCH)
				.ok()
				.and_then(|t| i64::try_from(t.as_micros()).ok())
			else {
				return AgentNativeGoalResult::Unavailable;
			};

			AgentNativeGoalResult::Available {
				work_id,
				thread_id,
				observed_at_micros,
				review_token,
				goal,
			}
		},
		Ok(Some(Err(ClientError::Remote(error)))) if error.code == -32_601 =>
			AgentNativeGoalResult::Unsupported,
		Ok(Some(Err(ClientError::Remote(error))))
			if error.code == -32_600 && error.message == "goals feature is disabled" =>
			AgentNativeGoalResult::Disabled,
		_ => AgentNativeGoalResult::Unavailable,
	}
}

#[cfg(test)]
#[path = "agent_native_goal_tests.rs"]
mod tests;

pub(crate) async fn write<F, Fut>(
	store: &SqliteStore,
	source: F,
	thread: &str,
	expected: &str,
	edit: &AgentGoalEdit,
) -> std::result::Result<(), AgentHostError>
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	use decodex_protocol::{AgentGoalBudgetEdit, AgentNativeGoalStatus};

	if edit.objective.as_ref().is_some_and(|text| text.trim().is_empty() || text.len() > 64 * 1_024)
		|| matches!(edit.budget,AgentGoalBudgetEdit::Set(n) if n<=0)
		|| edit.status.as_ref().is_some_and(|status| {
			!matches!(
				status,
				AgentNativeGoalStatus::Active
					| AgentNativeGoalStatus::Paused
					| AgentNativeGoalStatus::Complete
			)
		})
		|| (edit.objective.is_none()
			&& edit.status.is_none()
			&& matches!(edit.budget, AgentGoalBudgetEdit::Keep))
	{
		return Err(Rejected("Enter an objective or an explicit goal change."));
	}

	let before = source().await.ok_or(Rejected("The goal source is unavailable."))?;

	if thread != before.key.thread {
		let owner = native_subagents::request_owner(store, &before.client, thread)
			.await
			.map_err(|_| Rejected("The native goal is not owned by this task."))?;

		if owner.id != before.key.work {
			return Err(Rejected("The native goal is not owned by this task."));
		}
	}

	let goal = before
		.client
		.thread_goal(thread)
		.await
		.map_err(|_| Rejected("Read the native goal before editing."))?;

	if review_token(&before, thread, goal.as_ref()) != expected {
		return Err(Rejected("The goal changed. Read it again before saving."));
	}
	if goal.is_none() && (edit.objective.is_none() || edit.status.is_none()) {
		return Err(Rejected(
			"A new goal needs an objective and an explicit start or pause choice.",
		));
	}

	let guard = before
		.client
		.history_guard(before.key.history_revision)
		.ok_or(Rejected("The goal source changed."))?;
	let objective = match &edit.objective {
		Some(text) => Some(
			before.client.materialize_goal_objective(text, guard.clone()).await.map_err(|_| {
				Unknown(
					"The objective attachment could not be confirmed. Read the goal before retrying.",
				)
			})?,
		),
		None => None,
	};

	if source().await.is_none_or(|after| after.key != before.key) {
		return Err(Rejected("The goal source changed."));
	}

	let update = NativeGoalUpdate {
		objective,
		status: edit.status.as_ref().map(|status| match status {
			AgentNativeGoalStatus::Active => NativeThreadGoalStatus::Active,
			AgentNativeGoalStatus::Paused => NativeThreadGoalStatus::Paused,
			_ => NativeThreadGoalStatus::Complete,
		}),
		token_budget: match edit.budget {
			AgentGoalBudgetEdit::Keep => None,
			AgentGoalBudgetEdit::Reset => Some(None),
			AgentGoalBudgetEdit::Set(n) => Some(Some(n)),
		},
	};

	before.client.update_thread_goal(thread, &update, guard).await.map_err(
		|error| match error {
			ClientError::Remote(_) | ClientError::InvalidFrame | ClientError::StaleHistory =>
				Rejected("Native policy or a changed goal rejected the edit. Read the goal again."),
			_ => Unknown("The goal edit is unconfirmed. Read the goal before retrying."),
		},
	)?;

	Ok(())
}

fn review_token(source: &Source, thread: &str, goal: Option<&NativeThreadGoal>) -> String {
	use sha2::{Digest as _, Sha256};

	let identity = serde_json::json!([
		format!("{:?}", source.key),
		source.client.connection_identity(),
		thread,
		goal.map(|goal| (&goal.objective, &goal.status, goal.token_budget, goal.created_at))
	]);

	Sha256::digest(identity.to_string().as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}
