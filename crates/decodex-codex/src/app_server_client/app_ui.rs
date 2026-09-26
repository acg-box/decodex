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

/// Fresh native evidence for a later explicitly confirmed widget call.
#[derive(Clone)]
pub struct NativeAppUiToolReview {
	/// Exact originating native item.
	pub origin: Value,
	/// Live tool descriptor, including schema and visibility metadata.
	pub descriptor: Value,
	/// Raw server identity used by native dispatch.
	pub server: String,
	/// Native history/settings/connection validity guard.
	pub guard: HistoryGuard,
}

impl AppServerClient {
	/// Resolve callback ownership using native history and the live thread catalog.
	/// This reads evidence only; it does not approve, reserve or execute the call.
	pub async fn review_mcp_app_tool(
		&self,
		thread: &str,
		turn: &str,
		item: &str,
		tool: &str,
		arguments: &Value,
	) -> Result<Option<NativeAppUiToolReview>, ClientError> {
		if [thread, turn, item, tool]
			.iter()
			.any(|id| id.is_empty() || id.len() > 4096 || id.chars().any(char::is_control))
			|| !arguments.is_object()
		{
			return Err(ClientError::InvalidFrame);
		}
		let guard = self.thread_settings_guard(thread).ok_or(ClientError::InvalidFrame)?;
		let history = self.thread_read_turn(thread, turn).await?;
		let Some(origin) = select_item(&history, thread, turn, item)? else { return Ok(None) };
		let Some(target) = resource_target(origin)? else { return Ok(None) };
		if !target.uri.starts_with("ui://") || target.uri.len() == 5 {
			return Ok(None);
		}
		let catalog = self.mcp_server_statuses(thread).await?;
		let Some(descriptor) = callback_tool(&catalog, target.server, tool)? else {
			return Ok(None);
		};
		if target.server == "codex_apps" {
			let Some(connector) = target.connector else { return Ok(None) };
			let apps = self
				.request(
					"app/read",
					json!({"threadId":thread,"appIds":[connector],"includeTools":true}),
				)
				.await?;
			if !hosted_callback_owned(&apps, connector, tool, &descriptor, origin, arguments)? {
				return Ok(None);
			}
		}
		if !guard.is_live() {
			return Err(ClientError::InvalidFrame);
		}
		Ok(Some(NativeAppUiToolReview {
			origin: origin.clone(),
			descriptor,
			server: target.server.into(),
			guard,
		}))
	}

	/// Execute one explicitly confirmed and durably reserved widget tool call.
	/// The caller owns origin/catalog validation and uncertain-outcome recovery.
	/// A transport or malformed-response error is not proof that no effect occurred.
	pub async fn call_mcp_app_tool(
		&self,
		thread: &str,
		server: &str,
		tool: &str,
		arguments: Value,
		guard: HistoryGuard,
	) -> Result<Value, ClientError> {
		if [thread, server, tool].iter().any(|value| {
			value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control)
		}) || !arguments.is_object()
		{
			return Err(ClientError::InvalidFrame);
		}
		// Widget-supplied transport metadata is never forwarded. Account and thread
		// routing remain with the native process and its retained connection.
		let result = tokio::time::timeout(
			std::time::Duration::from_secs(60),
			self.request_with_history(
				"mcpServer/tool/call",
				json!({"threadId":thread,"server":server,"tool":tool,"arguments":arguments}),
				guard,
			),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		if !result["content"].is_array()
			|| (!result["isError"].is_null() && !result["isError"].is_boolean())
		{
			return Err(ClientError::InvalidFrame);
		}
		Ok(result)
	}

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

fn callback_tool(
	catalog: &[Value],
	server: &str,
	tool: &str,
) -> Result<Option<Value>, ClientError> {
	let mut servers = catalog.iter().filter(|row| row["name"] == server);
	let Some(server) = servers.next() else { return Ok(None) };
	if servers.next().is_some() {
		return Err(ClientError::InvalidFrame);
	}
	if server["runtimeStatus"] != "connected" || !server["toolsError"].is_null() {
		return Ok(None);
	}
	let tools = server["tools"].as_object().ok_or(ClientError::InvalidFrame)?;
	let mut matches = tools.values().filter(|value| value["name"] == tool);
	let Some(tool) = matches.next() else { return Ok(None) };
	if matches.next().is_some() {
		return Err(ClientError::InvalidFrame);
	}
	if let Some(visibility) = tool.pointer("/_meta/ui/visibility") {
		let visibility = visibility.as_array().ok_or(ClientError::InvalidFrame)?;
		if !visibility.iter().any(|v| v == "app") {
			return Ok(None);
		}
	}
	if tool.pointer("/_meta/openai~1widgetAccessible") == Some(&Value::Bool(false)) {
		return Ok(None);
	}
	Ok(Some(tool.clone()))
}

fn hosted_callback_owned(
	apps: &Value,
	connector: &str,
	name: &str,
	tool: &Value,
	origin: &Value,
	arguments: &Value,
) -> Result<bool, ClientError> {
	let rows = apps["apps"].as_array().ok_or(ClientError::InvalidFrame)?;
	let missing = apps["missingAppIds"].as_array().ok_or(ClientError::InvalidFrame)?;
	if missing.iter().any(|id| id == connector) {
		return Ok(false);
	}
	let mut matched = rows.iter().filter(|app| app["id"] == connector);
	let Some(app) = matched.next() else { return Ok(false) };
	if matched.next().is_some() {
		return Err(ClientError::InvalidFrame);
	}
	let Some(tools) = app["toolSummaries"].as_array() else { return Ok(false) };
	let mut matched = tools.iter().filter(|tool| tool["name"] == name);
	let Some(summary) = matched.next() else { return Ok(false) };
	if matched.next().is_some() || summary["isEnabled"] != true {
		return Ok(false);
	}
	let explicit =
		tool.pointer("/_meta/_codex_apps/requires_explicit_link_id") == Some(&Value::Bool(true));
	let link = if explicit { arguments.get("link_id") } else { tool.pointer("/_meta/link_id") };
	let link = link.and_then(Value::as_str).filter(|link| !link.trim().is_empty());
	if explicit && link.is_none() {
		return Ok(false);
	}
	let origin_link = origin
		.pointer("/appContext/linkId")
		.and_then(Value::as_str)
		.filter(|link| !link.trim().is_empty());
	Ok(link == origin_link)
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
	#[tokio::test]
	async fn confirmed_tool_transport_preserves_result_and_never_retries_uncertainty() {
		for mode in ["success", "lost", "malformed"] {
			let (local, remote) = tokio::io::duplex(65536);
			let (reader, writer) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let task = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let mut lines = BufReader::new(reader).lines();
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], "mcpServer/tool/call");
				assert_eq!(
					request["params"],
					json!({"threadId":"thread","server":"widget","tool":"counter","arguments":{"value":7}})
				);
				if mode == "lost" {
					return;
				}
				let result = if mode == "malformed" {
					json!({"content":false})
				} else {
					json!({"content":[],"structuredContent":{"value":7},"_meta":{"privateViewState":"retained"},"isError":false})
				};
				writer
					.write_all(
						format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
					)
					.await
					.unwrap();
				assert!(
					tokio::time::timeout(std::time::Duration::from_millis(100), lines.next_line())
						.await
						.is_err()
				);
			});
			let result = client
				.call_mcp_app_tool(
					"thread",
					"widget",
					"counter",
					json!({"value":7}),
					client.thread_settings_guard("thread").unwrap(),
				)
				.await;
			if mode == "success" {
				let result = result.unwrap();
				assert_eq!(result["structuredContent"]["value"], 7);
				assert_eq!(result["_meta"]["privateViewState"], "retained");
			} else {
				assert!(result.is_err());
			}
			task.await.unwrap();
		}
	}
	#[test]
	fn callback_catalog_requires_app_visibility_and_exact_hosted_account() {
		let tool = json!({"name":"calendar.find","inputSchema":{"type":"object"},"_meta":{"ui":{"visibility":["app"]},"link_id":"link-1"}});
		let catalog = vec![
			json!({"name":"codex_apps","runtimeStatus":"connected","tools":{"display-name":tool},"toolsError":null}),
		];
		let selected = callback_tool(&catalog, "codex_apps", "calendar.find").unwrap().unwrap();
		assert!(callback_tool(&catalog, "codex_apps", "other.find").unwrap().is_none());
		let mut hidden = catalog.clone();
		hidden[0]["tools"]["display-name"]["_meta"]["ui"]["visibility"] = json!(["model"]);
		assert!(callback_tool(&hidden, "codex_apps", "calendar.find").unwrap().is_none());
		let apps = json!({"apps":[{"id":"calendar","toolSummaries":[{"name":"calendar.find","isEnabled":true}]}],"missingAppIds":[]});
		let origin = json!({"appContext":{"connectorId":"calendar","linkId":"link-1"}});
		assert!(
			hosted_callback_owned(
				&apps,
				"calendar",
				"calendar.find",
				&selected,
				&origin,
				&json!({})
			)
			.unwrap()
		);
		assert!(
			!hosted_callback_owned(&apps, "other", "calendar.find", &selected, &origin, &json!({}))
				.unwrap()
		);
		let mut explicit = selected.clone();
		explicit["_meta"]["_codex_apps"] = json!({"requires_explicit_link_id":true});
		for arguments in [json!({}), json!({"link_id":"link-2"}), json!({"link_id":false})] {
			assert!(
				!hosted_callback_owned(
					&apps,
					"calendar",
					"calendar.find",
					&explicit,
					&origin,
					&arguments
				)
				.unwrap()
			);
		}
		assert!(
			hosted_callback_owned(
				&apps,
				"calendar",
				"calendar.find",
				&explicit,
				&origin,
				&json!({"link_id":"link-1"})
			)
			.unwrap()
		);
	}

	#[tokio::test]
	async fn tool_review_reads_native_app_membership_without_executing() {
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
					json!({"data":[{"turnId":"turn","item":{"id":"call","type":"mcpToolCall","server":"codex_apps","mcpAppUi":{"resourceUri":"ui://fixture/view"},"appContext":{"connectorId":"calendar","linkId":"link-1"}}}],"nextCursor":null}),
				),
				(
					"mcpServerStatus/list",
					json!({"data":[{"name":"codex_apps","runtimeStatus":"connected","tools":{"normalized":{"name":"calendar.find","inputSchema":{"type":"object"},"_meta":{"link_id":"link-1"}}},"authStatus":"oAuth","resources":[],"resourceTemplates":[]}],"nextCursor":null}),
				),
				(
					"app/read",
					json!({"apps":[{"id":"calendar","toolSummaries":[{"name":"calendar.find","isEnabled":true}]}],"missingAppIds":[]}),
				),
			] {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], method);
				if method == "app/read" {
					assert_eq!(
						request["params"],
						json!({"threadId":"thread","appIds":["calendar"],"includeTools":true})
					);
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
		let review = client
			.review_mcp_app_tool(
				"thread",
				"turn",
				"call",
				"calendar.find",
				&json!({"query":"meeting"}),
			)
			.await
			.unwrap()
			.unwrap();
		assert_eq!(review.server, "codex_apps");
		assert_eq!(review.descriptor["name"], "calendar.find");
		assert!(review.guard.is_live());
		release.send(()).unwrap();
		server.await.unwrap();
	}
	#[tokio::test]
	async fn tool_dispatch_rejects_foreign_review_guard_before_writing() {
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let (other, other_remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(other);
		let (other_client, _other_events) = AppServerClient::from_io(reader, writer);
		let guard = other_client.thread_settings_guard("thread").unwrap();
		assert!(matches!(
			client.call_mcp_app_tool("thread", "widget", "counter", json!({}), guard).await,
			Err(ClientError::StaleHistory)
		));
		let mut lines = BufReader::new(remote).lines();
		assert!(
			tokio::time::timeout(std::time::Duration::from_millis(100), lines.next_line())
				.await
				.is_err()
		);
		drop(other_remote);
	}
}
