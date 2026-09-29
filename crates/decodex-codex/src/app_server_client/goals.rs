//! Native goal observations, independent of application-owned coordination goals.
use super::{AppServerClient, ClientError, HistoryGuard};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Native scheduler state; a token limit is distinct from account usage limits.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeThreadGoalStatus {
	/// The native goal can continue work.
	Active,
	/// The native goal is paused.
	Paused,
	/// The native goal needs external progress.
	Blocked,
	/// Account usage prevents progress.
	UsageLimited,
	/// The explicit goal budget prevents progress.
	BudgetLimited,
	/// The native goal has completed.
	Complete,
}

/// One native goal snapshot; counters belong to this goal, not all thread history.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeThreadGoal {
	/// Exact native thread identity.
	pub thread_id: String,
	/// Native objective text.
	pub objective: String,
	/// Native scheduler state.
	pub status: NativeThreadGoalStatus,
	/// Explicit token budget; null means no configured budget.
	pub token_budget: Option<i64>,
	/// Native goal token counter.
	pub tokens_used: i64,
	/// Native goal elapsed time in seconds.
	pub time_used_seconds: i64,
	/// Native creation timestamp in Unix seconds.
	pub created_at: i64,
	/// Native last-update timestamp in Unix seconds.
	pub updated_at: i64,
}

/// One explicit goal edit. Omitted values preserve native state.
#[derive(Clone, Debug, Default)]
pub struct NativeGoalUpdate {
	/// New objective, including multiline text imported from a document.
	pub objective: Option<String>,
	/// Explicit scheduler action; editing text alone does not resume a paused goal.
	pub status: Option<NativeThreadGoalStatus>,
	/// None preserves the budget; Some(None) resets it to native policy; Some(Some(n)) sets it.
	pub token_budget: Option<Option<i64>>,
}
impl NativeGoalUpdate {
	fn params(&self, thread: &str) -> Value {
		let mut params = json!({"threadId":thread});
		if let Some(objective) = &self.objective {
			params["objective"] = json!(objective);
		}
		if let Some(status) = &self.status {
			params["status"] = json!(status);
		}
		if let Some(budget) = self.token_budget {
			params["tokenBudget"] = json!(budget);
		}
		params
	}
}
/// Permit only an explicit native goal edit, not unrelated native configuration.
pub fn is_native_goal_update(params: &Value) -> bool {
	let Some(object) = params.as_object() else { return false };
	(2..=4).contains(&object.len())
		&& object
			.keys()
			.all(|key| matches!(key.as_str(), "threadId" | "objective" | "status" | "tokenBudget"))
		&& params["threadId"].as_str().is_some_and(|id| !id.is_empty() && id.len() <= 512)
		&& params.get("objective").is_none_or(|value| {
			value
				.as_str()
				.is_some_and(|text| !text.trim().is_empty() && text.chars().count() <= 4_000)
		})
		&& params.get("status").is_none_or(|value| {
			value.as_str().is_some_and(|status| matches!(status, "active" | "paused" | "complete"))
		})
		&& params
			.get("tokenBudget")
			.is_none_or(|value| value.is_null() || value.as_i64().is_some_and(|budget| budget > 0))
}

/// Only the fixed native goal attachment layout can cross the retained bridge.
pub fn is_goal_attachment_write(method: &str, params: &Value) -> bool {
	let Some(path) = params["path"].as_str().map(std::path::Path::new) else {
		return false;
	};
	if !path.is_absolute()
		|| path.components().any(|c| matches!(c, std::path::Component::ParentDir))
	{
		return false;
	}
	let directory = if method == "fs/writeFile" {
		if path.file_name().and_then(|s| s.to_str()) != Some("goal-objective.md") {
			return false;
		}
		let Some(parent) = path.parent() else { return false };
		parent
	} else {
		path
	};
	let valid_directory = directory
		.file_name()
		.and_then(|s| s.to_str())
		.is_some_and(|id| decodex_core::AccountOperationId::new(id).is_ok())
		&& directory.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str())
			== Some("attachments");
	valid_directory
		&& match method {
			"fs/createDirectory" =>
				params.as_object().is_some_and(|p| p.len() == 2) && params["recursive"] == true,
			"fs/writeFile" =>
				params.as_object().is_some_and(|p| p.len() == 2)
					&& params["dataBase64"].as_str().is_some_and(|s| s.len() <= 128 * 1024),
			_ => false,
		}
}

impl AppServerClient {
	/// Keep long objective text in the native attachment layout, without truncation.
	pub async fn materialize_goal_objective(
		&self,
		text: &str,
		guard: HistoryGuard,
	) -> Result<String, ClientError> {
		if text.trim().is_empty() || text.len() > 64 * 1024 {
			return Err(ClientError::InvalidFrame);
		}
		if text.chars().count() <= 4000 {
			return Ok(text.into());
		}
		let home = self.native_home.get().ok_or(ClientError::InvalidFrame)?;
		let id = decodex_core::AccountOperationId::generate().map_err(|_| ClientError::Io)?;
		let directory = home.join("attachments").join(id.as_str());
		let path = directory.join("goal-objective.md");
		let path = path.to_str().ok_or(ClientError::InvalidFrame)?;
		let reference = format!("Read the Codex goal objective file at {path} before continuing.");
		if reference.chars().count() > 4000 {
			return Err(ClientError::InvalidFrame);
		}
		self.request_with_history(
			"fs/createDirectory",
			json!({"path":directory,"recursive":true}),
			guard.clone(),
		)
		.await?;
		use base64::{Engine as _, engine::general_purpose::STANDARD};
		self.request_with_history(
			"fs/writeFile",
			json!({"path":path,"dataBase64":STANDARD.encode(text)}),
			guard,
		)
		.await?;
		Ok(reference)
	}

	/// Apply one caller-authorized goal edit once, using the current native connection guard.
	/// Native policy remains authoritative for configured budget limits and scheduling.
	pub async fn update_thread_goal(
		&self,
		thread: &str,
		edit: &NativeGoalUpdate,
		guard: HistoryGuard,
	) -> Result<NativeThreadGoal, ClientError> {
		let params = edit.params(thread);
		if !is_native_goal_update(&params) {
			return Err(ClientError::InvalidFrame);
		}
		let result = self.request_with_history("thread/goal/set", params, guard).await?;
		project_goal(&result, thread)?.ok_or(ClientError::InvalidFrame)
	}

	/// Read the exact native goal without changing its status or starting model work.
	pub async fn thread_goal(&self, thread: &str) -> Result<Option<NativeThreadGoal>, ClientError> {
		let response = self.request("thread/goal/get", json!({"threadId":thread})).await?;
		project_goal(&response, thread)
	}
}

fn project_goal(response: &Value, thread: &str) -> Result<Option<NativeThreadGoal>, ClientError> {
	let raw = response.get("goal").ok_or(ClientError::InvalidFrame)?;
	if raw.is_null() {
		return Ok(None);
	}
	let goal: NativeThreadGoal =
		serde_json::from_value(raw.clone()).map_err(|_| ClientError::InvalidFrame)?;
	if goal.thread_id != thread
		|| goal.tokens_used < 0
		|| goal.time_used_seconds < 0
		|| goal.token_budget.is_some_and(|budget| budget <= 0)
	{
		return Err(ClientError::InvalidFrame);
	}
	Ok(Some(goal))
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::Value;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
	#[tokio::test]
	async fn native_goal_reads_distinguish_absence_limits_and_malformed_receipts() {
		let base = json!({"threadId":"thread","objective":"Native objective","status":"paused","tokenBudget":null,"tokensUsed":23,"timeUsedSeconds":7,"createdAt":10,"updatedAt":17});
		let mut cases = vec![(json!({"goal":null}), true), (json!({}), false)];
		for status in ["active", "paused", "blocked", "usageLimited", "budgetLimited", "complete"] {
			let mut goal = base.clone();
			goal["status"] = json!(status);
			cases.push((json!({"goal":goal}), true));
		}
		for (key, value) in [
			("threadId", json!("other")),
			("tokensUsed", json!(-1)),
			("timeUsedSeconds", json!(-1)),
			("tokenBudget", json!(0)),
			("status", json!("future")),
		] {
			let mut goal = base.clone();
			goal[key] = value;
			cases.push((json!({"goal":goal}), false));
		}
		for (result, valid) in cases {
			let (local, remote) = tokio::io::duplex(4096);
			let (reader, writer) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let request: Value = serde_json::from_str(
					&BufReader::new(reader).lines().next_line().await.unwrap().unwrap(),
				)
				.unwrap();
				assert_eq!(request["method"], "thread/goal/get");
				assert_eq!(request["params"], json!({"threadId":"thread"}));
				writer
					.write_all(
						format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
					)
					.await
					.unwrap();
			});
			assert_eq!(client.thread_goal("thread").await.is_ok(), valid);
			server.await.unwrap();
		}
	}
}

#[cfg(test)]
mod edit_tests {
	use super::*;
	#[tokio::test]
	#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated paused native goals"]
	async fn installed_native_goal_edits_preserve_pause_and_distinguish_budget_reset() {
		let home = tempfile::tempdir().unwrap();
		std::fs::write(
			home.path().join("config.toml"),
			"model=\"gpt-5.6-sol\"\n[goals]\nmax_goal_token_budget=100\n[features]\ngoals=true\n",
		)
		.unwrap();
		let (client, mut child) = super::super::app_link_settings::tests::native(home.path()).await;
		let started = client
			.thread_start(json!({"cwd":home.path(),"approvalPolicy":"never","sandbox":"read-only"}))
			.await
			.unwrap();
		let thread = started["thread"]["id"].as_str().unwrap();
		let guard = || client.history_guard(client.history_revision()).unwrap();
		let created = client
			.update_thread_goal(
				thread,
				&NativeGoalUpdate {
					objective: Some("Isolated paused objective".into()),
					status: Some(NativeThreadGoalStatus::Paused),
					token_budget: Some(Some(75)),
				},
				guard(),
			)
			.await
			.unwrap();
		assert_eq!(created.status, NativeThreadGoalStatus::Paused);
		assert_eq!(created.token_budget, Some(75));
		let text = "目标".repeat(2000);
		let edited = client
			.update_thread_goal(
				thread,
				&NativeGoalUpdate { objective: Some(text.clone()), ..Default::default() },
				guard(),
			)
			.await
			.unwrap_or_else(|error| match error {
				ClientError::Remote(error) => panic!("Native fixture: {}", error.message),
				other => panic!("{other:?}"),
			});
		assert_eq!(edited.objective, text);
		assert_eq!(edited.status, NativeThreadGoalStatus::Paused);
		assert_eq!(edited.token_budget, Some(75));
		let removed = client
			.update_thread_goal(
				thread,
				&NativeGoalUpdate { token_budget: Some(None), ..Default::default() },
				guard(),
			)
			.await
			.unwrap();
		assert_eq!(removed.token_budget, Some(100));
		assert_eq!(removed.objective, text);
		assert_eq!(removed.status, NativeThreadGoalStatus::Paused);
		assert!(matches!(
			client
				.update_thread_goal(
					thread,
					&NativeGoalUpdate { token_budget: Some(Some(101)), ..Default::default() },
					guard()
				)
				.await,
			Err(ClientError::Remote(_))
		));
		assert_eq!(client.thread_goal(thread).await.unwrap().unwrap().token_budget, Some(100));
		let long_text = "Long goal objective line.\n".repeat(700);
		let reference = client.materialize_goal_objective(&long_text, guard()).await.unwrap();
		let file = reference
			.strip_prefix("Read the Codex goal objective file at ")
			.unwrap()
			.strip_suffix(" before continuing.")
			.unwrap();
		assert!(
			std::path::Path::new(file)
				.starts_with(home.path().canonicalize().unwrap().join("attachments"))
		);
		assert_eq!(std::fs::read_to_string(file).unwrap(), long_text);
		let edited = client
			.update_thread_goal(
				thread,
				&NativeGoalUpdate { objective: Some(reference.clone()), ..Default::default() },
				guard(),
			)
			.await
			.unwrap();
		assert_eq!(edited.objective, reference);
		assert_eq!(edited.status, NativeThreadGoalStatus::Paused);
		child.kill().await.unwrap();
		child.wait().await.unwrap();
	}
}
