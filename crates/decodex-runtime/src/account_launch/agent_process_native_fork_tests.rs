//! Exercise the public branch commands against the installed native app-server.
use super::*;
use decodex_protocol::{PromptForkBoundary as Boundary, PromptForkPhase, PromptForkResult};

pub(super) async fn check(
	client: &AgentClient,
	native: &decodex_codex::app_server_client::AppServerClient,
	store: &SqliteStore,
	work: &EntityId,
	thread: &str,
	requests: &std::sync::atomic::AtomicUsize,
	home: &std::path::Path,
) {
	let source = native.thread_turns_since(thread, None).await.unwrap();
	let count = requests.load(Ordering::Acquire);
	let mut targets = Vec::new();
	for (index, boundary) in [Boundary::BeforeInput, Boundary::AfterTurn].into_iter().enumerate() {
		let selected = source[0]["id"].as_str().unwrap();
		let items = native.thread_read_turn_items(thread, selected).await.unwrap();
		let input =
			items.as_array().unwrap().iter().find(|item| item["type"] == "userMessage").unwrap();
		let target = EntityId::new(format!("fork-{index}")).unwrap();
		accepted(
			client,
			Action::PreparePromptEdit {
				work_id: work.clone(),
				thread_id: WireText::new(thread).unwrap(),
				turn_id: WireText::new(selected).unwrap(),
				item_id: WireText::new(input["id"].as_str().unwrap()).unwrap(),
			},
			&format!("fork-review-{index}"),
		)
		.await;
		let (review, content) =
			client.prompt_edit(work.clone(), WireText::new(thread).unwrap()).await.unwrap();
		let token = review.evidence.unwrap().review_token;
		let action = Action::ForkPromptEdit {
			work_id: work.clone(),
			thread_id: WireText::new(thread).unwrap(),
			review_token: token.clone(),
			target_work_id: target.clone(),
			boundary,
		};
		for retry in 0..2 {
			accepted(client, action.clone(), &format!("fork-confirm-{index}-{retry}")).await;
		}
		let PromptForkResult::Available(Some(receipt)) =
			client.prompt_fork(work.clone(), token.clone()).await.unwrap()
		else {
			panic!("durable branch receipt")
		};
		assert_eq!(receipt.phase, PromptForkPhase::Forked, "{receipt:?}");
		let fork = receipt.target_thread_id.unwrap();
		assert_ne!(fork.as_str(), thread);
		assert!(!targets.contains(&fork));
		targets.push(fork.clone());
		let turns = native.thread_turns_since(fork.as_str(), None).await.unwrap();
		assert_eq!(turns.len(), usize::from(boundary == Boundary::AfterTurn));
		assert_eq!(native.thread_turns_since(thread, None).await.unwrap(), source);
		let metadata = native.thread_read(json!({"threadId":fork.as_str()})).await.unwrap();
		assert_eq!(metadata["thread"]["forkedFromId"], thread);
		if boundary == Boundary::BeforeInput {
			let (status, restored) =
				client.prompt_edit(target.clone(), fork.clone()).await.unwrap();
			assert_eq!(restored, content);
			assert_eq!(status.phase, decodex_protocol::PromptEditPhase::Applied);
			qualify_prompt_acknowledgement(client, status, &content.unwrap(), home).await;
		}
		accepted(
			client,
			Action::RecoverPromptFork { work_id: work.clone(), review_token: token },
			&format!("fork-recover-{index}"),
		)
		.await;
		assert_eq!(
			requests.load(Ordering::Acquire),
			count,
			"branch/recovery/handback must not infer"
		);
	}
	assert_eq!(
		store.list_agent_work_items().await.unwrap().len(),
		3,
		"retries must not reserve extra work"
	);
}
