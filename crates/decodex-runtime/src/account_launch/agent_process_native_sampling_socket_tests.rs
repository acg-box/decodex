//! Real client, service owner, admitted bridge and native sampler under the shared socket fixture.
use super::{accepted, settled};
use decodex_codex::app_server_client::AppServerClient;
use decodex_protocol::{
	AgentActionDto, AgentClient, AgentCommandResponse, AgentSamplePhase, EntityId, HistoryText,
	IdempotencyKey, WireText,
};
use std::{
	sync::atomic::{AtomicUsize, Ordering},
	time::Duration,
};

pub(super) async fn parent(client: &AgentClient, store: &decodex_database::SqliteStore) -> String {
	if let Ok(thread) = tokio::time::timeout(Duration::from_secs(20), settled(client)).await {
		return thread;
	}
	panic!(
		"sample parent failed to settle: snapshot={:?}; events={:?}",
		client.query().await,
		store.read_agent_work_events("recap-root".into(), 10).await
	);
}

pub(super) async fn check(
	client: &AgentClient,
	native: &AppServerClient,
	work: &EntityId,
	thread: &str,
	requests: &AtomicUsize,
) {
	let before =
		native.thread_markdown_transcript(thread).await.expect("native sample service fixture");
	let calls = requests.load(Ordering::Acquire);
	assert_eq!(
		client.sample(work.clone()).await.expect("native sample service fixture").phase,
		AgentSamplePhase::Idle
	);
	assert_eq!(requests.load(Ordering::Acquire), calls, "opening sample state cannot infer");
	let command = AgentActionDto::GenerateSample {
		work_id: work.clone(),
		thread_id: WireText::new(thread).expect("native sample service fixture"),
		prompt: WireText::new("Give one other perspective without taking actions.")
			.expect("native sample service fixture"),
	};
	accepted(client, command.clone(), "explicit-native-sample").await;
	accepted(client, command, "explicit-native-sample").await;
	let answer = tokio::time::timeout(Duration::from_secs(40), async {
		loop {
			let state = client.sample(work.clone()).await.expect("native sample service fixture");
			assert_eq!(
				state.request_id.as_ref().expect("native sample service fixture").as_str(),
				"explicit-native-sample"
			);
			assert_eq!(
				state.thread_id.as_ref().expect("native sample service fixture").as_str(),
				thread
			);
			assert!(state.is_valid());
			if state.phase != AgentSamplePhase::Pending {
				break state;
			}
			tokio::time::sleep(Duration::from_millis(20)).await;
		}
	})
	.await
	.expect("native sample service fixture");
	assert_eq!(answer.phase, AgentSamplePhase::Ready);
	assert!(answer.answer.expect("native sample service fixture").text.is_some());
	assert_eq!(
		requests.load(Ordering::Acquire),
		calls + 1,
		"idempotent command replay must not infer twice"
	);
	assert_eq!(
		native.thread_markdown_transcript(thread).await.expect("native sample service fixture"),
		before
	);
	let rejected = client
		.execute(
			AgentActionDto::GenerateSample {
				work_id: work.clone(),
				thread_id: WireText::new("foreign-thread").expect("native sample service fixture"),
				prompt: WireText::new("Wrong source").expect("native sample service fixture"),
			},
			IdempotencyKey::new("foreign-source-sample").expect("native sample service fixture"),
		)
		.await
		.expect("native sample service fixture");
	assert!(matches!(rejected, AgentCommandResponse::Rejected { .. }));
	assert_eq!(requests.load(Ordering::Acquire), calls + 1);
	accepted(
		client,
		AgentActionDto::Send {
			root_id: work.clone(),
			text: HistoryText::new("Advance the parent after its separate answer.")
				.expect("native sample service fixture"),
		},
		"sample-parent-next-input",
	)
	.await;
	assert_eq!(settled(client).await, thread);
	let stale = client.sample(work.clone()).await.expect("native sample service fixture");
	assert_eq!(stale.phase, AgentSamplePhase::Obsolete);
	assert!(stale.answer.is_none());
}
