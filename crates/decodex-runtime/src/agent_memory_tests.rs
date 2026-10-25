use std::{
	future,
	sync::atomic::{AtomicUsize, Ordering},
};

use serde_json::Value;
use tokio::{
	io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
	runtime::Builder,
};

use crate::{
	agent_memory::{self, AgentMemoryResult, EntityId, Source, WireText},
	agent_usage_estimate::SourceKey,
};
use decodex_codex::app_server_client::AppServerClient;
use decodex_core::{AccountId, ProcessGenerationId};

fn source(client: AppServerClient, revision: i64) -> Source {
	Source {
		client,
		key: SourceKey {
			generation: ProcessGenerationId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			account: AccountId::new("30000000-0000-4000-8000-000000000003").unwrap(),
			revision,
			history_revision: 0,
			thread: "thread".into(),
			work: "agent".into(),
		},
	}
}

#[test]
fn memory_pages_keep_unicode_and_reject_content_or_source_changes() {
	let (local, _remote) = io::duplex(4_096);
	let (reader, writer) = io::split(local);
	let runtime = Builder::new_current_thread().enable_all().build().unwrap();
	let _guard = runtime.enter();
	let (client, _) = AppServerClient::from_io(reader, writer);
	let s = source(client.clone(), 1);
	let text = "界".repeat(5_000);
	let AgentMemoryResult::Page { digest, text: first, next_offset: Some(next), .. } =
		agent_memory::page(
			&s,
			EntityId::new("agent").unwrap(),
			WireText::new("thread").unwrap(),
			&text,
			None,
			0,
		)
		.unwrap()
	else {
		panic!("first memory page")
	};
	let AgentMemoryResult::Page { text: second, next_offset: None, .. } = agent_memory::page(
		&s,
		EntityId::new("agent").unwrap(),
		WireText::new("thread").unwrap(),
		&text,
		Some(&digest),
		next,
	)
	.unwrap() else {
		panic!("second memory page")
	};

	assert_eq!(first + &second, text);
	assert!(
		agent_memory::page(
			&s,
			EntityId::new("agent").unwrap(),
			WireText::new("thread").unwrap(),
			&text,
			None,
			next
		)
		.is_none()
	);
	assert!(
		agent_memory::page(
			&s,
			EntityId::new("agent").unwrap(),
			WireText::new("thread").unwrap(),
			&(text.clone() + "changed"),
			Some(&digest),
			next
		)
		.is_none()
	);
	assert!(
		agent_memory::page(
			&source(client, 2),
			EntityId::new("agent").unwrap(),
			WireText::new("thread").unwrap(),
			&text,
			Some(&digest),
			next
		)
		.is_none()
	);
}

#[tokio::test]
async fn native_memory_reads_only_once_and_discards_a_changed_owner() {
	for changed in [false, true] {
		let (local, remote) = io::duplex(4_096);
		let (reader, writer) = io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

			assert_eq!(request["method"], "thread/memory/read");
			assert_eq!(request["params"], serde_json::json!({"threadId":"thread"}));

			writer
				.write_all(
					format!(
						"{}\n",
						serde_json::json!({"id":request["id"],"result":{"summary":"Native memory"}})
					)
					.as_bytes(),
				)
				.await
				.unwrap();
		});
		let calls = AtomicUsize::new(0);
		let result = agent_memory::read(
			|| {
				future::ready(Some(source(
					client.clone(),
					1 + i64::from(changed && calls.fetch_add(1, Ordering::SeqCst) > 0),
				)))
			},
			None,
			0,
		)
		.await;

		server.await.unwrap();

		if changed {
			assert_eq!(result, AgentMemoryResult::Unavailable);
		} else {
			assert!(matches!(result,AgentMemoryResult::Page {text,..} if text=="Native memory"));
		}
	}
}
