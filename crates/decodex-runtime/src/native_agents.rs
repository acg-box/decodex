//! Native inspection and preparation without promoting spawned threads into manager authority.
use std::time::Duration;

use serde_json::Value;
use tokio::time;

use crate::agent::native_subagents;
use decodex_codex::app_server_client::{AppServerClient, ClientError};
use decodex_database::SqliteStore;
use decodex_protocol::{NATIVE_PARENT_RECONNECT_MESSAGE, NativeAgentDto, NativeAgentsResult};

pub(crate) async fn read(
	store: &SqliteStore,
	client: &AppServerClient,
	work: &str,
	thread: Option<&str>,
	cursor: Option<&str>,
) -> NativeAgentsResult {
	let result = time::timeout(Duration::from_secs(10), async {
        let owner = store.get_agent_work_item(work.into()).await.ok()?;
        let root = owner.codex_thread_id?;

        if let Some(thread) = thread {
            let verified = native_subagents::request_owner(store,client,thread).await.ok()?;

            if verified.id != work { return None; }

            let value = client.thread_read(serde_json::json!({"threadId":thread,"includeTurns":true})).await.ok()?;

            let mut result = conversation(&value,thread)?;
            if let NativeAgentsResult::Conversation { permission_mode, service_tier, .. } = &mut result {
                *permission_mode = client.configured_task_permissions(thread)
                    .filter(|(_, guard)| guard.is_live()).and_then(|(facts, _)| facts.mode());
                *service_tier = client.configured_task_models(thread)
                    .filter(|(_, guard)| guard.is_live())
                    .map(|(facts, _)| facts.service_tier.unwrap_or_else(|| "default".into()));
            }
            return Some(result);
        }

        let value = client.request("thread/list",serde_json::json!({"ancestorThreadId":root,"sourceKinds":["subAgentThreadSpawn"],"archived":false,"modelProviders":[],"sortKey":"recency_at","sortDirection":"desc","limit":100,"cursor":cursor,"useStateDbOnly":true})).await.ok()?;
        let data = value["data"].as_array()?;

        if data.len()>100 {return None;}

        let agents = data.iter().map(observation).collect::<Option<Vec<_>>>()?;
        let next_cursor = match value.get("nextCursor") {
            Some(Value::Null) => None,
            Some(Value::String(cursor)) if !cursor.is_empty() && cursor.len() <= 4_096 => Some(cursor.clone()),
            _ => return None,
        };

        Some(NativeAgentsResult::Available { agents, next_cursor })
    }).await;

	result.ok().flatten().unwrap_or(NativeAgentsResult::Unavailable)
}

/// Explicitly reconnect a conversation without sending input or changing its configuration.
/// Native resume can continue its existing queue or Goal; ordinary inspection never resumes.
pub(crate) async fn prepare(
	store: &SqliteStore,
	client: &AppServerClient,
	work: &str,
	thread: &str,
	generation: &str,
) -> Result<(), &'static str> {
	time::timeout(Duration::from_secs(20), async {
		let owner = store
			.get_agent_work_item(work.into())
			.await
			.map_err(|_| "Could not verify this conversation.")?;
		let own_conversation = owner.codex_thread_id.as_deref() == Some(thread);
		let connected = match read(store, client, work, Some(thread), None).await {
			NativeAgentsResult::Conversation { can_input, .. } =>
				can_input.is_some()
					&& (!own_conversation
						|| client
							.configured_task_permissions(thread)
							.is_some_and(|(_, guard)| guard.is_live())),
			_ => return Err("Could not connect to this conversation."),
		};

		if !connected {
			// Use persisted native configuration; do not override parent-owned child settings.
			let value = client
				.thread_resume(serde_json::json!({"threadId": thread, "excludeTurns": true}))
				.await
				.map_err(|error| match error {
					ClientError::Remote(ref error) if error.message.contains("archived") =>
						"This conversation is archived. Restore it in Codex before continuing.",
					ClientError::Remote(ref error)
						if error.message.contains("resume the parent first") =>
						NATIVE_PARENT_RECONNECT_MESSAGE,
					ClientError::Remote(ref error)
						if error.message.contains("in use") || error.message.contains("locked") =>
						"This conversation is in use by another app.",
					_ => "Could not connect to this conversation.",
				})?;

			if value.pointer("/thread/id").and_then(Value::as_str) != Some(thread) {
				return Err("Could not verify this conversation.");
			}
		}

		if owner.parent_goal_id.is_none()
			&& own_conversation
			&& !store
				.attach_independent_agent_process(work.into(), thread.into(), generation.into())
				.await
				.map_err(|_| "Could not attach this conversation.")?
		{
			return Err("Could not attach this conversation.");
		}

		Ok(())
	})
	.await
	.unwrap_or(Err("Connecting took too long. Try again."))
}

fn observation(item: &Value) -> Option<NativeAgentDto> {
	let id = item["id"].as_str()?;
	let parent = item["parentThreadId"].as_str()?;
	let source = item.pointer("/source/subAgent/thread_spawn/parent_thread_id")?.as_str()?;

	if parent != source || id.len() > 512 || parent.len() > 512 {
		return None;
	}

	Some(NativeAgentDto {
		model: native_setting(item, "model"),
		reasoning_effort: native_setting(item, "reasoningEffort"),
		thread_id: id.into(),
		parent_thread_id: parent.into(),
		title: clean(
			item["name"]
				.as_str()
				.filter(|s| !s.is_empty())
				.or(item["agentNickname"].as_str())
				.unwrap_or("Agent"),
			120,
		),
		task: clean(item["preview"].as_str().unwrap_or(""), 320),
		status: observed_status(item),
	})
}

fn native_setting(value: &Value, key: &str) -> Option<String> {
	value[key]
		.as_str()
		.filter(|text| !text.is_empty() && !decodex_core::contains_credential_material(text))
		.map(|text| clean(text, 160))
}

// Preserve native waiting flags instead of presenting every active thread as executing.
fn observed_status(item: &Value) -> String {
	let state = item.pointer("/status/type").and_then(Value::as_str).unwrap_or("unknown");

	if state == "active" {
		if let Some(flags) = item.pointer("/status/activeFlags").and_then(Value::as_array) {
			for flag in ["waitingOnApproval", "waitingOnUserInput"] {
				if flags.iter().any(|v| v.as_str() == Some(flag)) {
					return flag.into();
				}
			}

			if !flags.is_empty() {
				return "unknown".into();
			}
		} else {
			return "unknown".into();
		}
	}

	clean(state, 32)
}

fn clean(text: &str, limit: usize) -> String {
	if decodex_core::contains_credential_material(text) {
		return "[Private content omitted]".into();
	}

	text.chars().take(limit).collect()
}

fn conversation(value: &Value, thread: &str) -> Option<NativeAgentsResult> {
	if value.pointer("/thread/id")?.as_str()? != thread {
		return None;
	}

	let turns = value.pointer("/thread/turns")?.as_array()?;

	Some(NativeAgentsResult::Conversation {
		model: native_setting(&value["thread"], "model"),
		reasoning_effort: native_setting(&value["thread"], "reasoningEffort"),
		thread_id: thread.into(),
		can_input: value.pointer("/thread/canAcceptDirectInput").and_then(Value::as_bool),
		active_turn: turns
			.iter()
			.rev()
			.find(|t| t["status"] == "inProgress")
			.and_then(|t| t["id"].as_str())
			.map(str::to_owned),
		permission_mode: None,
		service_tier: None,
	})
}
#[cfg(test)]
mod tests {
	use crate::native_agents::{self, NativeAgentsResult};

	#[tokio::test]
	async fn reconnect_hydrates_and_attaches_only_the_saved_conversation() {
		use crate::{agent_model_settings::tests::OwnedReviewer, agent_usage_estimate::Source};
		use decodex_codex::app_server_client::AppServerClient;
		use decodex_database::AgentDispatchState;
		use decodex_protocol::AgentPermissionState;
		use tokio::io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

		let home = tempfile::tempdir().unwrap();
		let (local, remote) = io::duplex(32_768);
		let (r, w) = io::split(local);
		let (client, _events) = AppServerClient::from_io(r, w);
		let owner = OwnedReviewer::new(home.path(), &client, "main-thread", "main-turn").await;
		let main = owner.store.get_agent_work_item("root".into()).await.unwrap();
		let mut chat = main.clone();
		chat.id = "saved".into();
		chat.codex_thread_id = None;
		chat.active_turn_id = None;
		chat.dispatch_state = AgentDispatchState::Idle;
		owner.store.create_agent_manager(chat).await.unwrap();
		owner.store.bind_agent_thread("saved".into(), "saved-thread".into()).await.unwrap();
		let mut key = owner.key.clone();
		key.work = "saved".into();
		key.thread = "saved-thread".into();
		let source = || async { Some(Source { key: key.clone(), client: client.clone() }) };
		assert_eq!(
			crate::agent_permissions::read(&owner.store, source).await,
			AgentPermissionState::NotConnected
		);

		let (release, released) = tokio::sync::oneshot::channel::<()>();
		let server = tokio::spawn(async move {
			let (r, mut w) = io::split(remote);
			let mut wire = BufReader::new(r).lines();
			for method in ["thread/read", "thread/resume", "permissionProfile/list", "thread/read"]
			{
				let request: serde_json::Value =
					serde_json::from_str(&wire.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], method);
				let result = match method {
					"thread/read" => {
						assert_eq!(request["params"]["threadId"], "saved-thread");
						serde_json::json!({"thread":{"id":"saved-thread","canAcceptDirectInput":true,"turns":[]}})
					},
					"thread/resume" => {
						assert_eq!(
							request["params"],
							serde_json::json!({"threadId":"saved-thread","excludeTurns":true})
						);
						serde_json::json!({"thread":{"id":"saved-thread"},"cwd":"/fixture","approvalPolicy":"on-request","approvalsReviewer":"user","sandbox":{"type":"readOnly"},"activePermissionProfile":{"id":":read-only"}})
					},
					_ =>
						serde_json::json!({"data":[{"id":":read-only","allowed":true}],"nextCursor":null}),
				};
				w.write_all(
					format!("{}\n", serde_json::json!({"id":request["id"],"result":result}))
						.as_bytes(),
				)
				.await
				.unwrap();
			}
			let _ = released.await;
			assert!(
				tokio::time::timeout(std::time::Duration::from_millis(20), wire.next_line())
					.await
					.is_err()
			);
		});
		native_agents::prepare(
			&owner.store,
			&client,
			"saved",
			"saved-thread",
			key.generation.as_str(),
		)
		.await
		.unwrap();
		assert!(matches!(
			crate::agent_permissions::read(&owner.store, source).await,
			AgentPermissionState::Available {
				mode: Some(decodex_core::PermissionMode::ReadOnly),
				can_update: true,
				..
			}
		));
		// Once connected, a second preparation reads without repeating native resume.
		native_agents::prepare(
			&owner.store,
			&client,
			"saved",
			"saved-thread",
			key.generation.as_str(),
		)
		.await
		.unwrap();
		assert_eq!(owner.store.get_agent_work_item("root".into()).await.unwrap(), main);
		assert!(owner.store.list_pending_agent_events(100).await.unwrap().is_empty());
		let _ = release.send(());
		server.await.unwrap();
	}

	#[test]
	fn native_input_metadata_keeps_turn_identity_and_does_not_guess_capability() {
		let mut v = serde_json::json!({"thread":{"id":"child","turns":[{"id":"t","status":"inProgress","items":[{"id":"u","type":"userMessage","content":[{"text":"Check"}]},{"id":"a","type":"agentMessage","text":"Result"}]}]}});
		let Some(NativeAgentsResult::Conversation { can_input, active_turn, .. }) =
			native_agents::conversation(&v, "child")
		else {
			panic!()
		};

		assert_eq!(can_input, None);
		assert_eq!(active_turn.as_deref(), Some("t"));
		assert!(matches!(
			native_agents::conversation(&v, "child"),
			Some(NativeAgentsResult::Conversation { model: None, reasoning_effort: None, .. })
		));
		v["thread"]["model"] = serde_json::json!("child-model");
		v["thread"]["reasoningEffort"] = serde_json::json!("medium");
		assert!(
			matches!(native_agents::conversation(&v, "child"),Some(NativeAgentsResult::Conversation { model:Some(model), reasoning_effort:Some(effort), .. }) if model == "child-model" && effort == "medium")
		);
		assert!(native_agents::conversation(&v, "other").is_none());
	}
}
#[cfg(test)]
mod observation_tests {
	use crate::native_agents::{self, NativeAgentDto, Value};
	#[test]
	fn active_waiting_flags_are_not_running() {
		for flag in ["waitingOnApproval", "waitingOnUserInput"] {
			assert_eq!(
				native_agents::observed_status(
					&serde_json::json!({"status":{"type":"active", "activeFlags":[flag]}})
				),
				flag
			);
		}

		assert_eq!(
			native_agents::observed_status(
				&serde_json::json!({"status":{"type":"active", "activeFlags":[]}})
			),
			"active"
		);
		assert_eq!(
			native_agents::observed_status(&serde_json::json!({"status":{"type":"active"}})),
			"unknown"
		);
	}

	#[test]
	fn native_inventory_retains_bounded_task_and_exact_parent() {
		let mut item = serde_json::json!({"id":"child","parentThreadId":"parent","source":{"subAgent":{"thread_spawn":{"parent_thread_id":"parent"}}},"agentNickname":"Mencius","preview":"Review navigation","status":{"type":"idle"},"model":"child-model","reasoningEffort":"high"});
		let agent = native_agents::observation(&item).unwrap();

		assert_eq!(agent.task, "Review navigation");
		assert_eq!(agent.status, "idle");
		assert_eq!(agent.model.as_deref(), Some("child-model"));
		assert_eq!(agent.reasoning_effort.as_deref(), Some("high"));

		item["preview"] = Value::String("x".repeat(400));

		assert_eq!(native_agents::observation(&item).unwrap().task.len(), 320);

		item["parentThreadId"] = Value::String("other".into());

		assert!(native_agents::observation(&item).is_none());
	}
	#[test]
	fn older_native_observations_without_task_remain_readable() {
		let agent: NativeAgentDto = serde_json::from_value(serde_json::json!({"thread_id":"child","parent_thread_id":"parent","title":"Agent","status":"idle"})).unwrap();

		assert!(agent.task.is_empty());
		assert!(agent.model.is_none() && agent.reasoning_effort.is_none());
	}
}
