//! Native conversation receipts. No local receipt database or automatic acknowledgement.
use crate::{
	agent_host::AgentHostError::{self, Rejected, Unknown},
	agent_usage_estimate::Source,
};
use decodex_codex::app_server_client::ClientError;
use decodex_protocol::{AgentReadStateResult, EntityId, WireText};
use sha2::{Digest as _, Sha256};
use std::future::Future;

pub(crate) async fn read<F, Fut>(source: F, thread: &str) -> AgentReadStateResult
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	let Some(before) = source().await else {
		return AgentReadStateResult::Unavailable;
	};
	// Read receipts apply to the root user conversation, never a native child.
	if before.key.thread != thread {
		return AgentReadStateResult::Unavailable;
	}
	let result = before.client.thread_read_state(thread).await;
	if source().await.is_none_or(|after| after.key != before.key) {
		return AgentReadStateResult::Unavailable;
	}
	let Ok(Some(state)) = result else {
		return AgentReadStateResult::Unavailable;
	};
	let (Ok(work_id), Ok(thread_id), Ok(revision), Ok(review_token)) = (
		EntityId::new(before.key.work.clone()),
		EntityId::new(thread.to_owned()),
		WireText::new(state.revision.clone()),
		WireText::new(token(&before, &state.revision)),
	) else {
		return AgentReadStateResult::Unavailable;
	};
	let Ok(first_unread) =
		serde_json::to_value(state.first_unread).and_then(serde_json::from_value)
	else {
		return AgentReadStateResult::Unavailable;
	};
	AgentReadStateResult::Available { work_id, thread_id, first_unread, revision, review_token }
}
pub(crate) async fn write<F, Fut>(
	source: F,
	thread: &str,
	revision: &str,
	review: &str,
	read: bool,
) -> Result<(), AgentHostError>
where
	F: Fn() -> Fut,
	Fut: Future<Output = Option<Source>>,
{
	let before = source().await.ok_or(Rejected("The conversation source is unavailable."))?;
	if before.key.thread != thread || token(&before, revision) != review {
		return Err(Rejected("The conversation source changed. Refresh its read state."));
	}
	let guard = before
		.client
		.history_guard(before.key.history_revision)
		.ok_or(Rejected("The conversation changed. Refresh its read state."))?;
	if source().await.is_none_or(|after| after.key != before.key) {
		return Err(Rejected("The conversation source changed. Refresh its read state."));
	}
	before.client.update_thread_read_state(thread, revision, read, guard).await.map_err(
		|error| match error {
			ClientError::Remote(_) | ClientError::StaleHistory => Rejected(
				"The native receipt changed or the mark was rejected. Refresh before trying again.",
			),
			_ => Unknown("The native mark could not be confirmed. Refresh before trying again."),
		},
	)?;
	Ok(())
}
fn token(source: &Source, revision: &str) -> String {
	let value = serde_json::json!([
		format!("{:?}", source.key),
		source.client.connection_identity(),
		revision
	]);
	Sha256::digest(value.to_string().as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::agent_usage_estimate::SourceKey;
	use decodex_codex::app_server_client::AppServerClient;
	use decodex_core::{AccountId, ProcessGenerationId};
	use std::sync::atomic::{AtomicBool, Ordering};
	use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
	#[tokio::test]
	async fn read_state_rejects_new_source_and_never_retries_conflict() {
		let (local, remote) = io::duplex(8192);
		let (r, w) = io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let changed = AtomicBool::new(false);
		let source = || async {
			Some(Source {
				client: client.clone(),
				key: SourceKey {
					generation: ProcessGenerationId::new("10000000-0000-4000-8000-000000000001")
						.unwrap(),
					account: AccountId::new("30000000-0000-4000-8000-000000000003").unwrap(),
					revision: i64::from(changed.load(Ordering::SeqCst)),
					history_revision: 0,
					thread: "thread".into(),
					work: "work".into(),
				},
			})
		};
		let server = tokio::spawn(async move {
			let (r, mut w) = io::split(remote);
			let mut lines = BufReader::new(r).lines();
			let read: serde_json::Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(read["method"], "thread/read");
			w.write_all(format!("{}\n",serde_json::json!({"id":read["id"],"result":{"thread":{"id":"thread"},"readState":{"revision":"r1","firstUnread":{"type":"threadStart"}}}})).as_bytes()).await.unwrap();
			let mark: serde_json::Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(mark["method"], "thread/readState/update");
			assert_eq!(mark["params"]["expectedRevision"], "r1");
			w.write_all(format!("{}\n",serde_json::json!({"id":mark["id"],"error":{"code":-32600,"message":"conflict","data":{"reason":"readStateConflict","readState":{"revision":"r2","firstUnread":{"type":"turn","turnId":"new"}}}}})).as_bytes()).await.unwrap();
			assert!(lines.next_line().await.unwrap().is_none(), "no replay");
		});
		let AgentReadStateResult::Available { review_token, revision, .. } =
			read(&source, "thread").await
		else {
			panic!("available");
		};
		changed.store(true, Ordering::SeqCst);
		assert!(matches!(
			write(&source, "thread", revision.as_str(), review_token.as_str(), true).await,
			Err(Rejected(_))
		));
		changed.store(false, Ordering::SeqCst);
		assert!(matches!(
			write(&source, "thread", revision.as_str(), review_token.as_str(), true).await,
			Err(Rejected(_))
		));
		client.close();
		server.await.unwrap();
	}
}
