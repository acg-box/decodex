//! Read each integration source independently, scoped to the native task directory.
use std::{collections::HashSet, path::Path, time::Duration};

use serde_json::Value;
use tokio::time;

use decodex_codex::app_server_client::{AppServerClient, ClientError};
use decodex_protocol::{
	AgentAppInventory, AgentAppStatusDto, AgentIntegrationsResult, AgentMcpInventory,
	AgentMcpStatusDto, AgentPluginInventory, AgentPluginStatusDto,
};

pub(crate) fn project_plugins(result: Result<Value, ClientError>) -> AgentPluginInventory {
	let value = match result {
		Ok(value) => value,
		Err(ClientError::Remote(error)) if error.code == -32_601 =>
			return AgentPluginInventory::Unsupported,
		Err(ClientError::CapacityExceeded) => return AgentPluginInventory::CapacityExceeded,
		Err(_) => return AgentPluginInventory::Unavailable,
	};
	let Some(markets) = value["marketplaces"].as_array() else {
		return AgentPluginInventory::Unavailable;
	};
	let mut plugins = Vec::new();
	let mut ids = HashSet::new();

	for market in markets {
		let Some(rows) = market["plugins"].as_array() else {
			return AgentPluginInventory::Unavailable;
		};

		for row in rows {
			let (Some(id), Some(name), Some(installed), Some(enabled)) = (
				row["id"].as_str(),
				row["name"].as_str(),
				row["installed"].as_bool(),
				row["enabled"].as_bool(),
			) else {
				return AgentPluginInventory::Unavailable;
			};

			if !ids.insert(id.to_owned()) {
				return AgentPluginInventory::Unavailable;
			}
			if plugins.len() >= 128 || id.len() > 4_096 {
				return AgentPluginInventory::CapacityExceeded;
			}

			plugins.push(AgentPluginStatusDto {
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
		return AgentPluginInventory::CapacityExceeded;
	}

	AgentPluginInventory::Available { plugins, errors }
}

pub(crate) async fn read(client: &AppServerClient, thread: &str) -> AgentIntegrationsResult {
	let result = time::timeout(Duration::from_secs(35), async {
		let guard = client.thread_settings_guard(thread)?;
		let before = client.thread_read(serde_json::json!({"threadId":thread})).await.ok()?;
		let cwd = thread_cwd(&before, thread)?.to_owned();
		let (mcp, plugins, apps) = tokio::join!(
			client.mcp_server_statuses(thread),
			client.installed_plugins_for_directory(&cwd),
			client.installed_apps_for_thread(thread, false)
		);
		let after = client.thread_read(serde_json::json!({"threadId":thread})).await.ok()?;

		if !guard.is_live() || thread_cwd(&after, thread) != Some(cwd.as_str()) {
			return None;
		}

		let result = AgentIntegrationsResult::Available {
			cwd,
			mcp: project_mcp(mcp),
			plugins: project_plugins(plugins),
			apps: project_apps(apps),
		};

		Some(if serde_json::to_vec(&result).ok()?.len() > 64 * 1_024 {
			AgentIntegrationsResult::CapacityExceeded
		} else {
			result
		})
	})
	.await;

	result.ok().flatten().unwrap_or(AgentIntegrationsResult::Unavailable)
}

fn thread_cwd<'a>(value: &'a Value, thread: &str) -> Option<&'a str> {
	if value["thread"]["id"].as_str() != Some(thread) {
		return None;
	}

	value["thread"]["cwd"].as_str().filter(|cwd| Path::new(cwd).is_absolute())
}

fn text(value: &str) -> String {
	if decodex_core::contains_credential_material(value) {
		return "Sensitive details omitted".into();
	}

	value.chars().take(2_048).collect()
}

fn optional(value: &Value) -> Option<String> {
	value.as_str().map(text)
}

fn project_apps(result: Result<Vec<Value>, ClientError>) -> AgentAppInventory {
	let rows = match result {
		Ok(rows) => rows,
		Err(ClientError::Remote(error)) if error.code == -32_601 =>
			return AgentAppInventory::Unsupported,
		Err(ClientError::CapacityExceeded) => return AgentAppInventory::CapacityExceeded,
		Err(_) => return AgentAppInventory::Unavailable,
	};

	if rows.len() > 128 {
		return AgentAppInventory::CapacityExceeded;
	}

	let mut apps = Vec::new();

	for row in rows {
		let (Some(id), Some(enabled), Some(callable)) =
			(row["id"].as_str(), row["enabled"].as_bool(), row["callable"].as_bool())
		else {
			return AgentAppInventory::Unavailable;
		};

		if decodex_core::contains_credential_material(id) {
			return AgentAppInventory::Unavailable;
		}

		apps.push(AgentAppStatusDto {
			id: id.into(),
			runtime_name: optional(&row["runtimeName"]),
			enabled,
			callable,
		});
	}

	AgentAppInventory::Available { apps }
}

fn project_mcp(result: Result<Vec<Value>, ClientError>) -> AgentMcpInventory {
	let rows = match result {
		Ok(rows) => rows,
		Err(ClientError::Remote(error)) if error.code == -32_601 =>
			return AgentMcpInventory::Unsupported,
		Err(ClientError::CapacityExceeded) => return AgentMcpInventory::CapacityExceeded,
		Err(_) => return AgentMcpInventory::Unavailable,
	};

	if rows.len() > 128 {
		return AgentMcpInventory::CapacityExceeded;
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
			return AgentMcpInventory::Unavailable;
		};

		if name.len() > 4_096 {
			return AgentMcpInventory::CapacityExceeded;
		}

		servers.push(AgentMcpStatusDto {
			name: name.into(),
			plugin_id: optional(&row["pluginId"]),
			presentation: server_presentation(&row["serverInfo"]),
			runtime_status: optional(&row["runtimeStatus"]),
			auth_status: text(auth),
			tool_count: tools.len(),
			tools_error: optional(&row["toolsError"]),
			resource_count: resources.len(),
			template_count: templates.len(),
			advertised_capabilities: row["serverCapabilities"].as_object().map(|object| {
				let mut names: Vec<_> = object.keys().map(|key| text(key)).collect();

				if let Some(extensions) = object.get("extensions").and_then(Value::as_object) {
					names.extend(extensions.keys().map(|key| format!("extensions/{}", text(key))));
				}

				names.sort_unstable();

				names
			}),
		});
	}

	AgentMcpInventory::Available { servers }
}

fn server_presentation(info: &Value) -> Option<String> {
	let name = info["title"]
		.as_str()
		.filter(|s| !s.trim().is_empty())
		.or_else(|| info["name"].as_str())?;
	let version = info["version"].as_str()?;
	let mut lines = vec![format!("{} · {}", text(name), text(version))];

	for field in ["description", "websiteUrl"] {
		if let Some(value) = info[field].as_str().filter(|s| !s.trim().is_empty()) {
			lines.push(text(value));
		}
	}

	Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
	use crate::agent_integrations::{
		self, AgentAppInventory, AgentIntegrationsResult, AgentMcpInventory, AgentPluginInventory,
		AppServerClient, ClientError, Value,
	};

	use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

	#[test]
	fn initialized_server_presentation_uses_public_fields_without_fetching_icons() {
		let info = serde_json::json!({"name":"server-id","title":"Reference docs","version":"1.2","description":"Read documentation","websiteUrl":"https://example.invalid","icons":[{"src":"PRIVATE_ICON"}],"private":"PRIVATE_METADATA"});

		assert_eq!(
			agent_integrations::server_presentation(&info).as_deref(),
			Some("Reference docs · 1.2\nRead documentation\nhttps://example.invalid")
		);
		assert_eq!(
			agent_integrations::server_presentation(
				&serde_json::json!({"name":"server-id","version":"1"})
			)
			.as_deref(),
			Some("server-id · 1")
		);
		assert!(agent_integrations::server_presentation(&Value::Null).is_none());
		assert!(
			agent_integrations::server_presentation(&serde_json::json!({"title":"Partial"}))
				.is_none()
		);
	}

	#[test]
	fn failed_discovery_and_plugin_policy_are_not_readiness() {
		let mcp = agent_integrations::project_mcp(Ok(vec![
			serde_json::json!({"name":"server","runtimeStatus":"authenticationRequired","authStatus":"notLoggedIn","tools":{},"toolsError":"Discovery failed","resources":[],"resourceTemplates":[],"serverCapabilities":{"tools":{},"resources":{},"extensions":{"openai/settings":{"readTool":"settings.read","updateTool":"private-fixture-value"}}}}),
		]));
		let AgentMcpInventory::Available { servers } = mcp else {
			panic!("inventory");
		};

		assert_eq!(servers[0].tool_count, 0);
		assert_eq!(servers[0].tools_error.as_deref(), Some("Discovery failed"));
		assert_eq!(servers[0].runtime_status.as_deref(), Some("authenticationRequired"));
		assert_eq!(
			servers[0].advertised_capabilities.as_ref().unwrap(),
			&["extensions", "extensions/openai/settings", "resources", "tools"]
		);
		assert!(!serde_json::to_string(&servers).unwrap().contains("private-fixture-value"));

		let plugins = agent_integrations::project_plugins(Ok(
			serde_json::json!({"marketplaces":[{"plugins":[{"id":"example@market","name":"Example","installed":true,"enabled":false,"availability":"DISABLED_BY_ADMIN","disabledReason":"disabled_by_admin"}]}],"marketplaceLoadErrors":[{"message":"Another marketplace failed"}]}),
		));
		let AgentPluginInventory::Available { plugins, errors } = plugins else {
			panic!("catalog");
		};

		assert!(plugins[0].installed);
		assert!(!plugins[0].enabled);
		assert_eq!(plugins[0].availability, "DISABLED_BY_ADMIN");
		assert_eq!(errors.len(), 1);
		assert_eq!(
			agent_integrations::project_mcp(Err(ClientError::Closed)),
			AgentMcpInventory::Unavailable
		);
		assert_eq!(
			agent_integrations::project_plugins(Err(ClientError::Closed)),
			AgentPluginInventory::Unavailable
		);
	}

	#[test]
	fn apps_project_runtime_eligibility_and_keep_inventory_bounded() {
		let row = serde_json::json!({"id":"connector","runtimeName":"Calendar","enabled":true,"callable":false});
		let AgentAppInventory::Available { apps } =
			agent_integrations::project_apps(Ok(vec![row.clone()]))
		else {
			panic!("snapshot")
		};

		assert!(apps[0].enabled);
		assert!(!apps[0].callable);
		assert_eq!(apps[0].runtime_name.as_deref(), Some("Calendar"));
		assert_eq!(
			agent_integrations::project_apps(Ok(vec![row; 129])),
			AgentAppInventory::CapacityExceeded
		);
		assert_eq!(
			agent_integrations::project_apps(Err(ClientError::Closed)),
			AgentAppInventory::Unavailable
		);
	}

	#[tokio::test]
	async fn repository_change_during_discovery_invalidates_the_combined_observation() {
		for scenario in [
			"stable",
			"directory",
			"settings",
			"other_thread",
			"apps_unsupported",
			"apps_unavailable",
		] {
			let (local, remote) = tokio::io::duplex(65_536);
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

								writer.write_all(format!("{}\n", serde_json::json!({"method":"thread/settings/updated","params":{"threadId":target,"threadSettings":{"cwd":"/repo"}}})).as_bytes()).await.unwrap();
							}

							assert_eq!(request["params"]["threadId"], "thread");

							serde_json::json!({"thread":{"id":"thread","cwd":if scenario == "directory" && metadata==2 {"/different"} else {"/repo"}}})
						},
						"mcpServerStatus/list" => {
							assert_eq!(request["params"]["threadId"], "thread");

							serde_json::json!({"data":[],"nextCursor":null})
						},
						"app/installed" => {
							assert_eq!(
								request["params"],
								serde_json::json!({"threadId":"thread","forceRefresh":false})
							);

							serde_json::json!({"apps":[]})
						},
						"plugin/installed" => {
							assert_eq!(request["params"]["cwds"], serde_json::json!(["/repo"]));

							serde_json::json!({"marketplaces":[],"marketplaceLoadErrors":[]})
						},
						_ => panic!("unexpected request"),
					};
					let reply = if request["method"] == "app/installed"
						&& scenario.starts_with("apps_")
					{
						serde_json::json!({"id":request["id"],"error":{"code":if scenario == "apps_unsupported" {-32_601} else {-32_603},"message":"Fixture Apps failure"}})
					} else {
						serde_json::json!({"id":request["id"],"result":result})
					};

					writer.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
				}

				let _ = finished.await;
			});
			let result = agent_integrations::read(&client, "thread").await;

			if matches!(scenario, "directory" | "settings") {
				assert_eq!(result, AgentIntegrationsResult::Unavailable);
			} else {
				let AgentIntegrationsResult::Available { cwd, mcp, plugins, apps } = result else {
					panic!("independent observations")
				};

				assert_eq!(cwd, "/repo");
				assert!(matches!(mcp, AgentMcpInventory::Available { .. }));
				assert!(matches!(plugins, AgentPluginInventory::Available { .. }));
				assert_eq!(
					apps,
					match scenario {
						"apps_unsupported" => AgentAppInventory::Unsupported,
						"apps_unavailable" => AgentAppInventory::Unavailable,
						_ => AgentAppInventory::Available { apps: vec![] },
					}
				);
			}

			finish.send(()).unwrap();
			server.await.unwrap();
		}
	}
}
