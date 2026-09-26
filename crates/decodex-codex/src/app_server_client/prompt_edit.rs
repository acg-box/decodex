//! Read a canonical edit candidate without changing native history or starting work.
use super::{AppServerClient, ClientError, HistoryGuard};
use serde_json::{Value, json};

/// Complete native input evidence for a later, separately authorized edit.
#[derive(Clone)]
pub struct PromptEditCandidate {
	/// Exact native thread; editing must keep this identity.
	pub thread_id: String,
	/// First excluded turn if the user later confirms a native revert.
	pub before_turn_id: String,
	/// Exact first user item, never a mid-turn steer.
	pub item_id: String,
	/// Canonical native content, including attachment and mention fields.
	pub content: Vec<Value>,
	/// Latest native turn observed before and after reading the selected input.
	pub latest_turn_id: String,
	/// Complete chronological native history used by durable recovery.
	pub turn_ids: Vec<String>,
	/// Existing connection/history/settings guard; this is not mutation authorization.
	pub guard: HistoryGuard,
}

impl AppServerClient {
	/// Prepare one exact first input from a finished paginated turn. None means not editable.
	/// Complete native item pages distinguish first input from a clipped steer. The caller
	/// must revalidate this evidence and own uncertain mutation recovery before any revert.
	pub async fn prompt_edit_candidate(
		&self,
		thread: &str,
		turn: &str,
		item: &str,
	) -> Result<Option<PromptEditCandidate>, ClientError> {
		if [thread, turn, item].iter().any(|id| id.is_empty() || id.len() > 512) {
			return Err(ClientError::InvalidFrame);
		}
		tokio::time::timeout(std::time::Duration::from_secs(60), async {
			let guard = self.thread_settings_guard(thread).ok_or(ClientError::InvalidFrame)?;
			let metadata = self.thread_read(json!({"threadId":thread})).await?;
			if metadata["thread"]["id"] != thread {
				return Err(ClientError::InvalidFrame);
			}
			if metadata["thread"]["historyMode"] != "paginated" {
				return Ok(None);
			}
			let headers = self.thread_turns_since(thread, None).await?;
			let Some(index) = headers.iter().position(|header| header["id"] == turn) else {
				return Ok(None);
			};
			let last = headers.last().ok_or(ClientError::InvalidFrame)?;
			if !terminal(&headers[index]) || !terminal(last) {
				return Ok(None);
			}
			let latest = last["id"].as_str().ok_or(ClientError::InvalidFrame)?.to_owned();
			let items = self.thread_read_turn_items(thread, turn).await?;
			let items = items.as_array().ok_or(ClientError::InvalidFrame)?;
			let Some(content) = first_input(items, item)? else {
				return Ok(None);
			};
			if index > 0
				&& headers[index]["status"] == "interrupted"
				&& headers[index]["completedAt"].is_null()
			{
				let previous = &headers[index - 1];
				let previous_id = previous["id"].as_str().ok_or(ClientError::InvalidFrame)?;
				let previous_items = self.thread_read_turn_items(thread, previous_id).await?;
				if hidden_nested_review(
					previous,
					previous_items.as_array().ok_or(ClientError::InvalidFrame)?,
					items,
				) {
					return Ok(None);
				}
			}
			if self.thread_latest_turn_id(thread).await?.as_deref() != Some(latest.as_str())
				|| !guard.is_live()
			{
				return Err(ClientError::InvalidFrame);
			}
			Ok(Some(PromptEditCandidate {
				thread_id: thread.into(),
				before_turn_id: turn.into(),
				item_id: item.into(),
				content,
				latest_turn_id: latest,
				turn_ids: headers
					.iter()
					.map(|turn| {
						turn["id"].as_str().map(str::to_owned).ok_or(ClientError::InvalidFrame)
					})
					.collect::<Result<_, _>>()?,
				guard,
			}))
		})
		.await
		.map_err(|_| ClientError::Io)?
	}
}
fn terminal(turn: &Value) -> bool {
	matches!(turn["status"].as_str(), Some("completed" | "interrupted" | "failed"))
}
fn first_input(items: &[Value], selected: &str) -> Result<Option<Vec<Value>>, ClientError> {
	let mut review = false;
	for item in items {
		match item["type"].as_str() {
			Some("enteredReviewMode") => review = true,
			Some("exitedReviewMode") => review = false,
			Some("userMessage") => {
				if review || item["id"] != selected {
					return Ok(None);
				}
				let content = item["content"].as_array().ok_or(ClientError::InvalidFrame)?;
				return Ok((!content.is_empty()).then(|| content.clone()));
			},
			_ => {},
		}
	}
	Ok(None)
}
fn hidden_nested_review(previous: &Value, previous_items: &[Value], items: &[Value]) -> bool {
	let mut users = items.iter().filter(|item| item["type"] == "userMessage");
	previous["status"] == "completed"
		&& previous_items.iter().any(|item| item["type"] == "enteredReviewMode")
		&& previous_items.iter().any(|item| item["type"] == "exitedReviewMode")
		&& matches!((users.next(),users.next(),users.next()),(Some(a),Some(b),None) if a["content"] == b["content"])
}

#[cfg(test)]
#[path = "prompt_edit_tests.rs"]
mod tests;
