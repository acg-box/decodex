use super::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

async fn replies(responses: Vec<Value>) -> (AppServerClient, tokio::task::JoinHandle<Vec<Value>>) {
	let (local, remote) = tokio::io::duplex(256 * 1024);
	let (reader, writer) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(reader, writer);
	let server = tokio::spawn(async move {
		let (reader, mut writer) = tokio::io::split(remote);
		let mut lines = BufReader::new(reader).lines();
		let mut requests = Vec::new();
		for mut response in responses {
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			response["id"] = request["id"].clone();
			requests.push(request);
			writer.write_all(format!("{response}\n").as_bytes()).await.unwrap();
		}
		requests
	});
	(client, server)
}

#[tokio::test]
async fn paged_catalog_preserves_disabled_profiles_and_native_cwd() {
	let (client, server) = replies(vec![
		json!({"result":{"data":[{"id":"scoped","allowed":true,"description":"Scope"}],"nextCursor":"page-2"}}),
		json!({"result":{"data":[{"id":":full-access","allowed":false}]}}),
	]).await;
	let rows = client.permission_profiles("/native/project").await.unwrap();
	assert_eq!(rows.len(), 2);
	assert!(!rows[1].allowed);
	assert_eq!(rows[1].description, None);
	let requests = server.await.unwrap();
	assert_eq!(requests.len(), 2);
	for request in &requests {
		assert_eq!(request["method"], "permissionProfile/list");
		assert_eq!(request["params"]["cwd"], "/native/project");
	}
	assert_eq!(requests[1]["params"]["cursor"], "page-2");
}

#[tokio::test]
async fn invalid_or_incomplete_catalog_never_returns_partial_success() {
	for second in [
		json!({"result":{"data":[],"nextCursor":"next"}}),
		json!({"result":{"data":[{"id":"scoped","allowed":true}],"nextCursor":null}}),
		json!({"result":{"data":[{"id":"other"}],"nextCursor":null}}),
		json!({"result":{"data":[],"nextCursor":false}}),
		json!({"error":{"code":-32601,"message":"unsupported"}}),
	] {
		let (client, server) = replies(vec![
			json!({"result":{"data":[{"id":"scoped","allowed":true}],"nextCursor":"next"}}),
			second,
		])
		.await;
		assert!(client.permission_profiles("/native/project").await.is_err());
		assert_eq!(server.await.unwrap().len(), 2);
	}
}

#[test]
fn selection_cannot_carry_unrelated_policy_or_configuration() {
	let wire =
		serde_json::to_value(ThreadPermissionSelection::new("opaque thread", "scoped").unwrap())
			.unwrap();
	assert!(is_thread_permission_selection(&wire));
	for key in ["model", "config", "sandbox", "cwd", "approvalPolicy", "approvalsReviewer"] {
		let mut bad = wire.clone();
		bad[key] = json!("unexpected");
		assert!(!is_thread_permission_selection(&bad));
	}
	assert!(ThreadPermissionSelection::new("thread", " ").is_err());
	assert!(ThreadPermissionSelection::new("thread\n", "scoped").is_err());
}

#[tokio::test]
async fn selection_ack_is_only_queued_and_errors_are_not_retried() {
	for response in [
		json!({"result":{}}),
		json!({"result":{"applied":true}}),
		json!({"error":{"code":-32602,"message":"profile unavailable"}}),
	] {
		let expected = response == json!({"result":{}});
		let (client, server) = replies(vec![response]).await;
		let guard = client.history_guard(0).unwrap();
		let result = client
			.queue_thread_permission_selection(
				&ThreadPermissionSelection::new("thread", "scoped").unwrap(),
				guard,
			)
			.await;
		assert_eq!(result.is_ok(), expected);
		let requests = server.await.unwrap();
		assert_eq!(requests.len(), 1);
		assert_eq!(requests[0]["params"], json!({"threadId":"thread","permissions":"scoped"}));
	}
}

#[test]
fn permission_projection_keeps_native_facts_without_copying_unrelated_instructions() {
	let value = json!({"cwd":"/native","activePermissionProfile":{"id":"scoped"},
		"approvalPolicy":{"reject":{"sandbox_approval":true}},"approvalsReviewer":"auto_review",
		"sandboxPolicy":{"type":"readOnly"},"developerInstructions":"private unrelated instructions"});
	let observed = NativeTaskPermissions::from_notification(&value).unwrap();
	assert_eq!(observed.profile_id.as_deref(), Some("scoped"));
	assert!(!serde_json::to_string(&observed).unwrap().contains("private unrelated"));
	let mut resumed = value.clone();
	resumed["sandbox"] = resumed["sandboxPolicy"].take();
	assert_eq!(NativeTaskPermissions::from_thread_response(&resumed), Some(observed));
	for field in ["cwd", "approvalPolicy", "approvalsReviewer", "sandboxPolicy"] {
		let mut missing = value.clone();
		missing.as_object_mut().unwrap().remove(field);
		assert!(NativeTaskPermissions::from_notification(&missing).is_none());
	}
	let mut malformed = value.clone();
	malformed["activePermissionProfile"] = json!({});
	assert!(NativeTaskPermissions::from_notification(&malformed).is_none());
	let mut no_profile = value;
	no_profile["activePermissionProfile"] = Value::Null;
	assert_eq!(NativeTaskPermissions::from_notification(&no_profile).unwrap().profile_id, None);
}

use crate::app_server_client::{
	PendingReply, PermissionHydration, RequestId, ServerRequests, dispatch,
};
use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot};

fn permission_facts(profile: &str) -> Value {
	json!({"cwd":"/native", "activePermissionProfile":{"id":profile},
		"approvalPolicy":"on-request", "approvalsReviewer":"user",
		"sandboxPolicy":{"type":"readOnly"}})
}

#[test]
fn wire_publication_invalidates_old_authority_before_owner_consumption() {
	let requests = ServerRequests::default();
	let (events, mut receiver) = mpsc::channel(8);
	let mut pending = HashMap::new();
	let publish = |facts: Value| {
		json!({"method":"thread/settings/updated",
		"params":{"threadId":"task", "threadSettings":facts}})
	};
	dispatch(publish(permission_facts("old")), &mut pending, &events, &requests).unwrap();
	let (_, old_guard) = requests.permission_observation("task").unwrap();
	dispatch(publish(permission_facts("new")), &mut pending, &events, &requests).unwrap();
	assert!(!old_guard.is_live());
	let (current, current_guard) = requests.permission_observation("task").unwrap();
	assert_eq!(current.profile_id.as_deref(), Some("new"));
	assert!(current_guard.is_live());
	// Both publications are still queued for the coordinator.
	assert_eq!(receiver.len(), 2);
	dispatch(publish(json!({"cwd":"/native"})), &mut pending, &events, &requests).unwrap();
	assert!(!current_guard.is_live());
	assert!(requests.permission_observation("task").is_none());
	assert!(receiver.try_recv().is_ok());
}

#[test]
fn late_hydration_cannot_restore_invalidated_or_foreign_permission_facts() {
	for start in [false, true] {
		for malformed in [false, true] {
			let requests = ServerRequests::default();
			let (events, _receiver) = mpsc::channel(8);
			let (reply, _result) = oneshot::channel();
			let hydration = if start {
				PermissionHydration::Start { revision: requests.permission_revision() }
			} else {
				PermissionHydration::Resume {
					thread: "task".into(),
					guard: requests.thread_settings_guard("task").unwrap(),
				}
			};
			let mut pending = HashMap::from([(
				RequestId::Number(1),
				PendingReply { reply, permissions: Some(hydration) },
			)]);
			let facts = if malformed { json!({}) } else { permission_facts("new") };
			dispatch(json!({"method":"thread/settings/updated", "params":{"threadId":"task", "threadSettings":facts}}), &mut pending, &events, &requests).unwrap();
			let mut old = permission_facts("old");
			old["sandbox"] = old["sandboxPolicy"].take();
			old["thread"] = json!({"id":"task"});
			dispatch(json!({"id":1,"result":old}), &mut pending, &events, &requests).unwrap();
			let observed = requests.permission_observation("task");
			if malformed {
				assert!(observed.is_none());
			} else {
				assert_eq!(observed.unwrap().0.profile_id.as_deref(), Some("new"));
			}
		}
	}
	let requests = ServerRequests::default();
	let (events, _receiver) = mpsc::channel(8);
	let (reply, _result) = oneshot::channel();
	let hydration = PermissionHydration::Resume {
		thread: "task".into(),
		guard: requests.thread_settings_guard("task").unwrap(),
	};
	let mut pending = HashMap::from([(
		RequestId::Number(1),
		PendingReply { reply, permissions: Some(hydration) },
	)]);
	let mut response = permission_facts("foreign");
	response["sandbox"] = response["sandboxPolicy"].take();
	response["thread"] = json!({"id":"other"});
	dispatch(json!({"id":1,"result":response}), &mut pending, &events, &requests).unwrap();
	assert!(requests.permission_observation("task").is_none());
	assert!(requests.permission_observation("other").is_none());
}

#[test]
fn permission_cache_is_bounded_and_connection_close_revokes_authority() {
	let requests = ServerRequests::default();
	let mut response = permission_facts("scoped");
	response["sandbox"] = response["sandboxPolicy"].take();
	for index in 0..257 {
		requests.observe_permission_hydration(&format!("task-{index}"), &response);
	}
	assert!(requests.permission_observation("task-255").is_some());
	assert!(requests.permission_observation("task-256").is_none());
	let (_, old_guard) = requests.permission_observation("task-0").unwrap();
	response["activePermissionProfile"] = json!({"id":"replacement"});
	requests.observe_permission_hydration("task-0", &response);
	assert!(!old_guard.is_live());
	let (_, guard) = requests.permission_observation("task-0").unwrap();
	requests.clear();
	assert!(!guard.is_live());
	assert!(requests.permission_observation("task-0").is_none());
}

#[tokio::test]
async fn newer_wire_settings_reject_permission_write_before_owner_drain() {
	let (local, remote) = tokio::io::duplex(32768);
	let (reader, writer) = tokio::io::split(local);
	let (client, events) = AppServerClient::from_io(reader, writer);
	let (reader, mut writer) = tokio::io::split(remote);
	let mut lines = BufReader::new(reader).lines();
	let owned_client = client.clone();
	let hydrate = tokio::spawn(async move {
		owned_client.request("thread/resume", json!({"threadId":"task"})).await
	});
	let request: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
	let mut facts = permission_facts("old");
	facts["sandbox"] = facts["sandboxPolicy"].take();
	facts["thread"] = json!({"id":"task"});
	writer
		.write_all(format!("{}\n", json!({"id":request["id"],"result":facts})).as_bytes())
		.await
		.unwrap();
	hydrate.await.unwrap().unwrap();
	let (_, old_guard) = client.observed_task_permissions("task").unwrap();
	let barrier_client = client.clone();
	let barrier =
		tokio::spawn(async move { barrier_client.request("test/barrier", json!({})).await });
	let request: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
	writer.write_all(format!("{}\n{}\n", json!({"method":"thread/settings/updated","params":{"threadId":"task","threadSettings":permission_facts("new")}}), json!({"id":request["id"],"result":{}})).as_bytes()).await.unwrap();
	barrier.await.unwrap().unwrap();
	assert_eq!(events.len(), 1);
	assert_eq!(
		client.observed_task_permissions("task").unwrap().0.profile_id.as_deref(),
		Some("new")
	);
	let selection = ThreadPermissionSelection::new("task", "scoped").unwrap();
	assert!(matches!(
		client.queue_thread_permission_selection(&selection, old_guard).await,
		Err(ClientError::StaleHistory)
	));
	assert!(
		tokio::time::timeout(std::time::Duration::from_millis(20), lines.next_line())
			.await
			.is_err()
	);
}

#[test]
fn idle_permission_facts_return_only_after_exact_turn_completion_without_reviving_old_guards() {
	for update in ["unchanged", "changed", "malformed"] {
		let requests = ServerRequests::default();
		let (events, _receiver) = mpsc::channel(8);
		let mut pending = HashMap::new();
		let publish = |facts: Value| json!({"method":"thread/settings/updated","params":{"threadId":"task","threadSettings":facts}});
		dispatch(publish(permission_facts("old")), &mut pending, &events, &requests).unwrap();
		let (_, old_guard) = requests.permission_observation("task").unwrap();
		dispatch(
			json!({"method":"turn/started","params":{"threadId":"task","turn":{"id":"active"}}}),
			&mut pending,
			&events,
			&requests,
		)
		.unwrap();
		assert!(!old_guard.is_live());
		assert!(requests.permission_observation("task").is_none());
		let (configured, selection_guard) =
			requests.configured_permissions("task").expect("running configured facts");
		assert_eq!(configured.profile_id.as_deref(), Some("old"));
		assert!(selection_guard.is_live());
		match update {
			"changed" =>
				dispatch(publish(permission_facts("new")), &mut pending, &events, &requests)
					.unwrap(),
			"malformed" => dispatch(publish(json!({})), &mut pending, &events, &requests).unwrap(),
			_ => {},
		}
		if update == "malformed" {
			assert!(requests.configured_permissions("task").is_none());
		} else {
			assert_eq!(
				requests
					.configured_permissions("task")
					.expect("configured facts")
					.0
					.profile_id
					.as_deref(),
				Some(if update == "changed" { "new" } else { "old" })
			);
		}
		assert_eq!(selection_guard.is_live(), update == "unchanged");

		assert!(requests.permission_observation("task").is_none());
		dispatch(
			json!({"method":"turn/completed","params":{"threadId":"task","turn":{"id":"older"}}}),
			&mut pending,
			&events,
			&requests,
		)
		.unwrap();
		assert!(requests.permission_observation("task").is_none());
		dispatch(
			json!({"method":"turn/completed","params":{"threadId":"task","turn":{"id":"active"}}}),
			&mut pending,
			&events,
			&requests,
		)
		.unwrap();
		assert!(!old_guard.is_live());
		if update == "malformed" {
			assert!(requests.permission_observation("task").is_none());
		} else {
			let (facts, guard) = requests.permission_observation("task").unwrap();
			assert!(guard.is_live());
			assert_eq!(
				facts.profile_id.as_deref(),
				Some(if update == "changed" { "new" } else { "old" })
			);
		}
	}
}

#[test]
fn permission_lifecycle_revokes_facts_and_pending_resume_hydration() {
	for method in ["thread/closed", "thread/archived", "thread/deleted", "thread/reverted"] {
		let requests = ServerRequests::default();
		let mut facts = permission_facts("scoped");
		facts["sandbox"] = facts["sandboxPolicy"].take();
		facts["thread"] = json!({"id":"task"});
		requests.observe_permission_hydration("task", &facts);
		let (_, guard) = requests.permission_observation("task").unwrap();
		let hydration = PermissionHydration::Resume { thread: "task".into(), guard: guard.clone() };
		requests
			.observe(&crate::app_server_client::ServerEvent::Notification {
				method: method.into(),
				params: json!({"threadId":"task"}),
			})
			.unwrap();
		assert!(!guard.is_live(), "{method}");
		hydration.observe(&facts, &requests);
		assert!(requests.permission_observation("task").is_none(), "{method}");
		assert!(requests.configured_permissions("task").is_none(), "{method}");
	}
}
