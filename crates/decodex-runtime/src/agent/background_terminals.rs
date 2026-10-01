//! Manage native background commands within one current, manager-owned task.
use super::{AgentCoordinator, AgentError, AgentWorkItem, belongs_to, exact};
use serde_json::{Value, json};

impl AgentCoordinator {
	pub(super) async fn background_terminals(
		&self,
		agent: &AgentWorkItem,
		args: &Value,
	) -> Result<Value, AgentError> {
		if !self.is_manager(&agent.id).await? {
			return Err(AgentError::Invalid("background commands require a manager".into()));
		}

		let id = exact(args, "/id")?;
		let thread = exact(args, "/threadId")?;
		let all = self.store.list_agent_work_items().await?;
		let managers = self.store.agent_manager_ids().await?;
		let work = all
			.iter()
			.find(|work| work.id == id)
			.ok_or_else(|| AgentError::Invalid("work is unavailable".into()))?;
		// A read-only task reference does not grant control of another task's processes.
		if !(work.id == agent.id || belongs_to(work, &agent.id, &all, &managers))
			|| work.codex_thread_id.as_deref() != Some(thread.as_str())
		{
			return Err(AgentError::Invalid(
				"background command target is outside the current manager scope".into(),
			));
		}

		let operation = exact(args, "/operation")?;
		let (method, params) = match operation.as_str() {
			"list" => {
				let cursor = match args.get("cursor") {
					None | Some(Value::Null) => Value::Null,
					Some(Value::String(value)) if !value.is_empty() && value.len() <= 4_096 =>
						json!(value),
					_ =>
						return Err(AgentError::Invalid("invalid background command cursor".into())),
				};

				(
					"thread/backgroundTerminals/list",
					json!({"threadId":thread,"cursor":cursor,"limit":20}),
				)
			},
			"terminate" => {
				let process = exact(args, "/processId")?;

				process
					.parse::<i32>()
					.map_err(|_| AgentError::Invalid("invalid native processId".into()))?;

				(
					"thread/backgroundTerminals/terminate",
					json!({"threadId":thread,"processId":process}),
				)
			},
			_ => return Err(AgentError::Invalid("operation must be list or terminate".into())),
		};
		// Native owns process membership and termination. Never send OS signals or retry a write.
		let response = tokio::time::timeout(
			std::time::Duration::from_secs(20),
			self.client.request(method, params.clone()),
		)
		.await
		.map_err(|_| {
			AgentError::Invalid(
				"background command result is unconfirmed; inspect before retrying".into(),
			)
		})??;
		let invalid = || AgentError::Invalid("invalid native background command response".into());

		if operation == "terminate" {
			let terminated = response["terminated"].as_bool().ok_or_else(invalid)?;

			return Ok(
				json!({"workId":id,"threadId":thread,"processId":params["processId"],"terminated":terminated}),
			);
		}

		let entries =
			response["data"].as_array().filter(|rows| rows.len() <= 20).ok_or_else(invalid)?;
		let mut terminals = Vec::new();

		for entry in entries {
			let process = exact(entry, "/processId")?;
			let item = exact(entry, "/itemId")?;
			let command = entry["command"].as_str().ok_or_else(invalid)?;
			let cwd = entry["cwd"].as_str().ok_or_else(invalid)?;
			let text: String = command.chars().take(2_048).collect();

			terminals.push(json!({"processId":process,"itemId":item,"command":text,
				"commandTruncated":text.len()!=command.len(),"cwd":cwd}));
		}

		let cursor = match &response["nextCursor"] {
			Value::Null => Value::Null,
			Value::String(cursor)
				if !cursor.is_empty()
					&& cursor.len() <= 4_096
					&& response["nextCursor"] != params["cursor"] =>
				json!(cursor),
			_ => return Err(invalid()),
		};
		let result =
			json!({"workId":id,"threadId":thread,"terminals":terminals,"nextCursor":cursor});

		if result.to_string().len() > 64 * 1_024 {
			return Err(invalid());
		}

		Ok(result)
	}
}
