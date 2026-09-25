//! Source-bound integration discovery. Catalog metadata is not runtime readiness.
use super::{AppServerClient, ClientError, MAX_FRAME_BYTES};
use serde_json::{Value, json};
use std::collections::HashSet;

impl AppServerClient {
	/// Read installed connector state for one loaded native thread. An explicit
	/// refresh publishes its live tools; failure must not become an empty inventory.
	pub async fn installed_apps_for_thread(
		&self,
		thread: &str,
		force_refresh: bool,
	) -> Result<Vec<Value>, ClientError> {
		if thread.is_empty() || thread.len() > 4096 || thread.chars().any(char::is_control) {
			return Err(ClientError::InvalidFrame);
		}
		let response = tokio::time::timeout(
			std::time::Duration::from_secs(30),
			self.request("app/installed", json!({"threadId":thread,"forceRefresh":force_refresh})),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		let rows = response["apps"].as_array().ok_or(ClientError::InvalidFrame)?;
		let mut ids = HashSet::new();
		for row in rows {
			let id = row["id"]
				.as_str()
				.filter(|id| {
					!id.is_empty() && id.len() <= 4096 && !id.chars().any(char::is_control)
				})
				.ok_or(ClientError::InvalidFrame)?;
			if !ids.insert(id)
				|| !row["enabled"].is_boolean()
				|| !row["callable"].is_boolean()
				|| (!row["runtimeName"].is_null() && !row["runtimeName"].is_string())
				|| (row["callable"] == true && row["enabled"] == false)
			{
				return Err(ClientError::InvalidFrame);
			}
		}
		Ok(rows.clone())
	}

	/// Explicitly synchronize installed plugin bundles, then request native MCP reload.
	/// Partial reconciliation remains visible and no write is automatically retried.
	pub async fn refresh_integrations(&self) -> Result<bool, ClientError> {
		tokio::time::timeout(std::time::Duration::from_secs(45), async {
			let receipt = self
				.request(
					"plugin/reconcile",
					json!({"reason":"explicit Decodex integration refresh"}),
				)
				.await?;
			let changed = receipt["changedPlugins"].as_array().ok_or(ClientError::InvalidFrame)?;
			for plugin in changed {
				if !plugin["id"].is_string()
					|| ["hasMcps", "hasApps", "hasHooks", "hasSkills"]
						.iter()
						.any(|field| !plugin[field].is_boolean())
				{
					return Err(ClientError::InvalidFrame);
				}
			}
			let mut partial = false;
			for field in ["failedRemotePluginIds", "failedMaterializationRemotePluginIds"] {
				let failed = receipt[field].as_array().ok_or(ClientError::InvalidFrame)?;
				if failed.iter().any(|id| !id.is_string()) {
					return Err(ClientError::InvalidFrame);
				}
				partial |= !failed.is_empty();
			}
			// This reload affects loaded native threads, preserving each thread's
			// own repository config layers. A receipt does not prove connection readiness.
			let response = self.request("config/mcpServer/reload", Value::Null).await?;
			if response != json!({}) {
				return Err(ClientError::InvalidFrame);
			}
			Ok(!partial)
		})
		.await
		.map_err(|_| ClientError::Io)?
	}

	/// Read complete MCP inventory for an exact thread, preserving independent
	/// runtime/auth/discovery states. Errors never become an empty catalog.
	pub async fn mcp_server_statuses(&self, thread: &str) -> Result<Vec<Value>, ClientError> {
		if thread.is_empty() {
			return Err(ClientError::InvalidFrame);
		}
		tokio::time::timeout(std::time::Duration::from_secs(30), async {
			let mut rows = Vec::new();
			let mut names = HashSet::new();
			let mut cursors = HashSet::new();
			let mut cursor: Option<String> = None;
			let mut budget = MAX_FRAME_BYTES;
			for _ in 0..128 {
				let page = self
					.request(
						"mcpServerStatus/list",
						json!({"threadId":thread,"detail":"full","limit":100,"cursor":cursor}),
					)
					.await?;
				budget = budget
					.checked_sub(page.to_string().len())
					.ok_or(ClientError::CapacityExceeded)?;
				let data = page["data"].as_array().ok_or(ClientError::InvalidFrame)?;
				for row in data {
					let name = row["name"]
						.as_str()
						.filter(|name| !name.is_empty())
						.ok_or(ClientError::InvalidFrame)?;
					if !names.insert(name.to_owned())
						|| !row["tools"].is_object()
						|| !row["authStatus"].is_string()
						|| !row["resources"].is_array()
						|| !row["resourceTemplates"].is_array()
					{
						return Err(ClientError::InvalidFrame);
					}
					for key in ["runtimeStatus", "toolsError", "pluginId"] {
						if !row[key].is_null() && !row[key].is_string() {
							return Err(ClientError::InvalidFrame);
						}
					}
					rows.push(row.clone());
				}
				match page.get("nextCursor") {
					Some(Value::Null) => return Ok(rows),
					Some(Value::String(next))
						if !next.is_empty()
							&& next.len() <= 4096 && cursors.insert(next.clone()) =>
						cursor = Some(next.clone()),
					_ => return Err(ClientError::InvalidFrame),
				}
			}
			Err(ClientError::CapacityExceeded)
		})
		.await
		.map_err(|_| ClientError::Io)?
	}

	/// Discover installed plugins for this repository, not only home-scoped defaults.
	/// Keep marketplace errors in the native response for an explicit partial-state UI.
	pub async fn installed_plugins_for_directory(&self, cwd: &str) -> Result<Value, ClientError> {
		if !std::path::Path::new(cwd).is_absolute() {
			return Err(ClientError::InvalidFrame);
		}
		let result = tokio::time::timeout(
			std::time::Duration::from_secs(30),
			self.request("plugin/installed", json!({"cwds":[cwd]})),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		if !result["marketplaces"].is_array()
			|| result.get("marketplaceLoadErrors").is_some_and(|errors| !errors.is_array())
		{
			return Err(ClientError::InvalidFrame);
		}
		Ok(result)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
	fn server(name: &str, status: Value, error: Value) -> Value {
		json!({"name":name,"runtimeStatus":status,"authStatus":"notLoggedIn","tools":{},"toolsError":error,"resources":[],"resourceTemplates":[],"serverCapabilities":{"resources":{}},"pluginId":null})
	}
	#[tokio::test]
	async fn discovery_keeps_failure_distinct_from_empty_and_uses_exact_scope() {
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			for (method, response, cursor) in [
				(
					"mcpServerStatus/list",
					json!({"data":[server("broken",json!("authenticationRequired"),json!("Discovery failed"))],"nextCursor":"page2"}),
					Value::Null,
				),
				(
					"mcpServerStatus/list",
					json!({"data":[server("empty",json!("connected"),Value::Null)],"nextCursor":null}),
					json!("page2"),
				),
				(
					"plugin/installed",
					json!({"marketplaces":[],"marketplaceLoadErrors":[{"marketplacePath":"/repo/marketplace.json","message":"Invalid repository configuration"}]}),
					Value::Null,
				),
			] {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], method);
				if method == "mcpServerStatus/list" {
					assert_eq!(request["params"]["threadId"], "exact-thread");
					assert_eq!(request["params"]["cursor"], cursor);
					assert_eq!(request["params"]["detail"], "full");
				} else {
					assert_eq!(request["params"], json!({"cwds":["/repo"]}));
				}
				writer
					.write_all(
						format!("{}\n", json!({"id":request["id"],"result":response})).as_bytes(),
					)
					.await
					.unwrap();
			}
		});
		let statuses = client.mcp_server_statuses("exact-thread").await.unwrap();
		assert_eq!(statuses.len(), 2);
		assert_eq!(statuses[0]["toolsError"], "Discovery failed");
		assert!(statuses[1]["toolsError"].is_null());
		assert_eq!(statuses[0]["runtimeStatus"], "authenticationRequired");
		let plugins = client.installed_plugins_for_directory("/repo").await.unwrap();
		assert_eq!(plugins["marketplaceLoadErrors"].as_array().unwrap().len(), 1);
		server.await.unwrap();
	}
	#[tokio::test]
	async fn repeated_cursor_is_not_a_successful_partial_inventory() {
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			for _ in 0..2 {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				writer
					.write_all(
						format!(
							"{}\n",
							json!({"id":request["id"],"result":{"data":[],"nextCursor":"repeat"}})
						)
						.as_bytes(),
					)
					.await
					.unwrap();
			}
		});
		assert!(matches!(
			client.mcp_server_statuses("thread").await,
			Err(ClientError::InvalidFrame)
		));
		server.await.unwrap();
	}
	#[tokio::test]
	async fn explicit_refresh_reloads_after_partial_reconcile_without_claiming_readiness() {
		for partial in [false, true] {
			let (local, remote) = tokio::io::duplex(65536);
			let (reader, writer) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let server = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let mut lines = BufReader::new(reader).lines();
				for (method, result) in [
					(
						"plugin/reconcile",
						json!({"changedPlugins":[{"id":"example@market","hasMcps":true,"hasApps":false,"hasHooks":false,"hasSkills":true}],"failedRemotePluginIds":if partial {json!(["failed-plugin"])} else {json!([])},"failedMaterializationRemotePluginIds":[]}),
					),
					("config/mcpServer/reload", json!({})),
				] {
					let request: Value =
						serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
					assert_eq!(request["method"], method);
					if method == "config/mcpServer/reload" {
						assert!(request["params"].is_null());
					}
					writer
						.write_all(
							format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
						)
						.await
						.unwrap();
				}
			});
			assert_eq!(client.refresh_integrations().await.unwrap(), !partial);
			server.await.unwrap();
		}
	}
}

#[cfg(test)]
#[path = "installed_apps_tests.rs"]
mod installed_apps_tests;
