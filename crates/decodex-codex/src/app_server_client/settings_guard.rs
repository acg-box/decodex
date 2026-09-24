//! Track only settings sources with live read-to-write guards.
use std::{
	collections::HashMap,
	sync::{
		Arc, Mutex, Weak,
		atomic::{AtomicU64, Ordering},
	},
};

#[derive(Clone, Default)]
pub(super) struct SettingsRevisions(Arc<Mutex<HashMap<String, Weak<AtomicU64>>>>);

#[derive(Clone)]
pub(super) struct SettingsGuard {
	counter: Arc<AtomicU64>,
	revision: u64,
}

impl SettingsGuard {
	pub(super) fn is_live(&self) -> bool {
		self.revision != u64::MAX && self.counter.load(Ordering::Acquire) == self.revision
	}
}

impl SettingsRevisions {
	pub(super) fn capture(&self, thread: &str) -> Option<SettingsGuard> {
		if thread.is_empty() || thread.len() > 512 || thread.chars().any(char::is_control) {
			return None;
		}
		let mut rows = self.0.lock().ok()?;
		rows.retain(|_, value| value.strong_count() > 0);
		let counter = match rows.get(thread).and_then(Weak::upgrade) {
			Some(counter) => counter,
			None => {
				if rows.len() >= 256 {
					return None;
				}
				let counter = Arc::new(AtomicU64::new(0));
				rows.insert(thread.into(), Arc::downgrade(&counter));
				counter
			},
		};
		Some(SettingsGuard { revision: counter.load(Ordering::Acquire), counter })
	}

	pub(super) fn invalidate(&self, thread: &str) {
		if let Ok(rows) = self.0.lock()
			&& let Some(counter) = rows.get(thread).and_then(Weak::upgrade)
		{
			let _ = counter.fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
				Some(value.saturating_add(1))
			});
		}
	}

	pub(super) fn clear(&self) {
		if let Ok(mut rows) = self.0.lock() {
			for counter in rows.values().filter_map(Weak::upgrade) {
				counter.store(u64::MAX, Ordering::Release);
			}
			rows.clear();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn source_changes_invalidate_live_guards_and_dead_sources_do_not_accumulate() {
		let revisions = SettingsRevisions::default();
		let first = revisions.capture("one").unwrap();
		let second = revisions.capture("two").unwrap();
		revisions.invalidate("one");
		assert!(!first.is_live());
		assert!(second.is_live());
		let next = revisions.capture("one").unwrap();
		assert!(next.is_live());
		revisions.clear();
		assert!(!next.is_live());
		assert!(!second.is_live());
		for n in 0..1000 {
			assert!(revisions.capture(&n.to_string()).unwrap().is_live());
		}
		assert!(revisions.0.lock().unwrap().len() <= 1);
	}
}

#[cfg(test)]
mod transport_tests {
	use crate::app_server_client::{AppServerClient, ClientError};
	use serde_json::{Value, json};
	use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	#[tokio::test]
	async fn queued_settings_notification_prevents_retry_write_without_owner_processing() {
		let (local, remote) = tokio::io::duplex(8192);
		let (reader, writer) = tokio::io::split(local);
		let (client, mut events) = AppServerClient::from_io(reader, writer);
		let (reader, mut writer) = tokio::io::split(remote);
		let mut lines = BufReader::new(reader).lines();
		let guard = client.thread_settings_guard("task").unwrap();
		let other = client.thread_settings_guard("other").unwrap();
		let read_client = client.clone();
		let read_guard = guard.clone();
		let read =
			tokio::spawn(
				async move { read_client.thread_model_settings("task", read_guard).await },
			);
		let request: Value =
			serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
		writer.write_all(format!("{}\n",json!({"id":request["id"],"result":{"thread":{"id":"task","model":"selected","reasoningEffort":"high"}}})).as_bytes()).await.unwrap();
		assert_eq!(read.await.unwrap().unwrap().unwrap().model.as_deref(), Some("selected"));
		writer.write_all(format!("{}\n",json!({"method":"thread/settings/updated","params":{"threadId":"task","threadSettings":{"model":"new-choice"}}})).as_bytes()).await.unwrap();
		let _unprocessed = events.recv().await.unwrap();
		assert!(other.is_live());
		assert!(!guard.is_live());
		assert!(matches!(
			client
				.request_with_history(
					"turn/start",
					json!({"threadId":"task","model":"selected","input":[]}),
					guard
				)
				.await,
			Err(ClientError::StaleHistory)
		));
		assert!(
			tokio::time::timeout(std::time::Duration::from_millis(20), lines.next_line())
				.await
				.is_err()
		);
	}
}

#[cfg(test)]
mod combined_tests {
	use super::super::{AppServerClient, ServerEvent, server_requests::ServerRequests};
	use serde_json::json;

	#[test]
	fn settings_constraint_retains_question_revision_and_connection_identity() {
		let requests = ServerRequests::default();
		let combined = requests
			.with_thread_settings_guard("task", requests.question_guard(0).unwrap())
			.unwrap();
		requests
			.observe(&ServerEvent::Notification {
				method: "thread/settings/updated".into(),
				params: json!({"threadId":"other"}),
			})
			.unwrap();
		assert!(combined.is_live());
		requests
			.observe(&ServerEvent::Notification {
				method: "item/completed".into(),
				params: json!({"threadId":"task","turnId":"turn","item":{"id":"input","type":"userMessage"}}),
			})
			.unwrap();
		assert!(!combined.is_live());
		assert!(requests.with_thread_settings_guard("task", combined).is_none());
		let combined = requests
			.with_thread_settings_guard("task", requests.question_guard(1).unwrap())
			.unwrap();
		requests
			.observe(&ServerEvent::Notification {
				method: "thread/settings/updated".into(),
				params: json!({"threadId":"task"}),
			})
			.unwrap();
		assert!(!combined.is_live());
		let other = ServerRequests::default();
		assert!(
			requests.with_thread_settings_guard("task", other.question_guard(0).unwrap()).is_none()
		);
	}

	#[tokio::test]
	async fn closed_connection_cannot_combine_guards() {
		let (io, _remote) = tokio::io::duplex(128);
		let (reader, writer) = tokio::io::split(io);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let guard = client.question_guard(0).unwrap();
		client.close();
		assert!(client.with_thread_settings_guard("task", guard).is_none());
	}
}
