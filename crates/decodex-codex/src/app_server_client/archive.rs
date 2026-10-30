//! Exact-thread archive membership. Native thread/read has no archived field.
use std::{
	collections::{BTreeSet, HashMap, HashSet},
	mem,
	time::Duration,
};

use serde_json::Value;
use tokio::time;

use crate::app_server_client::{AppServerClient, ClientError, MAX_FRAME_BYTES};

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
	/// Observe only the requested bindings, stopping when every identity is found.
	/// Positive observations survive a later page failure or deadline. Each page
	/// has a deadline so a progressing large catalog can finish. An absent
	/// identity has no observation after an incomplete scan and must keep its
	/// previous visibility. Only two complete lists establish NotFound.
	pub async fn observe_thread_archives(
		&self,
		threads: &HashSet<String>,
	) -> HashMap<String, ThreadArchiveState> {
		let mut pending = threads.clone();
		let mut observed = HashMap::new();

		if pending.is_empty() || pending.iter().any(|id| valid_id(id).is_err()) {
			return observed;
		}

		let (directories, fork_sources) = self.archive_scope(threads).await;
		let complete_metadata = directories.is_some();
		let mut budget = MAX_FRAME_BYTES;
		let scan = async {
			// Most retained work is active. Do not scan unrelated archives once
			// the active list has supplied every requested identity.
			for archived in [false, true] {
				self.visit_archive_catalog(
					directories.clone(),
					None,
					archived,
					&mut budget,
					|id| {
						if pending.remove(id) {
							observed.insert(
								id.to_owned(),
								if archived {
									ThreadArchiveState::Archived
								} else {
									ThreadArchiveState::Active
								},
							);
						}

						pending.is_empty()
					},
				)
				.await?;

				if pending.is_empty() {
					break;
				}
			}

			Ok::<_, ClientError>(())
		};

		if scan.await.is_ok() {
			for id in pending {
				// Attached forks are deliberately absent from ordinary native catalogs.
				// Inspect their exact source before reporting a complete absence.
				let state = match fork_sources.get(&id) {
					Some(parent) => self.attached_thread_archive_state(&id, parent).await.ok(),
					None if complete_metadata => Some(ThreadArchiveState::NotFound),
					None => self.thread_archive_state(&id).await.ok(),
				};

				if let Some(state) = state {
					observed.insert(id, state);
				}
			}
		}

		observed
	}

	async fn archive_scope(
		&self,
		threads: &HashSet<String>,
	) -> (Option<Value>, HashMap<String, String>) {
		let mut directories = BTreeSet::new();
		let mut forks = HashMap::new();

		for thread in threads {
			let response = time::timeout(
				Duration::from_secs(8),
				self.thread_read(serde_json::json!({"threadId":thread,"includeTurns":false})),
			)
			.await;

			if let Ok(Ok(response)) = response
				&& let Ok(cwd) = archive_directory(&response, thread)
			{
				directories.insert(cwd.to_owned());

				if let Some(parent) = response["thread"]["forkedFromId"].as_str() {
					forks.insert(thread.clone(), parent.to_owned());
				}

				continue;
			}

			return (None, forks);
		}

		// Native metadata supplies the scope. If a binding cannot be read, inspect
		// the full catalogs so absence is never inferred from a stale local folder.
		(Some(serde_json::json!(directories)), forks)
	}

	/// Inspect both lists with explicit provider/source filters and bounded pagination.
	/// An incomplete scan is an error, never evidence that a thread is absent.
	pub async fn thread_archive_state(
		&self,
		thread: &str,
	) -> Result<ThreadArchiveState, ClientError> {
		let (state, parent) = self.archive_state_for(thread, None).await?;

		if state == ThreadArchiveState::NotFound
			&& let Some(parent) = parent
		{
			return self.attached_thread_archive_state(thread, &parent).await;
		}

		Ok(state)
	}

	/// Observe a fork only through its exact native attachment source, including empty forks.
	pub async fn attached_thread_archive_state(
		&self,
		thread: &str,
		parent: &str,
	) -> Result<ThreadArchiveState, ClientError> {
		valid_id(parent)?;

		self.archive_state_for(thread, Some(parent)).await.map(|(state, _)| state)
	}

	async fn archive_state_for(
		&self,
		thread: &str,
		attached_to: Option<&str>,
	) -> Result<(ThreadArchiveState, Option<String>), ClientError> {
		valid_id(thread)?;

		time::timeout(Duration::from_secs(8), async {
			let before = self
				.thread_read(serde_json::json!({"threadId":thread,"includeTurns":false}))
				.await?;
			let cwd = archive_directory(&before, thread)?;
			let parent = before["thread"]["forkedFromId"].as_str();

			if attached_to.is_some_and(|source| parent != Some(source)) {
				return Err(ClientError::InvalidFrame);
			}

			let mut budget = MAX_FRAME_BYTES;
			let archived =
				self.archive_membership(thread, cwd, attached_to, true, &mut budget).await?;
			let active =
				self.archive_membership(thread, cwd, attached_to, false, &mut budget).await?;
			let after = self
				.thread_read(serde_json::json!({"threadId":thread,"includeTurns":false}))
				.await?;

			if archive_directory(&after, thread)? != cwd
				|| after["thread"]["forkedFromId"] != before["thread"]["forkedFromId"]
			{
				return Ok((ThreadArchiveState::Changed, None));
			}

			Ok((
				match (archived, active) {
					(false, true) => ThreadArchiveState::Active,
					(true, false) => ThreadArchiveState::Archived,
					(false, false) => ThreadArchiveState::NotFound,
					(true, true) => ThreadArchiveState::Changed,
				},
				parent.map(str::to_owned),
			))
		})
		.await
		.map_err(|_| ClientError::Io)?
	}

	async fn archive_membership(
		&self,
		thread: &str,
		cwd: &str,
		attached_to: Option<&str>,
		archived: bool,
		budget: &mut usize,
	) -> Result<bool, ClientError> {
		Ok(self
			.archive_catalog(Some(cwd), attached_to, archived, Some(thread), budget)
			.await?
			.contains(thread))
	}

	async fn archive_catalog(
		&self,
		cwd: Option<&str>,
		attached_to: Option<&str>,
		archived: bool,
		target: Option<&str>,
		budget: &mut usize,
	) -> Result<HashSet<String>, ClientError> {
		let mut ids = HashSet::new();

		self.visit_archive_catalog(cwd.map(Value::from), attached_to, archived, budget, |id| {
			ids.insert(id.to_owned());

			target == Some(id)
		})
		.await?;

		Ok(ids)
	}

	async fn visit_archive_catalog(
		&self,
		cwd: Option<Value>,
		attached_to: Option<&str>,
		archived: bool,
		budget: &mut usize,
		mut observe: impl FnMut(&str) -> bool,
	) -> Result<(), ClientError> {
		let mut cursor: Option<String> = None;
		let mut cursors = HashSet::new();
		let mut ids = HashSet::new();

		loop {
			let mut params = serde_json::json!({
				"archived":archived,"cursor":cursor,"limit":100,"modelProviders":[],
				"useStateDbOnly":true,
				"sortKey":"updated_at",
				"sourceKinds":["cli","vscode","exec","appServer","subAgent","subAgentReview","subAgentCompact","subAgentThreadSpawn","subAgentOther","unknown"]
			});

			if let Some(cwd) = &cwd {
				params["cwd"] = cwd.clone();
			}
			if let Some(parent) = attached_to {
				params["attachedToThreadId"] = Value::from(parent);
			}

			let page = time::timeout(Duration::from_secs(8), self.request("thread/list", params))
				.await
				.map_err(|_| ClientError::Io)??;
			let data = page["data"].as_array().ok_or(ClientError::InvalidFrame)?;

			if data.len() > 100 {
				return Err(ClientError::InvalidFrame);
			}

			let mut page_ids = Vec::with_capacity(data.len());

			for item in data {
				let id = item["id"].as_str().ok_or(ClientError::InvalidFrame)?;

				valid_id(id)?;

				// The transport bounds each response. Bound retained identities here;
				// unrelated previews must not cap the number of archive pages.
				*budget = budget
					.checked_sub(id.len() + mem::size_of::<String>())
					.ok_or(ClientError::CapacityExceeded)?;

				if !ids.insert(id.to_owned()) {
					return Err(ClientError::InvalidFrame);
				}

				page_ids.push(id);
			}

			let next = match page.get("nextCursor") {
				Some(Value::Null) => None,
				Some(Value::String(next))
					if !next.is_empty() && next.len() <= 4_096 && cursors.insert(next.clone()) =>
					Some(next.clone()),
				_ => return Err(ClientError::InvalidFrame),
			};

			for id in page_ids {
				if observe(id) {
					return Ok(());
				}
			}

			if next.is_none() {
				return Ok(());
			}

			*budget = budget
				.checked_sub(
					next.as_ref().map_or(0, |cursor| cursor.len() + mem::size_of::<String>()),
				)
				.ok_or(ClientError::CapacityExceeded)?;
			cursor = next;
		}
	}

	/// Submit one explicit restore. Native unarchive is not idempotent: callers must
	/// inspect desired state before submission and reconcile it after uncertain replies.
	pub async fn thread_unarchive(&self, thread: &str) -> Result<(), ClientError> {
		valid_id(thread)?;

		let response = time::timeout(
			Duration::from_secs(8),
			self.request("thread/unarchive", serde_json::json!({"threadId":thread})),
		)
		.await
		.map_err(|_| ClientError::Io)??;

		if response["thread"]["id"].as_str() != Some(thread) {
			return Err(ClientError::InvalidFrame);
		}

		Ok(())
	}

	/// Reconcile an explicit deletion of a previously observed archived thread.
	/// A failed read alone proves nothing; both unfiltered catalogs must be complete.
	pub async fn archived_thread_absent(&self, thread: &str) -> Result<bool, ClientError> {
		valid_id(thread)?;
		if time::timeout(
			Duration::from_secs(8),
			self.thread_read(serde_json::json!({"threadId":thread,"includeTurns":false})),
		)
		.await
		.map_err(|_| ClientError::Io)?
		.is_ok()
		{
			return Ok(false);
		}
		let mut budget = MAX_FRAME_BYTES;
		for archived in [false, true] {
			if self.archive_catalog(None, None, archived, None, &mut budget).await?.contains(thread)
			{
				return Ok(false);
			}
		}
		Ok(true)
	}

	/// Delete one native root and let Codex enforce descendant and ownership rules.
	pub async fn thread_delete(&self, thread: &str) -> Result<(), ClientError> {
		valid_id(thread)?;
		let response = time::timeout(
			Duration::from_secs(8),
			self.request("thread/delete", serde_json::json!({"threadId":thread})),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		if !response.as_object().is_some_and(serde_json::Map::is_empty) {
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
		.filter(|cwd| !cwd.is_empty() && cwd.len() <= 16_384)
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
	use tokio::{
		io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
		time,
	};

	use crate::app_server_client::archive::{
		AppServerClient, ClientError, ThreadArchiveState, Value,
	};

	fn page(ids: &[&str], next: Value) -> Value {
		serde_json::json!({"data":ids.iter().map(|id|serde_json::json!({"id":id})).collect::<Vec<_>>(),"nextCursor":next})
	}

	#[tokio::test]
	async fn attached_archive_observation_uses_exact_source_and_keeps_failed_reads_unknown() {
		for mode in 0..4 {
			let (local, remote) = io::duplex(65_536);
			let (r, w) = io::split(local);
			let (client, _events) = AppServerClient::from_io(r, w);
			let server = tokio::spawn(async move {
				let (r, mut w) = io::split(remote);
				let mut lines = BufReader::new(r).lines();
				let mut reads = 0;

				while let Some(line) = lines.next_line().await.unwrap() {
					let request: Value = serde_json::from_str(&line).unwrap();

					if request["method"] == "thread/read" {
						reads += 1;

						if mode == 3 || mode == 2 && reads == 1 {
							w.write_all(
								format!(
									"{}\n",
									serde_json::json!({"id":request["id"],"error":{"code":-32_600,"message":"unavailable"}})
								)
								.as_bytes(),
							)
							.await
							.unwrap();

							continue;
						}
					}

					let result = match request["method"].as_str().unwrap() {
						"thread/read" =>
							serde_json::json!({"thread":{"id":"fork","cwd":"/workspace","forkedFromId":"source"}}),
						"thread/list" =>
							if !request["params"]["attachedToThreadId"].is_null() {
								assert_eq!(request["params"]["attachedToThreadId"], "source");
								assert!(request["params"].get("ancestorThreadId").is_none());

								if mode == 1 {
									return;
								}
								if request["params"]["archived"] == true {
									page(&["fork"], Value::Null)
								} else {
									page(&[], Value::Null)
								}
							} else {
								page(&[], Value::Null)
							},
						_ => panic!("unexpected mutation"),
					};

					w.write_all(
						format!("{}\n", serde_json::json!({"id":request["id"],"result":result}))
							.as_bytes(),
					)
					.await
					.unwrap();
				}
			});
			let observed = client
				.observe_thread_archives(&std::collections::HashSet::from(["fork".into()]))
				.await;

			assert_eq!(
				observed.get("fork"),
				if matches!(mode, 1 | 3) { None } else { Some(&ThreadArchiveState::Archived) }
			);

			client.close();
			server.await.unwrap();
		}
	}

	async fn next_catalog_request(
		lines: &mut tokio::io::Lines<impl tokio::io::AsyncBufRead + Unpin>,
		writer: &mut (impl tokio::io::AsyncWrite + Unpin),
	) -> Value {
		loop {
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

			if request["method"] != "thread/read" {
				assert_eq!(request["params"]["cwd"], serde_json::json!(["/workspace"]));

				return request;
			}

			assert_eq!(request["params"]["includeTurns"], false);

			let response = serde_json::json!({"id":request["id"],"result":{"thread":{"id":request["params"]["threadId"],"cwd":"/workspace"}}});

			writer.write_all(format!("{response}\n").as_bytes()).await.unwrap();
		}
	}

	#[tokio::test]
	async fn observes_external_archive_and_restore_on_the_same_connection() {
		let (local, remote) = io::duplex(65_536);
		let (r, w) = io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let server = tokio::spawn(async move {
			let (r, mut w) = io::split(remote);
			let mut lines = BufReader::new(r).lines();

			for (archived, response) in [
				(false, page(&["root", "target"], serde_json::json!("unrelated-active-page"))),
				(false, page(&["root"], Value::Null)),
				(true, page(&["target"], serde_json::json!("unrelated-archive-page"))),
				(false, page(&["root", "target"], serde_json::json!("unrelated-active-page"))),
			] {
				let request = next_catalog_request(&mut lines, &mut w).await;

				assert_eq!(request["method"], "thread/list");
				assert_eq!(request["params"]["archived"], archived);
				assert_eq!(request["params"]["sortKey"], "updated_at");
				assert_eq!(request["params"]["useStateDbOnly"], true);
				assert!(request["params"].get("projectId").is_none());

				w.write_all(
					format!("{}\n", serde_json::json!({"id":request["id"],"result":response}))
						.as_bytes(),
				)
				.await
				.unwrap();
			}
		});
		let threads = std::collections::HashSet::from(["root".to_owned(), "target".to_owned()]);

		for expected in
			[ThreadArchiveState::Active, ThreadArchiveState::Archived, ThreadArchiveState::Active]
		{
			let observed = client.observe_thread_archives(&threads).await;

			assert_eq!(observed.get("root"), Some(&ThreadArchiveState::Active));
			assert_eq!(observed.get("target"), Some(&expected));
			assert_eq!(observed.len(), 2);
		}

		client.close();
		server.await.unwrap();
	}

	#[tokio::test]
	async fn progressing_archive_lists_confirm_absent_bindings_after_eight_seconds() {
		let (local, remote) = io::duplex(65_536);
		let (r, w) = io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let server = tokio::spawn(async move {
			let (r, mut w) = io::split(remote);
			let mut lines = BufReader::new(r).lines();

			for (archived, ids, next) in [
				(false, vec!["active"], Value::Null),
				(true, vec!["archived"], serde_json::json!("last")),
				(true, vec![], Value::Null),
			] {
				let request = next_catalog_request(&mut lines, &mut w).await;

				assert_eq!(request["params"]["archived"], archived);

				// Every page progresses within its deadline, but the catalog takes nine seconds.
				time::sleep(std::time::Duration::from_secs(3)).await;

				w.write_all(
					format!(
						"{}\n",
						serde_json::json!({"id":request["id"],"result":page(&ids, next)})
					)
					.as_bytes(),
				)
				.await
				.unwrap();
			}
		});
		let observed = client
			.observe_thread_archives(&std::collections::HashSet::from([
				"active".into(),
				"archived".into(),
				"missing".into(),
			]))
			.await;

		assert_eq!(
			observed,
			std::collections::HashMap::from([
				("active".into(), ThreadArchiveState::Active),
				("archived".into(), ThreadArchiveState::Archived),
				("missing".into(), ThreadArchiveState::NotFound),
			])
		);

		client.close();
		server.await.unwrap();
	}

	#[tokio::test]
	async fn failed_later_page_preserves_positive_observations_without_inferring_missing_state() {
		let (local, remote) = io::duplex(65_536);
		let (r, w) = io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let server = tokio::spawn(async move {
			let (r, mut w) = io::split(remote);
			let mut lines = BufReader::new(r).lines();
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

			assert_eq!(request["method"], "thread/read");

			w.write_all(
				format!(
					"{}\n",
					serde_json::json!({"id":request["id"],"error":{"code":-32_600,"message":"unavailable"}})
				)
				.as_bytes(),
			)
			.await
			.unwrap();

			for response in [
				page(&["active"], serde_json::json!("next")),
				serde_json::json!({"data":null,"nextCursor":null}),
			] {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

				assert_eq!(request["method"], "thread/list");
				assert!(
					request["params"].get("cwd").is_none(),
					"unknown metadata requires the complete catalogs"
				);

				w.write_all(
					format!("{}\n", serde_json::json!({"id":request["id"],"result":response}))
						.as_bytes(),
				)
				.await
				.unwrap();
			}
		});
		let observed = client
			.observe_thread_archives(&std::collections::HashSet::from([
				"active".to_owned(),
				"missing".to_owned(),
			]))
			.await;

		assert_eq!(
			observed,
			std::collections::HashMap::from([("active".to_owned(), ThreadArchiveState::Active)])
		);

		client.close();
		server.await.unwrap();
	}

	async fn inspect_catalog(
		pages: Vec<Value>,
	) -> Result<std::collections::HashSet<String>, ClientError> {
		let (local, remote) = io::duplex(65_536);
		let (r, w) = io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let server = tokio::spawn(async move {
			let (r, mut w) = io::split(remote);
			let mut lines = BufReader::new(r).lines();

			for page in pages {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

				assert_eq!(request["method"], "thread/list");
				assert_eq!(request["params"]["archived"], true);
				assert_eq!(request["params"]["useStateDbOnly"], true);
				assert_eq!(request["params"]["sourceKinds"].as_array().unwrap().len(), 10);
				assert!(
					request["params"].get("projectId").is_none(),
					"unassigned threads must be included"
				);

				w.write_all(
					format!("{}\n", serde_json::json!({"id":request["id"],"result":page}))
						.as_bytes(),
				)
				.await
				.unwrap();
			}
		});
		let mut budget = super::MAX_FRAME_BYTES;
		let result = client.archive_catalog(None, None, true, None, &mut budget).await;

		client.close();
		server.await.unwrap();

		result
	}

	#[tokio::test]
	async fn archive_catalog_reads_all_pages_and_rejects_partial_results() {
		let result = inspect_catalog(vec![
			page(&["first"], serde_json::json!("next")),
			page(&["second"], Value::Null),
		])
		.await
		.unwrap();

		assert_eq!(result, std::collections::HashSet::from(["first".into(), "second".into()]));

		for pages in [
			vec![page(&["first"], serde_json::json!("next"))],
			vec![
				page(&["first"], serde_json::json!("loop")),
				page(&["second"], serde_json::json!("loop")),
			],
		] {
			assert!(inspect_catalog(pages).await.is_err());
		}
	}

	#[tokio::test]
	async fn large_archive_is_not_truncated_by_page_count_or_unrelated_preview_bytes() {
		let pages = (0..103).map(|page| serde_json::json!({
			"data": (0..100).map(|item| serde_json::json!({
				"id": format!("thread-{page}-{item}"),
				"preview": "x".repeat(900)
			})).collect::<Vec<_>>(),
			"nextCursor": if page == 102 { Value::Null } else { serde_json::json!(format!("page-{}", page + 1)) }
		})).collect();
		let archived = inspect_catalog(pages).await.unwrap();

		assert_eq!(archived.len(), 10_300);
		assert!(archived.contains("thread-102-99"));
	}

	async fn inspect(pages: Vec<Value>) -> Result<ThreadArchiveState, ClientError> {
		inspect_with_directories(pages, "/fixture", "/fixture").await
	}

	async fn inspect_with_directories(
		pages: Vec<Value>,
		before: &'static str,
		after: &'static str,
	) -> Result<ThreadArchiveState, ClientError> {
		let (local, remote) = io::duplex(65_536);
		let (r, w) = io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let server = tokio::spawn(async move {
			let (r, mut w) = io::split(remote);
			let mut lines = BufReader::new(r).lines();
			let mut pages = std::collections::VecDeque::from(pages);
			let mut reads = 0;

			while let Some(line) = lines.next_line().await.unwrap() {
				let request: Value = serde_json::from_str(&line).unwrap();
				let page = if request["method"] == "thread/read" {
					assert_eq!(request["params"]["threadId"], "target");
					assert_eq!(request["params"]["includeTurns"], false);

					reads += 1;

					serde_json::json!({"thread":{"id":"target","cwd":if reads==1 {before} else {after}}})
				} else {
					assert_eq!(request["method"], "thread/list");
					assert_eq!(request["params"]["modelProviders"], serde_json::json!([]));
					assert_eq!(request["params"]["sourceKinds"].as_array().unwrap().len(), 10);
					assert_eq!(request["params"]["limit"], 100);
					assert_eq!(request["params"]["cwd"], before);
					assert_eq!(request["params"]["useStateDbOnly"], true);

					{
						let Some(page) = pages.pop_front() else { break };

						page
					}
				};

				w.write_all(
					format!("{}\n", serde_json::json!({"id":request["id"],"result":page}))
						.as_bytes(),
				)
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
				page(&["first"], serde_json::json!("next")),
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
			vec![serde_json::json!({"data":[]})],
			vec![page(&[], serde_json::json!(""))],
			vec![page(&["same", "same"], Value::Null)],
			vec![
				page(&["first"], serde_json::json!("loop")),
				page(&["second"], serde_json::json!("loop")),
			],
			vec![
				page(&["target"], Value::Null),
				serde_json::json!({"data":null,"nextCursor":null}),
			],
		] {
			assert!(matches!(inspect(pages).await, Err(ClientError::InvalidFrame)));
		}

		assert!(matches!(inspect(vec![]).await, Err(ClientError::Closed)));
	}

	#[tokio::test]
	async fn unarchive_is_one_exact_mutation_and_rejects_a_foreign_reply() {
		let (local, remote) = io::duplex(65_536);
		let (r, w) = io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let server = tokio::spawn(async move {
			let (r, mut w) = io::split(remote);
			let mut lines = BufReader::new(r).lines();
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

			assert_eq!(request["method"], "thread/unarchive");
			assert_eq!(request["params"], serde_json::json!({"threadId":"target"}));

			w.write_all(
				format!(
					"{}\n",
					serde_json::json!({"id":request["id"],"result":{"thread":{"id":"foreign"}}})
				)
				.as_bytes(),
			)
			.await
			.unwrap();
		});

		assert!(matches!(client.thread_unarchive("target").await, Err(ClientError::InvalidFrame)));

		server.await.unwrap();
	}
	#[tokio::test]
	async fn deletion_uses_native_empty_response_and_rejects_malformed_success() {
		for result in
			[serde_json::json!({}), serde_json::json!({"threadId":"foreign"}), Value::Null]
		{
			let (local, remote) = io::duplex(65_536);
			let (r, w) = io::split(local);
			let (client, _events) = AppServerClient::from_io(r, w);
			let expected = result.is_object() && result.as_object().unwrap().is_empty();
			let server = tokio::spawn(async move {
				let (r, mut w) = io::split(remote);
				let mut lines = BufReader::new(r).lines();
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], "thread/delete");
				assert_eq!(request["params"], serde_json::json!({"threadId":"target"}));
				w.write_all(
					format!("{}\n", serde_json::json!({"id":request["id"],"result":result}))
						.as_bytes(),
				)
				.await
				.unwrap();
			});
			assert_eq!(client.thread_delete("target").await.is_ok(), expected);
			server.await.unwrap();
		}
	}
}
