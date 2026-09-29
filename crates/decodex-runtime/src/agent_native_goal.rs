//! Read and edit the native goal without creating a second persistent goal owner.
use crate::agent_usage_estimate::Source;
use decodex_codex::app_server_client::ClientError;
use decodex_database::SqliteStore;
use decodex_protocol::AgentNativeGoalResult as Result;

pub(crate) async fn read<F, Fut>(store: &SqliteStore, source: F, thread: &str) -> Result
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	let Some(before) = source().await else { return Result::Unavailable };
	let operation = async {
		if thread != before.key.thread {
			let owner =
				crate::agent::native_subagents::request_owner(store, &before.client, thread)
					.await
					.ok()?;
			if owner.id != before.key.work {
				return None;
			}
		}
		Some(before.client.thread_goal(thread).await)
	};
	let response = tokio::time::timeout(std::time::Duration::from_secs(30), operation).await;
	if source().await.is_none_or(|after| after.key != before.key) {
		return Result::Unavailable;
	}
	match response {
		Ok(Some(Ok(goal))) => {
			let review_token =
				decodex_protocol::WireText::new(review_token(&before, thread, goal.as_ref())).ok();
			let goal = goal
				.map(|goal| {
					let sensitive = decodex_core::contains_credential_material(&goal.objective);
					let end = goal.objective.floor_char_boundary(goal.objective.len().min(8192));
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
			let Some(goal) = goal else { return Result::Unavailable };
			let (Ok(work_id), Ok(thread_id)) = (
				decodex_protocol::EntityId::new(before.key.work),
				decodex_protocol::EntityId::new(thread.to_owned()),
			) else {
				return Result::Unavailable;
			};
			let Some(observed_at_micros) = std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.ok()
				.and_then(|t| i64::try_from(t.as_micros()).ok())
			else {
				return Result::Unavailable;
			};
			Result::Available { work_id, thread_id, observed_at_micros, review_token, goal }
		},
		Ok(Some(Err(ClientError::Remote(error)))) if error.code == -32601 => Result::Unsupported,
		Ok(Some(Err(ClientError::Remote(error))))
			if error.code == -32600 && error.message == "goals feature is disabled" =>
			Result::Disabled,
		_ => Result::Unavailable,
	}
}

#[cfg(test)]
#[path = "agent_native_goal_tests.rs"]
mod tests;

fn review_token(
	source: &Source,
	thread: &str,
	goal: Option<&decodex_codex::app_server_client::NativeThreadGoal>,
) -> String {
	use sha2::{Digest as _, Sha256};
	let identity = serde_json::json!([
		format!("{:?}", source.key),
		source.client.connection_identity(),
		thread,
		goal.map(|goal| (&goal.objective, &goal.status, goal.token_budget, goal.created_at))
	]);
	Sha256::digest(identity.to_string().as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) async fn write<F, Fut>(
	store: &SqliteStore,
	source: F,
	thread: &str,
	expected: &str,
	edit: &decodex_protocol::AgentGoalEdit,
) -> std::result::Result<(), crate::agent_host::AgentHostError>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	use crate::agent_host::AgentHostError::{Rejected, Unknown};
	use decodex_codex::app_server_client::{NativeGoalUpdate, NativeThreadGoalStatus};
	use decodex_protocol::{AgentGoalBudgetEdit as Budget, AgentNativeGoalStatus as Status};
	if edit.objective.as_ref().is_some_and(|text| text.trim().is_empty() || text.len() > 64 * 1024)
		|| matches!(edit.budget,Budget::Set(n) if n<=0)
		|| edit.status.as_ref().is_some_and(|status| {
			!matches!(status, Status::Active | Status::Paused | Status::Complete)
		})
		|| (edit.objective.is_none()
			&& edit.status.is_none()
			&& matches!(edit.budget, Budget::Keep))
	{
		return Err(Rejected("Enter an objective or an explicit goal change."));
	}
	let before = source().await.ok_or(Rejected("The goal source is unavailable."))?;
	if thread != before.key.thread {
		let owner = crate::agent::native_subagents::request_owner(store, &before.client, thread)
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
			Status::Active => NativeThreadGoalStatus::Active,
			Status::Paused => NativeThreadGoalStatus::Paused,
			_ => NativeThreadGoalStatus::Complete,
		}),
		token_budget: match edit.budget {
			Budget::Keep => None,
			Budget::Reset => Some(None),
			Budget::Set(n) => Some(Some(n)),
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
