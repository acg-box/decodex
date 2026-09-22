//! Exact-thread archive membership. Native thread/read has no archived field.
use super::{AppServerClient, ClientError, MAX_FRAME_BYTES};
use serde_json::{Value, json};
use std::collections::HashSet;

/// Observed native list membership, never inferred from a path or error message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadArchiveState {
	/// Present only in the active list.
	Active,
	/// Present only in the archived list.
	Archived,
	/// Not found in either complete list. This does not prove deletion.
	NotFound,
	/// Appeared in both lists during inspection; another client may have moved it.
	Changed,
}

impl AppServerClient {
	/// Inspect both lists with explicit provider/source filters and bounded pagination.
	/// An incomplete scan is an error, never evidence that a thread is absent.
	pub async fn thread_archive_state(
		&self,
		thread: &str,
	) -> Result<ThreadArchiveState, ClientError> {
		valid_id(thread)?;
		tokio::time::timeout(std::time::Duration::from_secs(8), async {
			let before = self.thread_read(json!({"threadId":thread,"includeTurns":false})).await?;
			let cwd = archive_directory(&before, thread)?;
			let mut budget = MAX_FRAME_BYTES;
			let archived = self.archive_membership(thread, cwd, true, &mut budget).await?;
			let active = self.archive_membership(thread, cwd, false, &mut budget).await?;
			let after = self.thread_read(json!({"threadId":thread,"includeTurns":false})).await?;
			if archive_directory(&after, thread)? != cwd {
				return Ok(ThreadArchiveState::Changed);
			}
			Ok(match (archived, active) {
				(false, true) => ThreadArchiveState::Active,
				(true, false) => ThreadArchiveState::Archived,
				(false, false) => ThreadArchiveState::NotFound,
				(true, true) => ThreadArchiveState::Changed,
			})
		})
		.await
		.map_err(|_| ClientError::Io)?
	}

	async fn archive_membership(
		&self,
		thread: &str,
		cwd: &str,
		archived: bool,
		budget: &mut usize,
	) -> Result<bool, ClientError> {
		let mut cursor: Option<String> = None;
		let mut cursors = HashSet::new();
		let mut ids = HashSet::new();
		for _ in 0..100 {
			let page = self.request("thread/list", json!({
				"archived":archived,"cursor":cursor,"limit":100,"modelProviders":[],"cwd":cwd,
                // Archive membership is native state; do not rescan every rollout on each UI poll.
                "useStateDbOnly":true,
				// An empty sourceKinds list means interactive sources, not all sources.
				"sourceKinds":["cli","vscode","exec","appServer","subAgent","subAgentReview","subAgentCompact","subAgentThreadSpawn","subAgentOther","unknown"]
			})).await?;
			*budget =
				budget.checked_sub(page.to_string().len()).ok_or(ClientError::CapacityExceeded)?;
			let data = page["data"].as_array().ok_or(ClientError::InvalidFrame)?;
			if data.len() > 100 {
				return Err(ClientError::InvalidFrame);
			}
			let mut found = false;
			for item in data {
				let id = item["id"].as_str().ok_or(ClientError::InvalidFrame)?;
				valid_id(id)?;
				if !ids.insert(id.to_owned()) {
					return Err(ClientError::InvalidFrame);
				}
				found |= id == thread;
			}
			let next = match page.get("nextCursor") {
				Some(Value::Null) => None,
				Some(Value::String(next))
					if !next.is_empty() && next.len() <= 4096 && cursors.insert(next.clone()) =>
					Some(next.clone()),
				_ => return Err(ClientError::InvalidFrame),
			};
			if found {
				return Ok(true);
			}
			let Some(next) = next else {
				return Ok(false);
			};
			cursor = Some(next);
		}
		Err(ClientError::CapacityExceeded)
	}

	/// Submit one explicit restore. Native unarchive is not idempotent: callers must
	/// inspect desired state before submission and reconcile it after uncertain replies.
	pub async fn thread_unarchive(&self, thread: &str) -> Result<(), ClientError> {
		valid_id(thread)?;
		let response = tokio::time::timeout(
			std::time::Duration::from_secs(8),
			self.request("thread/unarchive", json!({"threadId":thread})),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		if response["thread"]["id"].as_str() != Some(thread) {
			return Err(ClientError::InvalidFrame);
		}
		Ok(())
	}
}

fn archive_directory<'a>(value: &'a Value, thread: &str) -> Result<&'a str, ClientError> {
	if value["thread"]["id"].as_str() != Some(thread) {
		return Err(ClientError::InvalidFrame);
	}
	value["thread"]["cwd"]
		.as_str()
		.filter(|cwd| !cwd.is_empty() && cwd.len() <= 16384)
		.ok_or(ClientError::InvalidFrame)
}

fn valid_id(id: &str) -> Result<(), ClientError> {
	if id.is_empty() || id.len() > 512 || id.chars().any(char::is_control) {
		Err(ClientError::InvalidFrame)
	} else {
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
	async fn inspect(pages: Vec<Value>) -> Result<ThreadArchiveState, ClientError> {
		inspect_with_directories(pages, "/fixture", "/fixture").await
	}
	async fn inspect_with_directories(
		pages: Vec<Value>,
		before: &'static str,
		after: &'static str,
	) -> Result<ThreadArchiveState, ClientError> {
		let (local, remote) = tokio::io::duplex(65536);
		let (r, w) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let server = tokio::spawn(async move {
			let (r, mut w) = tokio::io::split(remote);
			let mut lines = BufReader::new(r).lines();
			let mut pages = std::collections::VecDeque::from(pages);
			let mut reads = 0;
			while let Some(line) = lines.next_line().await.unwrap() {
				let request: Value = serde_json::from_str(&line).unwrap();
				let page = if request["method"] == "thread/read" {
					assert_eq!(request["params"]["threadId"], "target");
					assert_eq!(request["params"]["includeTurns"], false);
					reads += 1;
					json!({"thread":{"id":"target","cwd":if reads==1 {before} else {after}}})
				} else {
					assert_eq!(request["method"], "thread/list");
					assert_eq!(request["params"]["modelProviders"], json!([]));
					assert_eq!(request["params"]["sourceKinds"].as_array().unwrap().len(), 10);
					assert_eq!(request["params"]["limit"], 100);
					assert_eq!(request["params"]["cwd"], before);
					assert_eq!(request["params"]["useStateDbOnly"], true);
					{
						let Some(page) = pages.pop_front() else { break };
						page
					}
				};
				w.write_all(format!("{}\n", json!({"id":request["id"],"result":page})).as_bytes())
					.await
					.unwrap();
			}
		});
		let result = client.thread_archive_state("target").await;
		client.close();
		server.await.unwrap();
		result
	}
	#[tokio::test]
	async fn moved_directory_is_not_reported_as_missing_or_active() {
		assert_eq!(
			inspect_with_directories(
				vec![page(&[], Value::Null), page(&["target"], Value::Null)],
				"/before",
				"/after"
			)
			.await
			.unwrap(),
			ThreadArchiveState::Changed
		);
	}

	fn page(ids: &[&str], next: Value) -> Value {
		json!({"data":ids.iter().map(|id|json!({"id":id})).collect::<Vec<_>>(),"nextCursor":next})
	}
	#[tokio::test]
	async fn archive_membership_requires_positive_exact_identity_and_complete_absence() {
		for (a, b, want) in [
			(true, false, ThreadArchiveState::Archived),
			(false, true, ThreadArchiveState::Active),
			(false, false, ThreadArchiveState::NotFound),
			(true, true, ThreadArchiveState::Changed),
		] {
			let result = inspect(vec![
				page(if a { &["target"] } else { &["unrelated"] }, Value::Null),
				page(if b { &["target"] } else { &[] }, Value::Null),
			])
			.await
			.unwrap();
			assert_eq!(result, want);
		}
		assert_eq!(
			inspect(vec![
				page(&["first"], json!("next")),
				page(&["target"], Value::Null),
				page(&[], Value::Null)
			])
			.await
			.unwrap(),
			ThreadArchiveState::Archived
		);
	}
	#[tokio::test]
	async fn incomplete_and_malformed_lists_never_become_active_or_missing() {
		for pages in [
			vec![json!({"data":[]})],
			vec![page(&[], json!(""))],
			vec![page(&["same", "same"], Value::Null)],
			vec![page(&["first"], json!("loop")), page(&["second"], json!("loop"))],
			vec![page(&["target"], Value::Null), json!({"data":null,"nextCursor":null})],
		] {
			assert!(matches!(inspect(pages).await, Err(ClientError::InvalidFrame)));
		}
		assert!(matches!(inspect(vec![]).await, Err(ClientError::Closed)));
	}
	#[tokio::test]
	async fn unarchive_is_one_exact_mutation_and_rejects_a_foreign_reply() {
		let (local, remote) = tokio::io::duplex(65536);
		let (r, w) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let server = tokio::spawn(async move {
			let (r, mut w) = tokio::io::split(remote);
			let mut lines = BufReader::new(r).lines();
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], "thread/unarchive");
			assert_eq!(request["params"], json!({"threadId":"target"}));
			w.write_all(
				format!("{}\n", json!({"id":request["id"],"result":{"thread":{"id":"foreign"}}}))
					.as_bytes(),
			)
			.await
			.unwrap();
		});
		assert!(matches!(client.thread_unarchive("target").await, Err(ClientError::InvalidFrame)));
		server.await.unwrap();
	}
}
