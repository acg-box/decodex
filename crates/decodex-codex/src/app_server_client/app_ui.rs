//! Native resource transport for source-bound MCP App UI consumers.
use super::{AppServerClient, ClientError, HistoryGuard};
use serde_json::{Value, json};

/// Native tool evidence and resources retained for a single widget view.
#[derive(Clone)]
pub struct NativeAppUi {
	/// Exact original tool item, including input, result and presentation metadata.
	pub item: Value,
	/// Native resource contents and sandbox metadata.
	pub resources: Vec<Value>,
	/// Guard invalidated by native history, settings or connection changes.
	pub guard: HistoryGuard,
}

impl AppServerClient {
	/// Resolve a widget from exact native history, never from a UI-provided resource URI.
	/// Account and process ownership must also be revalidated by the service caller.
	pub async fn mcp_app_for_item(
		&self,
		thread: &str,
		turn: &str,
		item: &str,
	) -> Result<Option<NativeAppUi>, ClientError> {
		if [thread, turn, item].iter().any(|value| value.is_empty() || value.len() > 4096) {
			return Err(ClientError::InvalidFrame);
		}
		let guard = self.thread_settings_guard(thread).ok_or(ClientError::InvalidFrame)?;
		let history = self.thread_read_turn(thread, turn).await?;
		let Some(selected) = select_item(&history, thread, turn, item)? else {
			return Ok(None);
		};
		let Some(target) = resource_target(selected)? else {
			return Ok(None);
		};
		if !guard.is_live() {
			return Err(ClientError::InvalidFrame);
		}
		let resources = self
			.mcp_app_resource(thread, target.server, item, target.uri, target.connector)
			.await?;
		if !guard.is_live() {
			return Err(ClientError::InvalidFrame);
		}
		Ok(Some(NativeAppUi { item: selected.clone(), resources, guard }))
	}

	/// Read one widget resource through its loaded native thread and originating call.
	/// The consumer must verify its account/process source before displaying the result.
	/// Resource metadata is preserved for sandbox policy; this grants no tool authority.
	pub async fn mcp_app_resource(
		&self,
		thread: &str,
		server: &str,
		origin_call: &str,
		uri: &str,
		connector: Option<&str>,
	) -> Result<Vec<Value>, ClientError> {
		for value in [Some(thread), Some(server), Some(origin_call), Some(uri), connector]
			.into_iter()
			.flatten()
		{
			if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
				return Err(ClientError::InvalidFrame);
			}
		}
		if !uri.starts_with("ui://") || uri.len() == 5 {
			return Err(ClientError::InvalidFrame);
		}
		let response = tokio::time::timeout(
			std::time::Duration::from_secs(30),
			self.request(
				"mcpServer/resource/read",
				json!({
					"threadId":thread,"server":server,"originCallId":origin_call,
					"uri":uri,"connectorId":connector
				}),
			),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		// Native codex_apps can select an app through its originating tool call.
		// Never silently accept a global-discovery fallback for that scoped read.
		if server == "codex_apps" && response["originCallId"].as_str() != Some(origin_call) {
			return Err(ClientError::InvalidFrame);
		}
		let contents = response["contents"].as_array().ok_or(ClientError::InvalidFrame)?;
		for part in contents {
			if !part["uri"].is_string()
				|| (part["text"].is_string() == part["blob"].is_string())
				|| (!part["mimeType"].is_null() && !part["mimeType"].is_string())
			{
				return Err(ClientError::InvalidFrame);
			}
		}
		Ok(contents.clone())
	}
}

fn select_item<'a>(
	history: &'a Value,
	thread: &str,
	turn: &str,
	item: &str,
) -> Result<Option<&'a Value>, ClientError> {
	if history["thread"]["id"] != thread {
		return Err(ClientError::InvalidFrame);
	}
	let turns = history["thread"]["turns"].as_array().ok_or(ClientError::InvalidFrame)?;
	let mut selected = turns.iter().filter(|value| value["id"] == turn);
	let Some(found) = selected.next() else { return Ok(None) };
	if selected.next().is_some() {
		return Err(ClientError::InvalidFrame);
	}
	let items = found["items"].as_array().ok_or(ClientError::InvalidFrame)?;
	let mut selected = items.iter().filter(|value| value["id"] == item);
	let found = selected.next();
	if selected.next().is_some() {
		return Err(ClientError::InvalidFrame);
	}
	Ok(found)
}

#[derive(Debug, PartialEq)]
struct ResourceTarget<'a> {
	server: &'a str,
	uri: &'a str,
	connector: Option<&'a str>,
}

fn resource_target(item: &Value) -> Result<Option<ResourceTarget<'_>>, ClientError> {
	if item["type"] != "mcpToolCall" {
		return Ok(None);
	}
	let uri = item
		.pointer("/mcpAppUi/resourceUri")
		.filter(|value| !value.is_null())
		.or_else(|| item.get("mcpAppResourceUri").filter(|value| !value.is_null()))
		.or_else(|| item.pointer("/appContext/resourceUri").filter(|value| !value.is_null()));
	let Some(uri) = uri else { return Ok(None) };
	let uri = uri.as_str().ok_or(ClientError::InvalidFrame)?;
	let server = item["server"].as_str().ok_or(ClientError::InvalidFrame)?;
	let connector = match item.pointer("/appContext/connectorId") {
		None | Some(Value::Null) => None,
		Some(Value::String(value)) => Some(value.as_str()),
		_ => return Err(ClientError::InvalidFrame),
	};
	Ok(Some(ResourceTarget { server, uri, connector }))
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

	#[tokio::test]
	async fn resource_read_preserves_scope_and_sandbox_metadata_without_tool_calls() {
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let resource = json!({"uri":"ui://fixture/view","mimeType":"text/html;profile=mcp-app",
			"text":"<button>Test</button>","_meta":{"ui":{"csp":{"connectDomains":[]}},"extension":7}});
		let expected = resource.clone();
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			for scope in [Some("call"), None, Some("other")] {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], "mcpServer/resource/read");
				assert_eq!(
					request["params"],
					json!({"threadId":"thread","server":"codex_apps",
					"originCallId":"call","uri":"ui://fixture/view","connectorId":"connector"})
				);
				let reply = json!({"id":request["id"],"result":{"contents":[resource],"originCallId":scope}});
				writer.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
			}
		});
		assert_eq!(
			client
				.mcp_app_resource(
					"thread",
					"codex_apps",
					"call",
					"ui://fixture/view",
					Some("connector")
				)
				.await
				.unwrap(),
			vec![expected]
		);
		for _ in 0..2 {
			assert!(matches!(
				client
					.mcp_app_resource(
						"thread",
						"codex_apps",
						"call",
						"ui://fixture/view",
						Some("connector")
					)
					.await,
				Err(ClientError::InvalidFrame)
			));
		}
		server.await.unwrap();
	}
	#[tokio::test]
	async fn unavailable_resource_is_not_retried_or_replaced_by_global_discovery() {
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], "mcpServer/resource/read");
			let reply = json!({"id":request["id"],"error":{"code":-32601,"message":"Unavailable"}});
			writer.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
			assert!(
				tokio::time::timeout(std::time::Duration::from_millis(100), lines.next_line())
					.await
					.is_err()
			);
		});
		for (thread, uri) in [("", "ui://fixture/view"), ("thread", "https://example.com")] {
			assert!(matches!(
				client.mcp_app_resource(thread, "widget", "call", uri, None).await,
				Err(ClientError::InvalidFrame)
			));
		}
		assert!(
			matches!(client.mcp_app_resource("thread", "widget", "call", "ui://fixture/view", None).await, Err(ClientError::Remote(error)) if error.code == -32601)
		);
		server.await.unwrap();
	}
	#[tokio::test]
	async fn exact_history_resolves_widget_and_rejects_reverted_source() {
		for reverted in [false, true] {
			let (local, remote) = tokio::io::duplex(65536);
			let (reader, writer) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let (release, hold) = tokio::sync::oneshot::channel::<()>();
			let server = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let mut lines = BufReader::new(reader).lines();
				for (method, result) in [
					("thread/read", json!({"thread":{"id":"thread","historyMode":"paginated"}})),
					("thread/turns/list", json!({"data":[{"id":"turn"}],"nextCursor":null})),
					(
						"thread/items/list",
						json!({"data":[{"turnId":"turn","item":{
						"id":"call","type":"mcpToolCall","server":"widget","tool":"counter",
						"arguments":{"value":7},"mcpAppUi":{"resourceUri":"ui://fixture/view"},
						"mcpAppResourceUri":"ui://obsolete/view","result":{"content":[]}
					}}],"nextCursor":null}),
					),
					(
						"mcpServer/resource/read",
						json!({"contents":[{"uri":"ui://fixture/view","text":"<button>7</button>"}]}),
					),
				] {
					let request: Value =
						serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
					assert_eq!(request["method"], method);
					assert_eq!(request["params"]["threadId"], "thread");
					if method == "mcpServer/resource/read" {
						assert_eq!(request["params"]["uri"], "ui://fixture/view");
						assert_eq!(request["params"]["originCallId"], "call");
						if reverted {
							writer.write_all(b"{\"method\":\"thread/reverted\",\"params\":{\"threadId\":\"thread\"}}\n").await.unwrap();
						}
					}
					writer
						.write_all(
							format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
						)
						.await
						.unwrap();
				}
				let _ = hold.await;
			});
			let result = client.mcp_app_for_item("thread", "turn", "call").await;
			if reverted {
				assert!(matches!(result, Err(ClientError::InvalidFrame)));
			} else {
				let widget = result.unwrap().unwrap();
				assert_eq!(widget.item["arguments"]["value"], 7);
				assert_eq!(widget.resources[0]["text"], "<button>7</button>");
				assert!(widget.guard.is_live());
			}
			release.send(()).unwrap();
			server.await.unwrap();
		}
	}
	#[test]
	fn widget_selection_rejects_ambiguous_identity_and_preserves_legacy_sources() {
		let item = json!({"id":"call","type":"mcpToolCall","server":"widget",
			"appContext":{"resourceUri":"ui://legacy/view","connectorId":"connector"}});
		assert_eq!(
			resource_target(&item).unwrap(),
			Some(ResourceTarget {
				server: "widget",
				uri: "ui://legacy/view",
				connector: Some("connector")
			})
		);
		let mut history =
			json!({"thread":{"id":"thread","turns":[{"id":"turn","items":[item.clone()]}]}});
		assert_eq!(select_item(&history, "thread", "turn", "call").unwrap(), Some(&item));
		assert!(select_item(&history, "other", "turn", "call").is_err());
		assert!(select_item(&history, "thread", "turn", "missing").unwrap().is_none());
		history["thread"]["turns"][0]["items"] = json!([item.clone(), item]);
		assert!(select_item(&history, "thread", "turn", "call").is_err());
		assert!(
			resource_target(
				&json!({"type":"agentMessage","mcpAppUi":{"resourceUri":"ui://wrong"}})
			)
			.unwrap()
			.is_none()
		);
		assert!(resource_target(&json!({"type":"mcpToolCall","server":"widget","mcpAppUi":{"resourceUri":7},"mcpAppResourceUri":"ui://fallback"})).is_err());
	}
}
