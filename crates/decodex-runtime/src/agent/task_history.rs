//! Native task evidence under the existing manager ownership boundary.

use super::{AgentCoordinator, AgentError, AgentWorkItem, belongs_to, exact};
use serde_json::{Value, json};

impl AgentCoordinator {
	pub(super) async fn read_work_history(
		&self,
		agent: &AgentWorkItem,
		args: &Value,
	) -> Result<Value, AgentError> {
		if !self.is_manager(&agent.id).await? {
			return Err(AgentError::Invalid("history reading requires a manager".into()));
		}
		if args.get("id").is_some() != args.get("threadId").is_some() {
			return Err(AgentError::Invalid("id and threadId must be supplied together".into()));
		}
		if args.get("searchTerm").is_some() && args.get("id").is_none() {
			return self.search_work_history(agent, args).await;
		}
		let id = exact(args, "/id")?;
		let expected = exact(args, "/threadId")?;
		let all = self.store.list_agent_work_items().await?;
		let managers = self.store.agent_manager_ids().await?;
		let work = all
			.iter()
			.find(|item| item.id == id)
			.ok_or_else(|| AgentError::Invalid("referenced work is unavailable".into()))?;
		let owned = work.id == agent.id || belongs_to(work, &agent.id, &all, &managers);
		let granted = self
			.store
			.agent_has_task_reference(agent.id.clone(), id.clone(), expected.clone())
			.await?;
		if !owned && !granted {
			return Err(AgentError::Invalid("work is outside this manager scope".into()));
		}
		let previous =
			if owned { self.store.agent_previous_threads(id.clone()).await? } else { Vec::new() };
		if !granted
			&& work.codex_thread_id.as_deref() != Some(expected.as_str())
			&& !previous.contains(&expected)
		{
			return Err(AgentError::Invalid("work thread changed; refresh agent_list_work".into()));
		}

		let limit = args.get("turnLimit").map_or(Ok(3), |v| {
			v.as_u64()
				.filter(|v| (1..=5).contains(v))
				.map(|v| v as u32)
				.ok_or_else(|| AgentError::Invalid("turnLimit must be between 1 and 5".into()))
		})?;
		let cursor = match args.get("cursor") {
			None | Some(Value::Null) => None,
			Some(Value::String(value)) => Some(value.as_str()),
			_ => return Err(AgentError::Invalid("cursor must be a string".into())),
		};
		let outputs = match args.get("includeOutputs") {
			None => false,
			Some(Value::Bool(value)) => *value,
			_ => return Err(AgentError::Invalid("includeOutputs must be boolean".into())),
		};
		if args.get("searchTerm").is_some() {
			let page = self.native_history_search(args, Some(&expected)).await?;
			let current = self.store.get_agent_work_item(id.clone()).await?;
			if current.codex_thread_id != work.codex_thread_id {
				return Err(AgentError::Invalid("work thread changed during search".into()));
			}
			return Ok(json!({"workId":id,"threadId":expected,"occurrences":page["data"],
				"nextCursor":page["nextCursor"],"evidenceOnly":true,"sourceUrl":source_url(&expected)}));
		}
		let native = self.client.thread_history_page(&expected, cursor, limit).await?;
		let current = self.store.get_agent_work_item(id.clone()).await?;
		if current.codex_thread_id != work.codex_thread_id {
			return Err(AgentError::Invalid("work thread changed during history read".into()));
		}
		let turns: Vec<_> = native["turns"]
			.as_array()
			.ok_or_else(|| AgentError::Invalid("native history has no turns".into()))?
			.iter()
			.map(|turn| summarize_turn(turn, outputs))
			.collect();
		let result = json!({"workId":id,"threadId":expected,"turns":turns,
			"order":"newest_first","nextCursor":native["nextCursor"],
			"evidenceOnly":true,"previousThreadIds":previous});
		if result.to_string().len() > 64 * 1024 {
			return Err(AgentError::Invalid(
				"history summary exceeds budget; reduce turnLimit".into(),
			));
		}
		Ok(result)
	}

	async fn search_work_history(
		&self,
		agent: &AgentWorkItem,
		args: &Value,
	) -> Result<Value, AgentError> {
		let page = self.native_history_search(args, None).await?;
		// Filter after the native read against current ownership. A native match never grants
		// access.
		let all = self.store.list_agent_work_items().await?;
		let managers = self.store.agent_manager_ids().await?;

		let mut permitted = std::collections::HashMap::new();
		for work in &all {
			if work.id == agent.id || belongs_to(work, &agent.id, &all, &managers) {
				if let Some(thread) = &work.codex_thread_id {
					permitted.insert(thread.clone(), work.id.clone());
				}
				for thread in self.store.agent_previous_threads(work.id.clone()).await? {
					permitted.insert(thread, work.id.clone());
				}
			}
		}
		let mut matches = Vec::new();
		for row in page["data"].as_array().expect("validated search page") {
			let thread = row["threadId"].as_str().expect("validated thread");
			let work = match permitted.get(thread) {
				Some(work) => Some(work.clone()),
				None =>
					self.store.agent_task_reference_target(agent.id.clone(), thread.into()).await?,
			};
			if let Some(work) = work {
				matches.push(json!({"workId":work,"threadId":thread,"title":row["title"],
					"snippet":row["snippet"],"sourceUrl":source_url(thread)}));
			}
		}

		Ok(json!({"matches":matches,"nextCursor":page["nextCursor"],"evidenceOnly":true,
			"scope":"current native connection; owned work and user-selected task references",
			"pageMayBeEmptyAfterScopeFilter":true}))
	}

	async fn native_history_search(
		&self,
		args: &Value,
		thread: Option<&str>,
	) -> Result<Value, AgentError> {
		let query = args["searchTerm"]
			.as_str()
			.filter(|s| !s.trim().is_empty() && s.len() <= 512)
			.ok_or_else(|| AgentError::Invalid("searchTerm must contain 1 to 512 bytes".into()))?;
		let cursor = match args.get("cursor") {
			None | Some(Value::Null) => Value::Null,
			Some(Value::String(s)) if !s.is_empty() && s.len() <= 4096 => json!(s),
			_ => return Err(AgentError::Invalid("invalid search cursor".into())),
		};
		let mut params = json!({"searchTerm":query,"cursor":cursor,"limit":20});
		let method = if let Some(thread) = thread {
			params["threadId"] = json!(thread);
			"thread/searchOccurrences"
		} else {
			let archived = match args.get("archived") {
				None => false,
				Some(Value::Bool(value)) => *value,
				_ => return Err(AgentError::Invalid("archived must be boolean".into())),
			};
			params["archived"] = json!(archived);
			params["sortKey"] = json!("recency_at");
			params["sourceKinds"] = json!([
				"cli",
				"vscode",
				"exec",
				"appServer",
				"subAgent",
				"subAgentReview",
				"subAgentCompact",
				"subAgentThreadSpawn",
				"subAgentOther",
				"unknown"
			]);
			"thread/search"
		};
		let page = tokio::time::timeout(
			std::time::Duration::from_secs(20),
			self.client.request(method, params),
		)
		.await
		.map_err(|_| AgentError::Invalid("native search timed out".into()))??;
		let invalid = || AgentError::Invalid("invalid or oversized native search page".into());
		let rows = page["data"].as_array().filter(|rows| rows.len() <= 20).ok_or_else(invalid)?;
		let next = match page.get("nextCursor").ok_or_else(invalid)? {
			Value::Null => Value::Null,
			Value::String(next)
				if !next.is_empty() && next.len() <= 4096 && page["nextCursor"] != cursor =>
				json!(next),
			_ => return Err(invalid()),
		};
		let mut data = Vec::new();
		for row in rows {
			let snippet =
				row["snippet"].as_str().filter(|s| s.len() <= 8192).ok_or_else(invalid)?;
			let projected = if thread.is_some() {
				let turn = exact(row, "/turnId")?;
				let item = exact(row, "/itemId")?;
				let turn_cursor = row["turnCursor"]
					.as_str()
					.filter(|s| !s.is_empty() && s.len() <= 4096)
					.ok_or_else(invalid)?;
				json!({"turnId":turn,"itemId":item,"turnCursor":turn_cursor,"snippet":snippet,
					"snippetMatchRange":row["snippetMatchRange"],"rangeEncoding":"utf16"})
			} else {
				let id = exact(row, "/thread/id")?;
				let title = row["thread"]["name"].as_str().filter(|s| s.len() <= 4096);
				json!({"threadId":id,"title":title,"snippet":snippet})
			};
			data.push(projected);
		}
		let result = json!({"data":data,"nextCursor":next});
		if result.to_string().len() > 64 * 1024 {
			return Err(invalid());
		}
		Ok(result)
	}
}

fn source_url(thread: &str) -> String {
	let mut url = reqwest::Url::parse("codex://threads/").expect("constant URL");
	url.path_segments_mut().expect("hierarchical URL").pop_if_empty().push(thread);
	url.into()
}

fn summarize_turn(turn: &Value, outputs: bool) -> Value {
	let items = turn["items"].as_array().expect("native page validates items");
	let start = items.len().saturating_sub(20);
	let summarized: Vec<_> =
		items[start..].iter().map(|item| summarize_item(item, outputs)).collect();
	json!({"id":turn["id"],"status":turn["status"],"items":summarized,
		"omittedEarlierItems":start,"startedAt":turn["startedAt"],"completedAt":turn["completedAt"]})
}

fn summarize_item(item: &Value, outputs: bool) -> Value {
	let mut result = json!({});
	let mut truncated = false;
	for field in [
		"id",
		"type",
		"status",
		"phase",
		"text",
		"content",
		"summary",
		"command",
		"cwd",
		"exitCode",
		"durationMs",
		"name",
		"namespace",
		"tool",
		"server",
		"success",
		"query",
		"path",
		"kind",
		"agentThreadId",
		"agentPath",
		"review",
	] {
		if let Some(value) = item.get(field) {
			// User content contains attachment references; never inline media bytes.
			result[field] = bounded(value, 0, &mut truncated);
		}
	}
	if outputs {
		for field in ["output", "aggregatedOutput", "changes"] {
			if let Some(value) = item.get(field) {
				result[field] = bounded(value, 0, &mut truncated);
			}
		}
	}
	result["truncated"] = json!(truncated);
	result["outputsIncluded"] = json!(outputs);
	result
}

fn bounded(value: &Value, depth: usize, truncated: &mut bool) -> Value {
	if depth > 4 {
		*truncated = true;
		return json!("[depth omitted]");
	}
	match value {
		Value::String(text) => {
			if text.starts_with("data:") {
				*truncated = true;
				return json!("[inline media omitted]");
			}
			let shortened: String = text.chars().take(1024).collect();
			*truncated |= shortened.len() < text.len();
			json!(shortened)
		},
		Value::Array(values) => {
			*truncated |= values.len() > 16;
			Value::Array(values.iter().take(16).map(|v| bounded(v, depth + 1, truncated)).collect())
		},
		Value::Object(values) => {
			*truncated |= values.len() > 16;
			Value::Object(
				values
					.iter()
					.take(16)
					.map(|(k, v)| (k.clone(), bounded(v, depth + 1, truncated)))
					.collect(),
			)
		},
		_ => value.clone(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn summary_reports_omitted_items_and_unicode_truncation() {
		let items: Vec<_> = (0..25)
			.map(|id| {
				json!({"id":id.to_string(),
			"type":"agentMessage","text":"界".repeat(1100)})
			})
			.collect();
		let result = summarize_turn(&json!({"id":"turn","items":items}), false);
		assert_eq!(result["omittedEarlierItems"], 5);
		assert_eq!(result["items"].as_array().unwrap().len(), 20);
		assert_eq!(result["items"][0]["id"], "5");
		assert_eq!(result["items"][19]["id"], "24");
		assert_eq!(result["items"][0]["text"].as_str().unwrap().chars().count(), 1024);
		assert_eq!(result["items"][0]["truncated"], true);
	}
}
