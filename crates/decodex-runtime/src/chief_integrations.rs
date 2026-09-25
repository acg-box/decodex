//! Read each integration source independently, scoped to the native task directory.
use decodex_codex::app_server_client::{AppServerClient, ClientError};
use decodex_protocol::{
	ChiefAppInventory, ChiefAppStatusDto, ChiefIntegrationsResult, ChiefMcpInventory,
	ChiefMcpStatusDto, ChiefPluginInventory, ChiefPluginStatusDto,
};
use serde_json::{Value, json};

pub(crate) async fn read(client: &AppServerClient, thread: &str) -> ChiefIntegrationsResult {
	let result = tokio::time::timeout(std::time::Duration::from_secs(35), async {
		let guard = client.thread_settings_guard(thread)?;
		let before = client.thread_read(json!({"threadId":thread})).await.ok()?;
		let cwd = thread_cwd(&before, thread)?.to_owned();
		let (mcp, plugins, apps) = tokio::join!(
			client.mcp_server_statuses(thread),
			client.installed_plugins_for_directory(&cwd),
			client.installed_apps_for_thread(thread, false)
		);
		let after = client.thread_read(json!({"threadId":thread})).await.ok()?;
		if !guard.is_live() || thread_cwd(&after, thread) != Some(cwd.as_str()) {
			return None;
		}
		let result = ChiefIntegrationsResult::Available {
			cwd,
			mcp: project_mcp(mcp),
			plugins: project_plugins(plugins),
			apps: project_apps(apps),
		};
		Some(if serde_json::to_vec(&result).ok()?.len() > 64 * 1024 {
			ChiefIntegrationsResult::CapacityExceeded
		} else {
			result
		})
	})
	.await;
	result.ok().flatten().unwrap_or(ChiefIntegrationsResult::Unavailable)
}

fn thread_cwd<'a>(value: &'a Value, thread: &str) -> Option<&'a str> {
	if value["thread"]["id"].as_str() != Some(thread) {
		return None;
	}
	value["thread"]["cwd"].as_str().filter(|cwd| std::path::Path::new(cwd).is_absolute())
}
fn text(value: &str) -> String {
	if decodex_core::contains_credential_material(value) {
		return "Sensitive details omitted".into();
	}
	value.chars().take(2048).collect()
}
fn optional(value: &Value) -> Option<String> {
	value.as_str().map(text)
}

fn project_apps(result: Result<Vec<Value>, ClientError>) -> ChiefAppInventory {
	let rows = match result {
		Ok(rows) => rows,
		Err(ClientError::Remote(error)) if error.code == -32601 =>
			return ChiefAppInventory::Unsupported,
		Err(ClientError::CapacityExceeded) => return ChiefAppInventory::CapacityExceeded,
		Err(_) => return ChiefAppInventory::Unavailable,
	};
	if rows.len() > 128 {
		return ChiefAppInventory::CapacityExceeded;
	}
	let mut apps = Vec::new();
	for row in rows {
		let (Some(id), Some(enabled), Some(callable)) =
			(row["id"].as_str(), row["enabled"].as_bool(), row["callable"].as_bool())
		else {
			return ChiefAppInventory::Unavailable;
		};
		if decodex_core::contains_credential_material(id) {
			return ChiefAppInventory::Unavailable;
		}
		apps.push(ChiefAppStatusDto {
			id: id.into(),
			runtime_name: optional(&row["runtimeName"]),
			enabled,
			callable,
		});
	}
	ChiefAppInventory::Available { apps }
}

fn project_mcp(result: Result<Vec<Value>, ClientError>) -> ChiefMcpInventory {
	let rows = match result {
		Ok(rows) => rows,
		Err(ClientError::Remote(error)) if error.code == -32601 =>
			return ChiefMcpInventory::Unsupported,
		Err(ClientError::CapacityExceeded) => return ChiefMcpInventory::CapacityExceeded,
		Err(_) => return ChiefMcpInventory::Unavailable,
	};
	if rows.len() > 128 {
		return ChiefMcpInventory::CapacityExceeded;
	}
	let mut servers = Vec::new();
	for row in rows {
		let (Some(name), Some(auth), Some(tools), Some(resources), Some(templates)) = (
			row["name"].as_str(),
			row["authStatus"].as_str(),
			row["tools"].as_object(),
			row["resources"].as_array(),
			row["resourceTemplates"].as_array(),
		) else {
			return ChiefMcpInventory::Unavailable;
		};
		if name.len() > 4096 {
			return ChiefMcpInventory::CapacityExceeded;
		}
		servers.push(ChiefMcpStatusDto {
			name: name.into(),
			plugin_id: optional(&row["pluginId"]),
			runtime_status: optional(&row["runtimeStatus"]),
			auth_status: text(auth),
			tool_count: tools.len(),
			tools_error: optional(&row["toolsError"]),
			resource_count: resources.len(),
			template_count: templates.len(),
			advertised_capabilities: row["serverCapabilities"]
				.as_object()
				.map(|object| object.keys().map(|key| text(key)).collect()),
		});
	}
	ChiefMcpInventory::Available { servers }
}

pub(crate) fn project_plugins(result: Result<Value, ClientError>) -> ChiefPluginInventory {
	let value = match result {
		Ok(value) => value,
		Err(ClientError::Remote(error)) if error.code == -32601 =>
			return ChiefPluginInventory::Unsupported,
		Err(ClientError::CapacityExceeded) => return ChiefPluginInventory::CapacityExceeded,
		Err(_) => return ChiefPluginInventory::Unavailable,
	};
	let Some(markets) = value["marketplaces"].as_array() else {
		return ChiefPluginInventory::Unavailable;
	};
	let mut plugins = Vec::new();
	let mut ids = std::collections::HashSet::new();
	for market in markets {
		let Some(rows) = market["plugins"].as_array() else {
			return ChiefPluginInventory::Unavailable;
		};
		for row in rows {
			let (Some(id), Some(name), Some(installed), Some(enabled)) = (
				row["id"].as_str(),
				row["name"].as_str(),
				row["installed"].as_bool(),
				row["enabled"].as_bool(),
			) else {
				return ChiefPluginInventory::Unavailable;
			};
			if !ids.insert(id.to_owned()) {
				return ChiefPluginInventory::Unavailable;
			}
			if plugins.len() >= 128 || id.len() > 4096 {
				return ChiefPluginInventory::CapacityExceeded;
			}
			plugins.push(ChiefPluginStatusDto {
				id: id.into(),
				name: text(name),
				installed,
				enabled,
				availability: row["availability"]
					.as_str()
					.map(text)
					.unwrap_or_else(|| "unknown".into()),
				disabled_reason: optional(&row["disabledReason"]),
			});
		}
	}
	let errors = value["marketplaceLoadErrors"]
		.as_array()
		.into_iter()
		.flatten()
		.map(|error| {
			error["message"]
				.as_str()
				.map(text)
				.unwrap_or_else(|| "Marketplace could not be loaded".into())
		})
		.collect::<Vec<_>>();
	if errors.len() > 128 {
		return ChiefPluginInventory::CapacityExceeded;
	}
	ChiefPluginInventory::Available { plugins, errors }
}

#[cfg(test)]
mod tests {
	use super::*;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

	#[test]
	fn failed_discovery_and_plugin_policy_are_not_readiness() {
		let mcp = project_mcp(Ok(vec![
			json!({"name":"server","runtimeStatus":"authenticationRequired","authStatus":"notLoggedIn","tools":{},"toolsError":"Discovery failed","resources":[],"resourceTemplates":[],"serverCapabilities":{"tools":{},"resources":{}}}),
		]));
		let ChiefMcpInventory::Available { servers } = mcp else {
			panic!("inventory");
		};
		assert_eq!(servers[0].tool_count, 0);
		assert_eq!(servers[0].tools_error.as_deref(), Some("Discovery failed"));
		assert_eq!(servers[0].runtime_status.as_deref(), Some("authenticationRequired"));
		assert_eq!(servers[0].advertised_capabilities.as_ref().unwrap().len(), 2);
		let plugins = project_plugins(Ok(
			json!({"marketplaces":[{"plugins":[{"id":"example@market","name":"Example","installed":true,"enabled":false,"availability":"DISABLED_BY_ADMIN","disabledReason":"disabled_by_admin"}]}],"marketplaceLoadErrors":[{"message":"Another marketplace failed"}]}),
		));
		let ChiefPluginInventory::Available { plugins, errors } = plugins else {
			panic!("catalog");
		};
		assert!(plugins[0].installed);
		assert!(!plugins[0].enabled);
		assert_eq!(plugins[0].availability, "DISABLED_BY_ADMIN");
		assert_eq!(errors.len(), 1);
		assert_eq!(project_mcp(Err(ClientError::Closed)), ChiefMcpInventory::Unavailable);
		assert_eq!(project_plugins(Err(ClientError::Closed)), ChiefPluginInventory::Unavailable);
	}

	#[tokio::test]
	async fn repository_change_during_discovery_invalidates_the_combined_observation() {
		for scenario in ["stable", "directory", "settings", "other_thread"] {
			let (local, remote) = tokio::io::duplex(65536);
			let (reader, writer) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let (finish, finished) = tokio::sync::oneshot::channel::<()>();
			let server = tokio::spawn(async move {
				let (reader, mut writer) = tokio::io::split(remote);
				let mut lines = BufReader::new(reader).lines();
				let mut metadata = 0;
				for _ in 0..5 {
					let request: Value =
						serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
					let result = match request["method"].as_str().unwrap() {
						"thread/read" => {
							metadata += 1;
							if metadata == 2 && matches!(scenario, "settings" | "other_thread") {
								let target =
									if scenario == "settings" { "thread" } else { "another" };
								writer.write_all(format!("{}\n", json!({"method":"thread/settings/updated","params":{"threadId":target,"threadSettings":{"cwd":"/repo"}}})).as_bytes()).await.unwrap();
							}
							assert_eq!(request["params"]["threadId"], "thread");
							json!({"thread":{"id":"thread","cwd":if scenario == "directory" && metadata==2 {"/different"} else {"/repo"}}})
						},
						"mcpServerStatus/list" => {
							assert_eq!(request["params"]["threadId"], "thread");
							json!({"data":[],"nextCursor":null})
						},
						"app/installed" => {
							assert_eq!(
								request["params"],
								json!({"threadId":"thread","forceRefresh":false})
							);
							json!({"apps":[]})
						},
						"plugin/installed" => {
							assert_eq!(request["params"]["cwds"], json!(["/repo"]));
							json!({"marketplaces":[],"marketplaceLoadErrors":[]})
						},
						_ => panic!("unexpected request"),
					};
					writer
						.write_all(
							format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
						)
						.await
						.unwrap();
				}
				let _ = finished.await;
			});
			let result = read(&client, "thread").await;
			if matches!(scenario, "directory" | "settings") {
				assert_eq!(result, ChiefIntegrationsResult::Unavailable);
			} else {
				assert!(
					matches!(result,ChiefIntegrationsResult::Available {cwd,..} if cwd=="/repo")
				);
			}
			finish.send(()).unwrap();
			server.await.unwrap();
		}
	}
}
