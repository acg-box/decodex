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
