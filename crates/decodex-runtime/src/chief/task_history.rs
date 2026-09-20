//! Native task evidence under the existing manager ownership boundary.

use super::{ChiefCoordinator, ChiefError, ChiefWorkItem, belongs_to, exact};
use serde_json::{Value, json};

impl ChiefCoordinator {
	pub(super) async fn read_work_history(
		&self,
		chief: &ChiefWorkItem,
		args: &Value,
	) -> Result<Value, ChiefError> {
		if !self.is_manager(&chief.id).await? {
			return Err(ChiefError::Invalid("history reading requires a manager".into()));
		}
		let id = exact(args, "/id")?;
		let expected = exact(args, "/threadId")?;
		let all = self.store.list_chief_work_items().await?;
		let managers = self.store.chief_manager_ids().await?;
		let work = all
			.iter()
			.find(|item| item.id == id)
			.ok_or_else(|| ChiefError::Invalid("referenced work is unavailable".into()))?;
		let owned = work.id == chief.id || belongs_to(work, &chief.id, &all, &managers);
		let granted = self
			.store
			.chief_has_task_reference(chief.id.clone(), id.clone(), expected.clone())
			.await?;
		if !owned && !granted {
			return Err(ChiefError::Invalid("work is outside this manager scope".into()));
		}
		let previous =
			if owned { self.store.chief_previous_threads(id.clone()).await? } else { Vec::new() };
		if !granted
			&& work.codex_thread_id.as_deref() != Some(expected.as_str())
			&& !previous.contains(&expected)
		{
			return Err(ChiefError::Invalid("work thread changed; refresh chief_list_work".into()));
		}

		let limit = args.get("turnLimit").map_or(Ok(3), |v| {
			v.as_u64()
				.filter(|v| (1..=5).contains(v))
				.map(|v| v as u32)
				.ok_or_else(|| ChiefError::Invalid("turnLimit must be between 1 and 5".into()))
		})?;
		let cursor = match args.get("cursor") {
			None | Some(Value::Null) => None,
			Some(Value::String(value)) => Some(value.as_str()),
			_ => return Err(ChiefError::Invalid("cursor must be a string".into())),
		};
		let outputs = match args.get("includeOutputs") {
			None => false,
			Some(Value::Bool(value)) => *value,
			_ => return Err(ChiefError::Invalid("includeOutputs must be boolean".into())),
		};
		let native = self.client.thread_history_page(&expected, cursor, limit).await?;
		let current = self.store.get_chief_work_item(id.clone()).await?;
		if current.codex_thread_id != work.codex_thread_id {
			return Err(ChiefError::Invalid("work thread changed during history read".into()));
		}
		let turns: Vec<_> = native["turns"]
			.as_array()
			.ok_or_else(|| ChiefError::Invalid("native history has no turns".into()))?
			.iter()
			.map(|turn| summarize_turn(turn, outputs))
			.collect();
		let result = json!({"workId":id,"threadId":expected,"turns":turns,
			"order":"newest_first","nextCursor":native["nextCursor"],
			"evidenceOnly":true,"previousThreadIds":previous});
		if result.to_string().len() > 64 * 1024 {
			return Err(ChiefError::Invalid(
				"history summary exceeds budget; reduce turnLimit".into(),
			));
		}
		Ok(result)
	}
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
