//! Real client, service owner, admitted bridge and native sampler under the shared socket fixture.
use std::{
	env,
	path::PathBuf,
	sync::atomic::{AtomicUsize, Ordering},
	time::Duration,
};

use tokio::time;

use crate::account_launch::agent_process::native_tests::cold_settings::recap_socket::{self};
use decodex_codex::app_server_client::AppServerClient;
use decodex_database::SqliteStore;
use decodex_protocol::{
	AgentActionDto, AgentClient, AgentCommandResponse, AgentSamplePhase, EntityId, HistoryText,
	IdempotencyKey, WireText,
};

pub(super) async fn parent(client: &AgentClient, store: &SqliteStore) -> String {
	if let Ok(thread) = time::timeout(Duration::from_secs(20), recap_socket::settled(client)).await
	{
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

	recap_socket::accepted(client, command.clone(), "explicit-native-sample").await;
	recap_socket::accepted(client, command, "explicit-native-sample").await;

	let answer = time::timeout(Duration::from_secs(40), async {
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

			time::sleep(Duration::from_millis(20)).await;
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

	recap_socket::accepted(
		client,
		AgentActionDto::CancelSample {
			work_id: work.clone(),
			request_id: WireText::new("explicit-native-sample").expect("sample request"),
		},
		"cancel-completed-sample",
	)
	.await;

	let preserved = client.sample(work.clone()).await.expect("completed sample");

	assert_eq!(preserved.phase, AgentSamplePhase::Ready);
	assert!(preserved.answer.is_some());

	check_foreign_cancel(client, work).await;

	assert_eq!(
		requests.load(Ordering::Acquire),
		calls + 1,
		"cancellation must not start inference"
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

	check_cancellation(client, native, work, thread, requests).await;

	recap_socket::accepted(
		client,
		AgentActionDto::Send {
			root_id: work.clone(),
			text: HistoryText::new("Advance the parent after its separate answer.")
				.expect("native sample service fixture"),
		},
		"sample-parent-next-input",
	)
	.await;

	assert_eq!(recap_socket::settled(client).await, thread);

	let stale = client.sample(work.clone()).await.expect("native sample service fixture");

	assert_eq!(stale.phase, AgentSamplePhase::Obsolete);
	assert!(stale.answer.is_none());
}

async fn check_cancellation(
	client: &AgentClient,
	native: &AppServerClient,
	work: &EntityId,
	thread: &str,
	requests: &AtomicUsize,
) {
	let home =
		PathBuf::from(env::var_os("DECODEX_TEST_ACCOUNT_HOME").expect("isolated fixture home"));
	let before = native.thread_markdown_transcript(thread).await.expect("parent history");
	let calls = requests.load(Ordering::Acquire);

	recap_socket::accepted(
		client,
		AgentActionDto::GenerateSample {
			work_id: work.clone(),
			thread_id: WireText::new(thread).expect("parent thread"),
			prompt: WireText::new("Hold this sample until cancellation.").expect("fixture prompt"),
		},
		"pending-native-sample",
	)
	.await;
	time::timeout(Duration::from_secs(15), async {
		while !home.join("sample-provider-started").exists() {
			time::sleep(Duration::from_millis(20)).await;
		}
	})
	.await
	.expect("sampling reached the controlled provider");

	assert_eq!(
		client.sample(work.clone()).await.expect("pending sample").phase,
		AgentSamplePhase::Pending
	);
	assert_eq!(native.thread_markdown_transcript(thread).await.expect("responsive parent"), before);

	let cancel = AgentActionDto::CancelSample {
		work_id: work.clone(),
		request_id: WireText::new("pending-native-sample").expect("original sample"),
	};

	recap_socket::accepted(client, cancel.clone(), "cancel-pending-native-sample").await;
	recap_socket::accepted(client, cancel, "cancel-pending-native-sample").await;

	time::timeout(Duration::from_secs(15), async {
		loop {
			let state = client.sample(work.clone()).await.expect("sample cleanup");

			assert!(state.is_valid());
			assert_eq!(
				state.request_id.as_ref().expect("original identity").as_str(),
				"pending-native-sample"
			);

			if state.phase == AgentSamplePhase::Cancelled {
				assert!(state.answer.is_none());

				break;
			}

			assert_eq!(state.phase, AgentSamplePhase::Cancelling);

			time::sleep(Duration::from_millis(20)).await;
		}
	})
	.await
	.expect("native cancellation completed");

	time::timeout(Duration::from_secs(5), async {
		while !home.join("sample-provider-closed").exists() {
			time::sleep(Duration::from_millis(20)).await;
		}
	})
	.await
	.expect("native closed the inference connection");

	assert_eq!(native.thread_markdown_transcript(thread).await.expect("preserved parent"), before);
	assert_eq!(requests.load(Ordering::Acquire), calls + 1, "cancel and retry cannot infer");
}

async fn check_foreign_cancel(client: &AgentClient, work: &EntityId) {
	let foreign = client
		.execute(
			AgentActionDto::CancelSample {
				work_id: work.clone(),
				request_id: WireText::new("foreign-sample").expect("sample request"),
			},
			IdempotencyKey::new("foreign-sample-cancel").expect("sample command"),
		)
		.await
		.expect("sample command");

	assert!(matches!(foreign, AgentCommandResponse::Rejected { .. }));
}
