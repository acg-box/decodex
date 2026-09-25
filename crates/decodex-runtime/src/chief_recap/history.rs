//! Read public native messages through the existing bounded item-page owner.
use super::excerpts::Exchange;
use decodex_codex::app_server_client::{AppServerClient, ClientError};
use serde_json::{Value, json};
use std::collections::HashSet;

pub(super) struct History {
	pub prompt: String,
	pub latest_turn: Option<String>,
}

pub(super) async fn read(client: &AppServerClient, thread: &str) -> Result<History, ClientError> {
	tokio::time::timeout(std::time::Duration::from_secs(25), read_inner(client, thread))
		.await
		.map_err(|_| ClientError::Io)?
}

async fn read_inner(client: &AppServerClient, thread: &str) -> Result<History, ClientError> {
	let metadata = client.thread_read(json!({"threadId":thread})).await?;
	if metadata["thread"]["id"] != thread {
		return Err(ClientError::InvalidFrame);
	}
	let mut recent = Recent::default();
	match metadata["thread"]["historyMode"].as_str() {
		None | Some("legacy") => {
			let history =
				client.thread_read(json!({"threadId":thread,"includeTurns":true})).await?;
			if history["thread"]["id"] != thread {
				return Err(ClientError::InvalidFrame);
			}
			let turns = history["thread"]["turns"].as_array().ok_or(ClientError::InvalidFrame)?;
			for turn in turns.iter().rev().take(64) {
				if recent.push(turn, &turn["items"])? {
					return recent.finish();
				}
			}
			if turns.len() > 64 {
				return Err(ClientError::CapacityExceeded);
			}
			recent.finish()
		},
		Some("paginated") => read_pages(client, thread, recent).await,
		_ => Err(ClientError::InvalidFrame),
	}
}

async fn read_pages(
	client: &AppServerClient,
	thread: &str,
	mut recent: Recent,
) -> Result<History, ClientError> {
	let mut cursor: Option<String> = None;
	let mut cursors = HashSet::new();
	for _ in 0..8 {
		let page=client.request("thread/turns/list",json!({"threadId":thread,"cursor":cursor,"limit":8,"sortDirection":"desc","itemsView":"notLoaded"})).await?;
		let turns = page["data"].as_array().ok_or(ClientError::InvalidFrame)?;
		if turns.len() > 8 {
			return Err(ClientError::InvalidFrame);
		}
		for turn in turns {
			let items = client.thread_read_turn_items(thread, turn_id(turn)?).await?;
			if recent.push(turn, &items)? {
				return recent.finish();
			}
		}
		match page.get("nextCursor").ok_or(ClientError::InvalidFrame)? {
			Value::Null => return recent.finish(),
			Value::String(next)
				if !next.is_empty() && next.len() <= 4096 && cursors.insert(next.clone()) =>
				cursor = Some(next.clone()),
			_ => return Err(ClientError::InvalidFrame),
		}
	}
	Err(ClientError::CapacityExceeded)
}

fn turn_id(turn: &Value) -> Result<&str, ClientError> {
	turn["id"]
		.as_str()
		.filter(|id| !id.is_empty() && id.len() <= 512)
		.ok_or(ClientError::InvalidFrame)
}
#[derive(Default)]
struct Recent {
	seen: HashSet<String>,
	messages: Vec<Message>,
	latest: Option<String>,
	bytes: usize,
}
impl Recent {
	fn push(&mut self, turn: &Value, items: &Value) -> Result<bool, ClientError> {
		let id = turn_id(turn)?;
		if !self.seen.insert(id.to_owned()) {
			return Err(ClientError::InvalidFrame);
		}
		self.latest.get_or_insert_with(|| id.into());
		match turn["status"].as_str() {
			Some("inProgress") => return Err(ClientError::StaleHistory),
			Some("completed" | "interrupted" | "failed") => {},
			_ => return Err(ClientError::InvalidFrame),
		}
		self.bytes = self.bytes.saturating_add(items.to_string().len());
		if self.bytes > decodex_codex::app_server_client::MAX_FRAME_BYTES {
			return Err(ClientError::CapacityExceeded);
		}
		let mut next = visible(items.as_array().ok_or(ClientError::InvalidFrame)?)?;
		if turn["status"] != "completed"
			&& let Some(user) = next.iter_mut().find(|m| m.user)
		{
			user.text.push_str(if turn["status"] == "failed" {
				"\n[Native turn failed]"
			} else {
				"\n[Native turn interrupted]"
			});
		}
		next.reverse();
		self.messages.extend(next);
		Ok(select(&self.messages).iter().filter(|e| !e.assistant.is_empty()).count() >= 8)
	}

	fn finish(self) -> Result<History, ClientError> {
		finish(select(&self.messages), self.latest)
	}
}

fn finish(exchanges: Vec<Exchange>, latest_turn: Option<String>) -> Result<History, ClientError> {
	let history = super::excerpts::render(&exchanges);
	if history.is_empty() {
		return Err(ClientError::InvalidFrame);
	}
	Ok(History { prompt: super::prompt::build(&history), latest_turn })
}

struct Message {
	user: bool,
	text: String,
}
fn visible(items: &[Value]) -> Result<Vec<Message>, ClientError> {
	let mut messages = Vec::new();
	for item in items {
		// This internal handoff is not the visible voice transcript. Do not summarize it.
		if crate::chief::voice_handoff(item) {
			return Err(ClientError::InvalidFrame);
		}
		let (user, text) = match item["type"].as_str() {
			Some("userMessage") => {
				let parts = item["content"].as_array().ok_or(ClientError::InvalidFrame)?;
				let text = parts
					.iter()
					.map(|part| match part["type"].as_str() {
						Some("text") => part["text"]
							.as_str()
							.map(str::to_owned)
							.ok_or(ClientError::InvalidFrame),
						Some("image" | "localImage") => Ok("[Image attachment]".into()),
						Some("skill" | "mention") => part["name"]
							.as_str()
							.map(|name| format!("[Reference: {name}]"))
							.ok_or(ClientError::InvalidFrame),
						_ => Ok("[Attachment]".into()),
					})
					.collect::<Result<Vec<_>, _>>()?
					.join("\n");
				(true, decodex_protocol::render_chief_async_question_history(&text))
			},
			Some("agentMessage") =>
				(false, item["text"].as_str().ok_or(ClientError::InvalidFrame)?.to_owned()),
			_ => continue,
		};
		if !text.trim().is_empty() {
			messages.push(Message { user, text: text.trim().into() });
		}
	}
	Ok(messages)
}

// Input is newest-first, including native item order within each turn.
fn select(messages: &[Message]) -> Vec<Exchange> {
	let mut exchanges = Vec::new();
	let mut current = Exchange::default();
	let mut answered = 0;
	for message in messages {
		if !message.user && !current.user.is_empty() {
			answered += usize::from(!current.assistant.is_empty());
			exchanges.push(std::mem::take(&mut current));
			if answered == 8 {
				break;
			}
		}
		let field = if message.user { &mut current.user } else { &mut current.assistant };
		*field = if field.is_empty() {
			message.text.clone()
		} else {
			format!("{}\n\n{field}", message.text)
		};
	}
	if !current.user.is_empty() {
		exchanges.push(current);
	}
	exchanges.reverse();
	exchanges
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
