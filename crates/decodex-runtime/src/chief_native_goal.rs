//! Read the native goal without creating a second persistent goal owner.
use crate::chief_usage_estimate::Source;
use decodex_codex::app_server_client::ClientError;
use decodex_database::SqliteStore;
use decodex_protocol::ChiefNativeGoalResult as Result;

pub(crate) async fn read<F, Fut>(store: &SqliteStore, source: F, thread: &str) -> Result
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	let Some(before) = source().await else { return Result::Unavailable };
	let operation = async {
		if thread != before.key.thread {
			let owner =
				crate::chief::native_subagents::request_owner(store, &before.client, thread)
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
			Result::Available { work_id, thread_id, observed_at_micros, goal }
		},
		Ok(Some(Err(ClientError::Remote(error)))) if error.code == -32601 => Result::Unsupported,
		Ok(Some(Err(ClientError::Remote(error))))
			if error.code == -32600 && error.message == "goals feature is disabled" =>
			Result::Disabled,
		_ => Result::Unavailable,
	}
}

#[cfg(test)]
#[path = "chief_native_goal_tests.rs"]
mod tests;
