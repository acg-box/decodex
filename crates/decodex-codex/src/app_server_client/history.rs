//! Read one exact turn through the native history pages without hydrating the whole thread.

use super::{AppServerClient, ClientError, MAX_FRAME_BYTES};
use serde_json::{Value, json};
use std::collections::HashSet;

const MAX_PAGES: usize = 128;
const PAGE_SIZE: usize = 100;

impl AppServerClient {
	/// Read thread metadata and the requested turn only. Paginated threads use native
	/// turn/item pages; legacy threads retain their supported full-history read.
	/// Missing turns remain absent. Incomplete or mismatched pages return an error,
	/// never partial evidence represented as complete history.
	pub async fn thread_read_turn(&self, thread: &str, turn: &str) -> Result<Value, ClientError> {
		tokio::time::timeout(
			std::time::Duration::from_secs(60),
			self.read_turn_history(thread, turn),
		)
		.await
		.map_err(|_| ClientError::Io)?
	}

	/// Read turn headers newer than an exact baseline, in chronological order.
	/// Missing baselines and incomplete pages are errors, never empty recovery.
	pub async fn thread_turns_since(
		&self,
		thread: &str,
		baseline: Option<&str>,
	) -> Result<Vec<Value>, ClientError> {
		tokio::time::timeout(
			std::time::Duration::from_secs(60),
			self.turn_headers(thread, baseline, false),
		)
		.await
		.map_err(|_| ClientError::Io)?
	}

	/// Read only the latest turn identity for a new voice call's recovery baseline.
	pub async fn thread_latest_turn_id(&self, thread: &str) -> Result<Option<String>, ClientError> {
		let turns = tokio::time::timeout(
			std::time::Duration::from_secs(60),
			self.turn_headers(thread, None, true),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		Ok(turns.last().and_then(|turn| turn["id"].as_str()).map(str::to_owned))
	}

	async fn turn_headers(
		&self,
		thread: &str,
		baseline: Option<&str>,
		latest: bool,
	) -> Result<Vec<Value>, ClientError> {
		let history = self.thread_read(json!({"threadId":thread})).await?;
		validate_thread(&history, thread)?;
		match history.pointer("/thread/historyMode").and_then(Value::as_str) {
			None | Some("legacy") => {
				let history =
					self.thread_read(json!({"threadId":thread,"includeTurns":true})).await?;
				validate_thread(&history, thread)?;
				let turns = history
					.pointer("/thread/turns")
					.and_then(Value::as_array)
					.ok_or(ClientError::InvalidFrame)?;
				let mut ids = HashSet::new();
				for turn in turns {
					let id = turn["id"]
						.as_str()
						.filter(|id| !id.is_empty())
						.ok_or(ClientError::InvalidFrame)?;
					if !ids.insert(id) {
						return Err(ClientError::InvalidFrame);
					}
				}
				if latest {
					return Ok(turns.last().cloned().into_iter().collect());
				}
				let start = match baseline {
					Some(id) =>
						turns
							.iter()
							.position(|turn| turn["id"].as_str() == Some(id))
							.ok_or(ClientError::InvalidFrame)?
							+ 1,
					None => 0,
				};
				Ok(turns[start..].to_vec())
			},
			Some("paginated") => {
				let mut pages = Pages::default();
				let mut budget = MAX_FRAME_BYTES;
				let mut turns = Vec::new();
				let mut ids = HashSet::new();
				loop {
					let page = self.request("thread/turns/list", json!({"threadId":thread,"cursor":pages.cursor,"limit":if latest {1} else {PAGE_SIZE},"sortDirection":"desc","itemsView":"notLoaded"})).await?;
					charge(&page, &mut budget)?;
					for turn in data(&page)? {
						let id = turn["id"]
							.as_str()
							.filter(|id| !id.is_empty())
							.ok_or(ClientError::InvalidFrame)?;
						if !ids.insert(id.to_owned()) {
							return Err(ClientError::InvalidFrame);
						}
						if Some(id) == baseline {
							turns.reverse();
							return Ok(turns);
						}
						turns.push(turn.clone());
						if latest {
							return Ok(turns);
						}
					}
					if !pages.advance(&page)? {
						if baseline.is_some() {
							return Err(ClientError::InvalidFrame);
						}
						turns.reverse();
						return Ok(turns);
					}
				}
			},
			_ => Err(ClientError::InvalidFrame),
		}
	}

	async fn read_turn_history(&self, thread: &str, turn: &str) -> Result<Value, ClientError> {
		let mut history = self.thread_read(json!({"threadId":thread})).await?;
		validate_thread(&history, thread)?;
		let mode = history.pointer("/thread/historyMode");
		if mode.is_none() || mode == Some(&json!("legacy")) {
			let history = self.thread_read(json!({"threadId":thread,"includeTurns":true})).await?;
			validate_thread(&history, thread)?;
			return Ok(history);
		}
		if mode != Some(&json!("paginated")) {
			return Err(ClientError::InvalidFrame);
		}
		let mut budget = MAX_FRAME_BYTES;
		let mut pages = Pages::default();
		let mut selected = None;
		loop {
			let page = self
				.request(
					"thread/turns/list",
					json!({
						"threadId":thread,"cursor":pages.cursor,"limit":PAGE_SIZE,
						"sortDirection":"desc","itemsView":"notLoaded"
					}),
				)
				.await?;
			charge(&page, &mut budget)?;
			let entries = data(&page)?;
			for entry in entries {
				let id = entry["id"].as_str().ok_or(ClientError::InvalidFrame)?;
				if id == turn {
					if selected.is_some() {
						return Err(ClientError::InvalidFrame);
					}
					selected = Some(entry.clone());
				}
			}
			if selected.is_some() || !pages.advance(&page)? {
				break;
			}
		}
		history["thread"]["turns"] = match selected {
			Some(mut selected) => {
				selected["items"] = self.read_turn_items(thread, turn, &mut budget).await?;
				selected["itemsView"] = json!("full");
				json!([selected])
			},
			None => json!([]),
		};
		Ok(history)
	}

	async fn read_turn_items(
		&self,
		thread: &str,
		turn: &str,
		budget: &mut usize,
	) -> Result<Value, ClientError> {
		let mut pages = Pages::default();
		let mut items = Vec::new();
		let mut ids = HashSet::new();
		loop {
			let page = self
				.request(
					"thread/items/list",
					json!({
						"threadId":thread,"turnId":turn,"cursor":pages.cursor,
						"limit":PAGE_SIZE,"sortDirection":"asc"
					}),
				)
				.await?;
			charge(&page, budget)?;
			for entry in data(&page)? {
				if entry["turnId"].as_str() != Some(turn) {
					return Err(ClientError::InvalidFrame);
				}
				let item = &entry["item"];
				let id = item["id"].as_str().ok_or(ClientError::InvalidFrame)?;
				if !ids.insert(id.to_owned()) {
					return Err(ClientError::InvalidFrame);
				}
				items.push(item.clone());
			}
			if !pages.advance(&page)? {
				return Ok(Value::Array(items));
			}
		}
	}
}

fn validate_thread(history: &Value, thread: &str) -> Result<(), ClientError> {
	if history.pointer("/thread/id").and_then(Value::as_str) != Some(thread) {
		return Err(ClientError::InvalidFrame);
	}
	Ok(())
}

fn data(page: &Value) -> Result<&Vec<Value>, ClientError> {
	page["data"].as_array().ok_or(ClientError::InvalidFrame)
}

fn charge(page: &Value, budget: &mut usize) -> Result<(), ClientError> {
	*budget = budget.checked_sub(page.to_string().len()).ok_or(ClientError::CapacityExceeded)?;
	Ok(())
}

#[derive(Default)]
struct Pages {
	cursor: Option<String>,
	seen: HashSet<String>,
}

impl Pages {
	fn advance(&mut self, page: &Value) -> Result<bool, ClientError> {
		match page.get("nextCursor") {
			Some(Value::Null) => Ok(false),
			Some(Value::String(next)) if !next.is_empty() => {
				if !self.seen.insert(next.clone()) {
					return Err(ClientError::InvalidFrame);
				}
				if self.seen.len() >= MAX_PAGES {
					return Err(ClientError::CapacityExceeded);
				}
				self.cursor = Some(next.clone());
				Ok(true)
			},
			_ => Err(ClientError::InvalidFrame),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

	async fn read(pages: Vec<(&'static str, Value)>) -> (Result<Value, ClientError>, Vec<Value>) {
		run(pages, None).await
	}

	async fn run(
		pages: Vec<(&'static str, Value)>,
		mode: Option<Option<&str>>,
	) -> (Result<Value, ClientError>, Vec<Value>) {
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			let mut requests = Vec::new();
			for (method, response) in pages.into_iter().map(|(m, r)| (m.to_owned(), r)) {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], method);
				writer
					.write_all(
						format!("{}\n", json!({"id":request["id"],"result":response})).as_bytes(),
					)
					.await
					.unwrap();
				requests.push(request);
			}
			requests
		});
		let result = match mode {
			Some(Some("__latest_test__")) =>
				client.thread_latest_turn_id("thread/opaque").await.map(|id| json!(id)),
			None => client.thread_read_turn("thread/opaque", "target").await,
			Some(baseline) =>
				client.thread_turns_since("thread/opaque", baseline).await.map(Value::Array),
		};
		(result, server.await.unwrap())
	}

	fn metadata() -> Value {
		json!({"thread":{"id":"thread/opaque","historyMode":"paginated","status":{"type":"idle"},"turns":[]}})
	}

	fn turn_page() -> Value {
		json!({"data":[{"id":"target","status":"completed","items":[],"itemsView":"notLoaded"}],"nextCursor":null})
	}

	#[tokio::test]
	async fn exact_turn_and_all_item_pages_preserve_order_and_metadata() {
		let (result, requests) = read(vec![
			("thread/read", metadata()),
			("thread/turns/list", json!({"data":[{"id":"other"}],"nextCursor":"turn cursor"})),
			("thread/turns/list", turn_page()),
			("thread/items/list", json!({"data":[{"turnId":"target","item":{"id":"one","type":"agentMessage","text":"progress","phase":"commentary"}}],"nextCursor":"item cursor"})),
			("thread/items/list", json!({"data":[{"turnId":"target","item":{"id":"two","type":"agentMessage","text":"done","phase":"final_answer","delivery":"async"}}],"nextCursor":null})),
		]).await;
		let history = result.unwrap();
		assert_eq!(history["thread"]["turns"].as_array().unwrap().len(), 1);
		assert_eq!(history["thread"]["turns"][0]["itemsView"], "full");
		assert_eq!(history["thread"]["turns"][0]["items"][1]["text"], "done");
		assert_eq!(history["thread"]["turns"][0]["items"][1]["delivery"], "async");
		assert_eq!(requests[2]["params"]["cursor"], "turn cursor");
		assert_eq!(requests[4]["params"]["cursor"], "item cursor");
		assert_eq!(requests[3]["params"]["turnId"], "target");
		assert!(requests.iter().all(|r| r["params"].get("includeTurns").is_none()));
	}

	#[tokio::test]
	async fn missing_turn_is_not_replaced_with_a_neighbor() {
		let (result, _) = read(vec![
			("thread/read", metadata()),
			("thread/turns/list", json!({"data":[{"id":"other"}],"nextCursor":null})),
		])
		.await;
		assert_eq!(result.unwrap()["thread"]["turns"], json!([]));
	}

	#[tokio::test]
	async fn repeated_cursor_and_cross_turn_items_are_rejected() {
		let page = json!({"data":[],"nextCursor":"same"});
		let (result, _) = read(vec![
			("thread/read", metadata()),
			("thread/turns/list", page.clone()),
			("thread/turns/list", page),
		])
		.await;
		assert!(matches!(result, Err(ClientError::InvalidFrame)));
		let (result, _) = read(vec![
			("thread/read", metadata()),
			("thread/turns/list", turn_page()),
			(
				"thread/items/list",
				json!({"data":[{"turnId":"other","item":{"id":"message"}}],"nextCursor":null}),
			),
		])
		.await;
		assert!(matches!(result, Err(ClientError::InvalidFrame)));
	}

	#[tokio::test]
	async fn legacy_read_preserves_history_and_checks_thread_identity() {
		let history = json!({"thread":{"id":"thread/opaque","turns":[{"id":"target"}]}});
		let (result, requests) =
			read(vec![("thread/read", history.clone()), ("thread/read", history.clone())]).await;
		assert_eq!(result.unwrap(), history);
		assert_eq!(requests[1]["params"]["includeTurns"], true);
		let (result, _) = read(vec![("thread/read", json!({"thread":{"id":"other"}}))]).await;
		assert!(matches!(result, Err(ClientError::InvalidFrame)));
	}

	#[tokio::test]
	async fn recovery_pages_stop_at_exact_baseline_and_restore_chronological_order() {
		let (result, requests) = run(
			vec![
				("thread/read", metadata()),
				(
					"thread/turns/list",
					json!({"data":[{"id":"newest","status":"completed"}],"nextCursor":"older"}),
				),
				(
					"thread/turns/list",
					json!({"data":[{"id":"middle"},{"id":"baseline"},{"id":"old"}],"nextCursor":null}),
				),
			],
			Some(Some("baseline")),
		)
		.await;
		let turns = result.unwrap();
		assert_eq!(turns[0]["id"], "middle");
		assert_eq!(turns[1]["id"], "newest");
		assert_eq!(turns.as_array().unwrap().len(), 2);
		assert!(requests.iter().all(|r| r["params"].get("includeTurns").is_none()));
	}

	#[tokio::test]
	async fn recovery_rejects_missing_baseline_and_duplicate_turns() {
		for entries in [json!([{"id":"new"}]), json!([{"id":"new"},{"id":"new"}])] {
			let (result, _) = run(
				vec![
					("thread/read", metadata()),
					("thread/turns/list", json!({"data":entries,"nextCursor":null})),
				],
				Some(Some("missing")),
			)
			.await;
			assert!(matches!(result, Err(ClientError::InvalidFrame)));
		}
	}

	#[tokio::test]
	async fn recovery_without_baseline_reads_all_pages_and_legacy_remains_supported() {
		let (result, _) = run(
			vec![
				("thread/read", metadata()),
				(
					"thread/turns/list",
					json!({"data":[{"id":"two"},{"id":"one"}],"nextCursor":null}),
				),
			],
			Some(None),
		)
		.await;
		assert_eq!(result.unwrap(), json!([{"id":"one"},{"id":"two"}]));
		let legacy =
			json!({"thread":{"id":"thread/opaque","turns":[{"id":"baseline"},{"id":"new"}]}});
		let (result, _) = run(
			vec![("thread/read", legacy.clone()), ("thread/read", legacy)],
			Some(Some("baseline")),
		)
		.await;
		assert_eq!(result.unwrap(), json!([{"id":"new"}]));
	}

	#[tokio::test]
	async fn voice_baseline_reads_only_latest_header_and_accepts_empty_thread() {
		for (data, expected) in
			[(json!([{"id":"latest"}]), json!("latest")), (json!([]), Value::Null)]
		{
			let (result, requests) = run(
				vec![
					("thread/read", metadata()),
					("thread/turns/list", json!({"data":data,"nextCursor":null})),
				],
				Some(Some("__latest_test__")),
			)
			.await;
			assert_eq!(result.unwrap(), expected);
			assert_eq!(requests[1]["params"]["limit"], 1);
			assert_eq!(requests[1]["params"]["itemsView"], "notLoaded");
			assert!(requests.iter().all(|r| r["params"].get("includeTurns").is_none()));
		}
	}

	#[test]
	fn malformed_and_unbounded_pages_never_look_complete() {
		let mut pages = Pages::default();
		assert!(pages.advance(&json!({})).is_err());
		assert!(pages.advance(&json!({"nextCursor":42})).is_err());
		for index in 0..MAX_PAGES - 1 {
			assert!(pages.advance(&json!({"nextCursor":index.to_string()})).unwrap());
		}
		assert!(matches!(
			pages.advance(&json!({"nextCursor":"last"})),
			Err(ClientError::CapacityExceeded)
		));
		assert!(matches!(
			charge(&json!({"data":["large"]}), &mut 1),
			Err(ClientError::CapacityExceeded)
		));
	}
}
