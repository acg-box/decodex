//! Native-owned goal state queried through the currently admitted process.
use super::ChiefHost;
use decodex_codex::app_server_client::ClientError;
use decodex_protocol::{ChiefGoalResult, ChiefNativeGoal};
use serde_json::{Value, json};

impl ChiefHost {
	pub(crate) async fn goal_state(&self, work: &str) -> ChiefGoalResult {
		let Some(source) = self.runtime_source().await else { return ChiefGoalResult::Unavailable };
		let Some((generation, client)) = self.runtime.chief_catalog_client() else {
			return ChiefGoalResult::Unavailable;
		};
		let Ok(owner) = self.store.get_chief_work_item(work.into()).await else {
			return ChiefGoalResult::Unavailable;
		};
		let Some(thread) = owner.codex_thread_id else { return ChiefGoalResult::Unbound };
		let owned = || {
			self.store.chief_thread_is_owned(
				work.into(),
				thread.clone(),
				Some(generation.as_str().into()),
			)
		};
		if !owned().await.unwrap_or(false) {
			return ChiefGoalResult::Unavailable;
		}
		let result = tokio::time::timeout(
			std::time::Duration::from_secs(8),
			client.request("thread/goal/get", json!({"threadId":thread})),
		)
		.await;
		if !owned().await.unwrap_or(false) || self.runtime_source().await.as_ref() != Some(&source)
		{
			return ChiefGoalResult::Unavailable;
		}
		match result {
			Ok(Ok(value)) => match project(&value, &thread) {
				Ok(goal) => ChiefGoalResult::Available { source, thread_id: thread, goal },
				Err(()) => ChiefGoalResult::Unavailable,
			},
			Ok(Err(ClientError::Remote(error)))
				if error.code == -32601
					|| (error.code == -32600 && error.message == "goals feature is disabled") =>
				ChiefGoalResult::Unsupported,
			_ => ChiefGoalResult::Unavailable,
		}
	}
}

fn project(value: &Value, thread: &str) -> Result<Option<ChiefNativeGoal>, ()> {
	let goal = value.get("goal").ok_or(())?;
	if goal.is_null() {
		return Ok(None);
	}
	let goal: ChiefNativeGoal = serde_json::from_value(goal.clone()).map_err(|_| ())?;
	if !goal.is_valid() || goal.thread_id != thread {
		return Err(());
	}
	Ok(Some(goal))
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn native_goal_read_requires_exact_identity_and_complete_counters() {
		let goal = json!({"threadId":"thread","objective":"Finish task","status":"active","tokenBudget":100,"tokensUsed":12,"timeUsedSeconds":3,"createdAt":1,"updatedAt":2});
		assert_eq!(project(&json!({"goal":goal}), "thread").unwrap().unwrap().tokens_used, 12);
		assert!(project(&json!({"goal":goal}), "foreign").is_err());
		assert_eq!(project(&json!({"goal":null}), "thread"), Ok(None));
		assert!(project(&json!({}), "thread").is_err());
		for (field, value) in [
			("tokensUsed", json!(-1)),
			("timeUsedSeconds", Value::Null),
			("updatedAt", json!(0)),
			("tokenBudget", json!(0)),
			("objective", json!("")),
		] {
			let mut invalid = goal.clone();
			invalid[field] = value;
			assert!(project(&json!({"goal":invalid}), "thread").is_err(), "{field}");
		}
	}
}
