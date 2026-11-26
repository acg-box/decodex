//! Ephemeral Codex threads expose live items but have no native history pages.
use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
};

use decodex_protocol::AgentTimelineContent;
use serde_json::{Value, json};

#[derive(Clone, Default)]
pub(crate) struct SideChatHistory(Arc<Mutex<HashMap<String, Transcript>>>);

#[derive(Default)]
struct Transcript {
	items: Vec<AgentTimelineContent>,
	pending: HashMap<(String, String), usize>,
}

pub(crate) struct Page {
	pub items: Vec<AgentTimelineContent>,
	pub next: Option<String>,
}

impl SideChatHistory {
	pub fn open(&self, thread: &str) {
		self.0.lock().unwrap().entry(thread.into()).or_default();
	}

	pub fn close(&self, thread: &str) {
		self.0.lock().unwrap().remove(thread);
	}

	pub fn clear(&self) {
		self.0.lock().unwrap().clear();
	}

	pub fn observe(&self, method: &str, params: &Value) {
		if !matches!(method, "item/started" | "item/completed" | "turn/started" | "turn/completed")
		{
			return;
		}
		let Some(thread) = params["threadId"].as_str() else { return };
		let mut open = self.0.lock().unwrap();
		let Some(chat) = open.get_mut(thread) else { return };
		if method.starts_with("turn/") {
			let turn = &params["turn"];
			if let Some(boundary) = super::turn_boundary(&json!({
				"type":if method=="turn/started" {"turnStarted"} else {"turnCompleted"},
				"turn_id":turn["id"],"status":turn["status"],"duration_ms":turn["durationMs"],
				"started_at":turn["startedAt"],"error":turn["error"]
			})) {
				chat.items.push(boundary);
			}
			if method == "turn/completed" {
				chat.pending.clear();
			}
		} else if let Some(item @ AgentTimelineContent::Item { .. }) = super::ordinary(params) {
			let key = (
				params["turnId"].as_str().unwrap().into(),
				params["item"]["id"].as_str().unwrap().into(),
			);
			if let Some(position) = chat.pending.remove(&key) {
				chat.items[position] = item;
				if method == "item/started" {
					chat.pending.insert(key, position);
				}
			} else {
				if method == "item/started" {
					chat.pending.insert(key, chat.items.len());
				}
				chat.items.push(item);
			}
		}
	}

	/// Stable offsets address only this in-memory conversation, never a native rollout.
	pub fn page(&self, thread: &str, cursor: Option<&str>) -> Option<Result<Page, ()>> {
		let open = self.0.lock().unwrap();
		let items = &open.get(thread)?.items;
		let prefix = format!("side:{thread}:");
		let end = match cursor {
			None => items.len(),
			Some(cursor) =>
				match cursor.strip_prefix(&prefix).and_then(|s| s.parse::<usize>().ok()) {
					Some(end) if end > 0 && end <= items.len() => end,
					_ => return Some(Err(())),
				},
		};
		let mut start = end;
		let mut size = 0;
		for item in items[..end].iter().rev().take(30) {
			let bytes = serde_json::to_vec(item).ok()?.len();
			if size + bytes > 55 * 1024 && start < end {
				break;
			}
			start -= 1;
			size += bytes;
		}
		Some(Ok(Page {
			items: items[start..end].to_vec(),
			next: (start > 0).then(|| format!("{prefix}{start}")),
		}))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn side_chat_pages_track_the_native_item_until_close() {
		let history = SideChatHistory::default();
		history.open("side");
		for (method, text) in [("item/started", ""), ("item/completed", "Answer")] {
			history.observe(method,&json!({"threadId":"side","turnId":"turn","item":{"type":"agentMessage","id":"reply","text":text,"phase":"final_answer"}}));
		}
		let page = history.page("side", None).unwrap().unwrap();
		assert!(matches!(&page.items[..],[AgentTimelineContent::Item{text,..}] if text=="Answer"));
		assert!(history.page("side", Some("side:other:1")).unwrap().is_err());
		history.close("side");
		assert!(history.page("side", None).is_none());
	}
}
