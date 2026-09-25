use super::*;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn installed_state_preserves_disabled_and_non_callable_apps() {
	let rows = json!([
		{"id":"disabled","runtimeName":null,"enabled":false,"callable":false},
		{"id":"no-tools","runtimeName":"Empty","enabled":true,"callable":false},
		{"id":"ready","enabled":true,"callable":true}
	]);
	let expected = rows.clone();
	let (local, remote) = tokio::io::duplex(65536);
	let (reader, writer) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(reader, writer);
	let server = tokio::spawn(async move {
		let (reader, mut writer) = tokio::io::split(remote);
		let mut lines = BufReader::new(reader).lines();
		for refresh in [false, true] {
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], "app/installed");
			assert_eq!(
				request["params"],
				json!({"threadId":"exact-thread","forceRefresh":refresh})
			);
			writer
				.write_all(
					format!("{}\n", json!({"id":request["id"],"result":{"apps":rows}})).as_bytes(),
				)
				.await
				.unwrap();
		}
	});
	for refresh in [false, true] {
		assert_eq!(
			json!(client.installed_apps_for_thread("exact-thread", refresh).await.unwrap()),
			expected
		);
	}
	server.await.unwrap();
}

#[tokio::test]
async fn failed_or_malformed_live_refresh_never_acknowledges_directory_refresh() {
	for reply in [
		json!({"error":{"code":-32603,"message":"Live runtime refresh failed"}}),
		json!({"error":{"code":-32601,"message":"Method not supported"}}),
		json!({"result":{}}),
		json!({"result":{"apps":[{"id":"missing-state"}]}}),
		json!({"result":{"apps":[{"id":"inconsistent","enabled":false,"callable":true}]}}),
		json!({"result":{"apps":[{"id":"dup","enabled":true,"callable":true},{"id":"dup","enabled":true,"callable":true}]}}),
	] {
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			for (method, response) in [
				(
					"plugin/reconcile",
					json!({"result":{"changedPlugins":[],"failedRemotePluginIds":[],"failedMaterializationRemotePluginIds":[]}}),
				),
				("config/mcpServer/reload", json!({"result":{}})),
				("app/list", json!({"result":{"data":[],"nextCursor":null}})),
				("app/installed", reply),
			] {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], method);
				if method == "app/installed" {
					assert_eq!(
						request["params"],
						json!({"threadId":"exact-thread","forceRefresh":true})
					);
				}
				let mut response = response;
				response["id"] = request["id"].clone();
				writer.write_all(format!("{response}\n").as_bytes()).await.unwrap();
			}
			assert!(
				tokio::time::timeout(std::time::Duration::from_millis(50), lines.next_line())
					.await
					.is_err()
			);
		});
		assert!(client.refresh_integrations("exact-thread").await.is_err());
		server.await.unwrap();
	}
}
