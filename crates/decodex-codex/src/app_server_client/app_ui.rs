//! Native resource transport for source-bound MCP App UI consumers.
use super::{AppServerClient, ClientError};
use serde_json::{Value, json};

impl AppServerClient {
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
}
