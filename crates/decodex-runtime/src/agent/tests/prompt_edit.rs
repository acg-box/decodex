use super::*;
use std::sync::{Arc, Mutex};

type NativeHistory = Arc<Mutex<Vec<Value>>>;
fn history() -> NativeHistory {
	Arc::new(Mutex::new(["prefix","selected","suffix"].iter().map(|id| json!({"id":id,"status":"completed","items":[{"type":"userMessage","id":format!("{id}-input"),"content":[{"type":"text","text":format!("Input {id}"),"text_elements":[]},{"type":"mention","name":"Sample","path":"plugin://sample@test"}]}]})).collect()))
}
fn transport(
	mode: &'static str,
	history: NativeHistory,
) -> (AppServerClient, tokio::sync::mpsc::UnboundedReceiver<Value>, tokio::task::JoinHandle<()>) {
	let (incoming, frames) = tokio::sync::mpsc::channel(16);
	let (outgoing, mut writes) = tokio::sync::mpsc::channel::<Value>(16);
	let (client, _events) = AppServerClient::from_framed(1, frames, outgoing).unwrap();
	let (log, reads) = tokio::sync::mpsc::unbounded_channel();
	let server = tokio::spawn(async move {
		let mut selected_reads = 0;
		let mut fork: Option<Vec<Value>> = None;

		while let Some(request) = writes.recv().await {
			log.send(request.clone()).unwrap();

			let is_fork = request["params"]["threadId"] == "branch-thread";

			assert!(is_fork || request["params"]["threadId"] == "opaque thread/1");

			if is_fork && mode == "fork-read-failure" {
				return;
			}

			let visible = if is_fork {
				fork.clone().unwrap_or_else(|| history.lock().unwrap()[..1].to_vec())
			} else {
				history.lock().unwrap().clone()
			};
			let method = request["method"].as_str().unwrap();

			if method == "thread/revert" {
				assert_eq!(
					request["params"],
					json!({"threadId":"opaque thread/1","beforeTurnId":"selected"})
				);

				if mode != "validation" {
					history.lock().unwrap().truncate(1);
				}
				if mode == "lost-reply" {
					return;
				}
				if mode == "validation" || mode == "internal-after-commit" {
					incoming.send(Ok(json!({"id":request["id"],"error":{"code":if mode=="validation" {-32_602} else {-32_603},"message":"fixture"}}))).await.unwrap();

					continue;
				}
			}

			let result = match method {
				"thread/fork" => {
					assert!(mode.starts_with("fork"));
					assert_eq!(request["params"]["deferGoalContinuation"], true);
					assert_eq!(request["params"]["excludeTurns"], true);

					let after = request["params"].get("lastTurnId").is_some();
					let selected = request["params"]
						[if after { "lastTurnId" } else { "beforeTurnId" }]
					.as_str()
					.unwrap();
					let end = visible.iter().position(|t| t["id"] == selected).unwrap()
						+ usize::from(after);

					fork = Some(visible[..end].to_vec());

					if mode == "fork-lost-reply" {
						return;
					}

					json!({"thread":{"id":"branch-thread","forkedFromId":"opaque thread/1","historyMode":"paginated","turns":[]}})
				},
				"thread/read" if is_fork =>
					json!({"thread":{"id":"branch-thread","forkedFromId":"opaque thread/1","historyMode":"paginated","turns":[]}}),
				"thread/read" | "thread/revert" =>
					json!({"thread":{"id":"opaque thread/1","historyMode":"paginated","turns":[]}}),
				"thread/turns/list" => {
					let mut turns = visible.clone();

					turns.reverse();
					turns.truncate(request["params"]["limit"].as_u64().unwrap() as usize);

					if request["params"]["itemsView"] != "full" {
						for t in &mut turns {
							t["items"] = json!([]);
						}
					}

					json!({"data":turns,"nextCursor":null})
				},
				"thread/items/list" => {
					let turn = request["params"]["turnId"].as_str().unwrap();
					let mut items = visible.iter().find(|t| t["id"] == turn).unwrap()["items"]
						.as_array()
						.unwrap()
						.clone();

					if turn == "selected" {
						selected_reads += 1;

						if mode == "changed-content" && selected_reads > 1 {
							items[0]["content"][0]["text"] = json!("Different persisted input");
						}
					}

					json!({"data":items.into_iter().map(|item|json!({"turnId":turn,"item":item})).collect::<Vec<_>>(),"nextCursor":null})
				},
				other => panic!("unexpected native effect: {other}"),
			};

			if incoming.send(Ok(json!({"id":request["id"],"result":result}))).await.is_err() {
				break;
			}
		}
	});

	(client, reads, server)
}

#[tokio::test]
async fn prompt_edit_submits_once_and_recovers_lost_or_postcommit_replies_without_replay() {
	for mode in ["success", "lost-reply", "internal-after-commit", "validation"] {
		let (mut agent, _old_reads, _directory) = fixture().await;

		agent.start_agent("agent", "Start").await.unwrap();

		complete(&mut agent, "agent").await;

		let native = history();
		let (client, mut reads, server) = transport(mode, native.clone());

		agent.client = client;

		let review = agent
			.prepare_prompt_edit("agent", "opaque thread/1", "selected", "selected-input", "review")
			.await
			.unwrap()
			.unwrap();
		let content = review.evidence().content.clone();

		assert_eq!(review.evidence().turn_ids, vec!["prefix", "selected", "suffix"]);

		let receipt = agent.confirm_prompt_edit(review).await.unwrap();
		let requests = std::iter::from_fn(|| reads.try_recv().ok()).collect::<Vec<_>>();

		assert_eq!(requests.iter().filter(|r| r["method"] == "thread/revert").count(), 1);
		assert_eq!(receipt.attempt.content, content);

		if mode == "validation" {
			assert_eq!(receipt.state, "not_submitted");
			assert_eq!(native.lock().unwrap().len(), 3);
		} else {
			assert!(agent.store.begin_agent_dispatch("agent".into()).await.is_err());
			assert_eq!(receipt.state, if mode == "lost-reply" { "reserved" } else { "applied" });

			let store = agent.store.clone();
			let config = agent.config.clone();

			drop(agent);

			let (fresh, mut recovery_reads, recovery_server) = transport("success", native.clone());
			let mut recovered = AgentCoordinator::new(store, fresh, config).unwrap();
			let receipt =
				recovered.recover_prompt_edit("agent", "opaque thread/1").await.unwrap().unwrap();

			assert_eq!(receipt.state, "applied");
			assert_eq!(receipt.attempt.content, content);
			assert!(
				recovered.store.begin_agent_dispatch("agent".into()).await.is_err(),
				"draft acknowledgement remains required"
			);
			assert!(
				std::iter::from_fn(|| recovery_reads.try_recv().ok())
					.all(|r| r["method"] != "thread/revert")
			);

			recovery_server.abort();

			let _ = recovery_server.await;
		}

		server.abort();

		let _ = server.await;
	}
}

#[tokio::test]
async fn prompt_edit_revalidates_content_before_reservation_or_native_mutation() {
	let (mut agent, _old_reads, _directory) = fixture().await;

	agent.start_agent("agent", "Start").await.unwrap();

	complete(&mut agent, "agent").await;

	let native = history();
	let (client, mut reads, server) = transport("changed-content", native);

	agent.client = client;

	let review = agent
		.prepare_prompt_edit("agent", "opaque thread/1", "selected", "selected-input", "review")
		.await
		.unwrap()
		.unwrap();

	assert!(agent.confirm_prompt_edit(review).await.is_err());
	assert!(
		agent
			.store
			.agent_prompt_edit_receipt("agent".into(), "opaque thread/1".into())
			.await
			.unwrap()
			.is_none()
	);
	assert!(std::iter::from_fn(|| reads.try_recv().ok()).all(|r| r["method"] != "thread/revert"));

	server.abort();

	let _ = server.await;
}

#[tokio::test]
async fn canonical_input_queue_preserves_parts_and_settings_without_sending_the_preview() {
	let (mut agent, _old_reads, _directory) = fixture().await;

	agent.start_agent("agent", "Start").await.unwrap();

	complete(&mut agent, "agent").await;

	let (client, _reads, server) = transport("success", history());

	agent.client = client;

	let review = agent
		.prepare_prompt_edit(
			"agent",
			"opaque thread/1",
			"selected",
			"selected-input",
			"canonical-review",
		)
		.await
		.unwrap()
		.unwrap();
	let receipt = agent.confirm_prompt_edit(review).await.unwrap();
	let content = vec![
		json!({"type":"text","text":"Use $skill","text_elements":[{"byteRange":{"start":4,"end":10},"placeholder":"skill"}]}),
		json!({"type":"image","url":format!("data:image/png;base64,{}", "A".repeat(100_000)),"detail":"original"}),
		json!({"type":"skill","name":"skill","path":"/fixture/SKILL.md"}),
	];
	let saved = agent
		.store
		.retain_agent_prompt_input(
			"agent".into(),
			"opaque thread/1".into(),
			receipt.id,
			content.clone(),
		)
		.await
		.unwrap();
	let event = decodex_database::EnqueueAgentEvent {
		source_event_id: json!(["user_message", "agent", "canonical-send"]).to_string(), work_item_id: "agent".into(), event_kind: "user_message".into(),
		payload: json!({"text":"BOUNDED PREVIEW ONLY","source":"user","options":{
			"canonicalInput":{"id":saved.id,"threadId":saved.thread,"editReceiptId":receipt.id,"sha256":saved.sha256},
			"execution":{"reasoning_effort":"high"},"attachments":[],"taskReferences":[]
		}}).to_string(),
	};

	assert!(
		agent.store.enqueue_agent_event(event.clone()).await.is_err(),
		"handback must be acknowledged"
	);
	assert!(agent.store.release_agent_prompt_edit_draft(receipt.id, None).await.unwrap());

	let queued = agent.store.enqueue_agent_event(event.clone()).await.unwrap();

	assert!(queued.payload.len() < 2_048);

	let payload: Value = serde_json::from_str(&queued.payload).unwrap();
	let reference = payload["options"]["canonicalInput"].clone();
	let execution = payload["options"]["execution"].clone();

	assert_eq!(
		agent
			.store
			.agent_prompt_send_event(
				"agent".into(),
				"canonical-send".into(),
				reference.clone(),
				execution.clone()
			)
			.await
			.unwrap(),
		Some(queued.id)
	);
	assert_eq!(
		agent
			.store
			.agent_prompt_send_event(
				"agent".into(),
				"unknown-send".into(),
				reference.clone(),
				execution.clone()
			)
			.await
			.unwrap(),
		None
	);
	assert_eq!(
		agent
			.store
			.agent_prompt_send_event(
				"agent".into(),
				"canonical-send".into(),
				reference.clone(),
				json!({"reasoning_effort":"low"})
			)
			.await
			.unwrap(),
		None
	);

	let mut crossed = reference;

	crossed["threadId"] = json!("different-thread");

	assert_eq!(
		agent
			.store
			.agent_prompt_send_event("agent".into(), "canonical-send".into(), crossed, execution)
			.await
			.unwrap(),
		None
	);
	assert_eq!(agent.store.enqueue_agent_event(event.clone()).await.unwrap().id, queued.id);

	let item = agent.store.get_agent_work_item("agent".into()).await.unwrap();
	let (params, _) =
		agent.dispatch_input(&item, "BOUNDED PREVIEW ONLY", &[queued.id], false).await.unwrap();

	assert_eq!(params["input"], json!(content));
	assert_eq!(params["effort"], "high");
	assert_eq!(params["turnTrigger"], "user");

	let mut crossed = item.clone();

	crossed.codex_thread_id = Some("replacement-thread".into());

	assert!(agent.dispatch_input(&crossed, "preview", &[queued.id], false).await.is_err());

	let mut foreign = event;

	foreign.source_event_id = "foreign".into();

	let mut payload: Value = serde_json::from_str(&foreign.payload).unwrap();

	payload["options"]["canonicalInput"]["sha256"] = json!("0".repeat(64));
	foreign.payload = payload.to_string();

	assert!(agent.store.enqueue_agent_event(foreign).await.is_err());

	server.abort();

	let _ = server.await;
}

#[tokio::test]
async fn prompt_fork_preserves_source_and_recovers_acknowledged_identity_without_replay() {
	use decodex_database::AgentForkBoundary;

	for (mode, boundary) in [
		("fork-success", AgentForkBoundary::BeforeInput),
		("fork-success", AgentForkBoundary::AfterTurn),
		("fork-read-failure", AgentForkBoundary::BeforeInput),
		("fork-lost-reply", AgentForkBoundary::BeforeInput),
	] {
		let (mut agent, _old_reads, _directory) = fixture().await;

		agent.start_agent("agent", "Start").await.unwrap();

		complete(&mut agent, "agent").await;

		let native = history();
		let original = native.lock().unwrap().clone();
		let (client, mut reads, server) = transport(mode, native.clone());

		agent.client = client;

		let review = agent
			.prepare_prompt_edit(
				"agent",
				"opaque thread/1",
				"selected",
				"selected-input",
				"fork-review",
			)
			.await
			.unwrap()
			.unwrap();
		let token = review.evidence().review_token.clone();
		let content = review.evidence().content.clone();
		let receipt = agent.fork_prompt_edit(review, "branch-work".into(), boundary).await.unwrap();

		assert_eq!(*native.lock().unwrap(), original);
		assert!(
			agent
				.store
				.agent_prompt_edit_receipt("agent".into(), "opaque thread/1".into())
				.await
				.unwrap()
				.is_none()
		);

		let requests = std::iter::from_fn(|| reads.try_recv().ok()).collect::<Vec<_>>();

		assert_eq!(requests.iter().filter(|r| r["method"] == "thread/fork").count(), 1);
		assert!(requests.iter().all(|r| !matches!(
			r["method"].as_str(),
			Some("thread/revert" | "turn/start" | "thread/resume")
		)));
		assert_eq!(
			receipt.state,
			match mode {
				"fork-lost-reply" => "reserved",
				"fork-read-failure" => "acknowledged",
				_ => "forked",
			}
		);

		if mode == "fork-success" {
			let draft = agent
				.store
				.agent_prompt_edit_receipt("branch-work".into(), "branch-thread".into())
				.await
				.unwrap();

			assert_eq!(draft.is_some(), boundary == AgentForkBoundary::BeforeInput);

			if let Some(draft) = draft {
				assert_eq!(draft.attempt.content, content);
			}
		} else {
			let store = agent.store.clone();
			let config = agent.config.clone();

			drop(agent);

			let (fresh, mut recovery_reads, recovery_server) =
				transport("fork-recovery", native.clone());
			let mut recovered = AgentCoordinator::new(store, fresh, config).unwrap();
			let recovered = recovered.recover_prompt_fork("agent", &token).await.unwrap().unwrap();

			assert_eq!(
				recovered.state,
				if mode == "fork-lost-reply" { "reserved" } else { "forked" }
			);
			assert!(std::iter::from_fn(|| recovery_reads.try_recv().ok()).all(|r| matches!(
				r["method"].as_str(),
				Some("thread/read" | "thread/turns/list" | "thread/items/list")
			)));

			recovery_server.abort();

			let _ = recovery_server.await;
		}

		server.abort();

		let _ = server.await;
	}
}
