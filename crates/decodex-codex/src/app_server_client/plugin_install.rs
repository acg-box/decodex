//! Resolve installation identities from native catalogs. Never retry an installation.
use super::{AppServerClient, ClientError, MAX_FRAME_BYTES};
use serde_json::{Value, json};
use std::{collections::HashSet, time::Duration};

#[derive(Clone, Default)]
pub(super) struct InstallReceipts(
	std::sync::Arc<
		std::sync::Mutex<std::collections::BTreeMap<String, (String, PluginInstallReceipt)>>,
	>,
);
impl InstallReceipts {
	pub(super) fn clear(&self) {
		if let Ok(mut receipts) = self.0.lock() {
			receipts.clear();
		}
	}

	fn remember(&self, key: &str, plugin: &str, receipt: PluginInstallReceipt) {
		if let Ok(mut receipts) = self.0.lock() {
			if receipts.len() >= 128 && !receipts.contains_key(key) {
				receipts.pop_first();
			}
			receipts.insert(key.into(), (plugin.into(), receipt));
		}
	}
}

/// Catalog facts for one exact plugin. Only native catalog parsing constructs this value.
#[derive(Clone)]
pub struct PluginInstallTarget {
	id: String,
	selector: Value,
	installed: bool,
	enabled: bool,
	install_allowed: bool,
	interstitial_required: bool,
	review_details: String,
}

impl PluginInstallTarget {
	/// Exact catalog source, policy and interface facts to show before installation.
	pub fn review_details(&self) -> &str {
		&self.review_details
	}

	/// Exact catalog identity.
	pub fn id(&self) -> &str {
		&self.id
	}

	/// Native observation, distinct from enabled or authenticated.
	pub fn installed(&self) -> bool {
		self.installed
	}

	/// Native configuration observation.
	pub fn enabled(&self) -> bool {
		self.enabled
	}

	/// Whether the catalog permits a new installation.
	pub fn install_allowed(&self) -> bool {
		self.install_allowed
	}

	/// Whether native policy requires installation details to be shown before consent.
	pub fn interstitial_required(&self) -> bool {
		self.interstitial_required
	}
}

impl std::fmt::Debug for PluginInstallTarget {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("PluginInstallTarget([private catalog facts])")
	}
}

/// Native installation receipt. It does not prove connector authorization or tool readiness.
#[derive(Clone)]
pub struct PluginInstallReceipt {
	/// Native ON_INSTALL or ON_USE policy.
	pub auth_policy: String,
	/// Native connector summaries; preserve them for explicit authentication.
	pub apps_needing_auth: Vec<Value>,
}

impl std::fmt::Debug for PluginInstallReceipt {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("PluginInstallReceipt([private authorization details])")
	}
}

impl AppServerClient {
	/// Private installation links retained only on this native connection; never a success proof.
	pub fn cached_plugin_install_receipt(
		&self,
		attempt: &str,
		plugin: &str,
	) -> Option<PluginInstallReceipt> {
		if *self.closed.borrow() || self.outbound.is_closed() {
			return None;
		}
		self.install_receipts
			.0
			.lock()
			.ok()?
			.get(attempt)
			.filter(|(id, _)| id == plugin)
			.map(|(_, receipt)| receipt.clone())
	}

	/// Read current plugin details using the same catalog-resolved selector.
	pub async fn catalog_plugin_details(
		&self,
		target: &PluginInstallTarget,
	) -> Result<Value, ClientError> {
		let result = tokio::time::timeout(
			Duration::from_secs(30),
			self.request("plugin/read", target.selector.clone()),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		let plugin = &result["plugin"];
		if plugin["summary"]["id"] != target.id || !plugin["apps"].is_array() {
			return Err(ClientError::InvalidFrame);
		}
		Ok(plugin.clone())
	}

	/// Refresh the repository's catalog and find exactly one matching plugin.
	/// None means a complete catalog did not contain the identity; failures are not absence.
	pub async fn suggested_plugin_target(
		&self,
		cwd: &str,
		id: &str,
		expected_remote_id: Option<&str>,
	) -> Result<Option<PluginInstallTarget>, ClientError> {
		if !std::path::Path::new(cwd).is_absolute()
			|| !valid_id(id)
			|| expected_remote_id.is_some_and(|id| !valid_id(id))
		{
			return Err(ClientError::InvalidFrame);
		}
		let marketplace_kinds = if expected_remote_id.is_some() {
			json!(["vertical", "workspace-directory", "shared-with-me", "created-by-me-remote"])
		} else {
			json!(["local"])
		};
		let catalog = tokio::time::timeout(
			Duration::from_secs(30),
			self.request(
				"plugin/list",
				json!({"cwds":[cwd],"forceRefetch":true,
				"marketplaceKinds":marketplace_kinds}),
			),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		resolve_target(&catalog, id, expected_remote_id)
	}

	/// Submit exactly one installation with a separate attempt correlation identity.
	/// The caller owns consent, live event/account checks and reconciliation after errors.
	pub async fn install_catalog_plugin(
		&self,
		target: &PluginInstallTarget,
		attempt_id: &str,
	) -> Result<PluginInstallReceipt, ClientError> {
		self.install_plugin_inner(target, attempt_id, None).await
	}

	/// Install only while the originating exact suggestion remains live on this connection.
	pub async fn install_catalog_plugin_guarded(
		&self,
		target: &PluginInstallTarget,
		attempt_id: &str,
		guard: super::ServerRequestGuard,
	) -> Result<PluginInstallReceipt, ClientError> {
		self.install_plugin_inner(target, attempt_id, Some(guard)).await
	}

	async fn install_plugin_inner(
		&self,
		target: &PluginInstallTarget,
		attempt_id: &str,
		guard: Option<super::ServerRequestGuard>,
	) -> Result<PluginInstallReceipt, ClientError> {
		if !valid_id(attempt_id) || target.installed || !target.install_allowed {
			return Err(ClientError::InvalidFrame);
		}
		let mut params = target.selector.clone();
		params["installAttemptId"] = json!(attempt_id);
		let result = tokio::time::timeout(Duration::from_secs(90), async {
			match guard {
				Some(guard) => self.request_guarded("plugin/install", params, guard).await,
				None => self.request("plugin/install", params).await,
			}
		})
		.await
		.map_err(|_| ClientError::Io)??;
		let receipt = parse_receipt(result)?;
		self.install_receipts.remember(attempt_id, target.id(), receipt.clone());
		Ok(receipt)
	}

	/// Refresh all connector observations for one exact native thread.
	/// Missing/failed pages never become a successful partial inventory.
	pub async fn apps_for_thread(&self, thread: &str) -> Result<Vec<Value>, ClientError> {
		if !valid_id(thread) {
			return Err(ClientError::InvalidFrame);
		}
		tokio::time::timeout(Duration::from_secs(30), async {
			let mut rows = Vec::new();
			let mut ids = HashSet::new();
			let mut cursors = HashSet::new();
			let mut cursor: Option<String> = None;
			let mut budget = MAX_FRAME_BYTES;
			for _ in 0..128 {
				let page = self
					.request(
						"app/list",
						json!({"threadId":thread,"forceRefetch":true,"limit":100,"cursor":cursor}),
					)
					.await?;
				budget = budget
					.checked_sub(page.to_string().len())
					.ok_or(ClientError::CapacityExceeded)?;
				let data = page["data"].as_array().ok_or(ClientError::InvalidFrame)?;
				for row in data {
					let id = required_id(&row["id"])?;
					if !ids.insert(id.to_owned())
						|| !row["name"].is_string()
						|| ["isAccessible", "isEnabled"]
							.iter()
							.any(|key| row.get(key).is_some_and(|v| !v.is_boolean()))
					{
						return Err(ClientError::InvalidFrame);
					}
					rows.push(row.clone());
				}
				match page.get("nextCursor") {
					Some(Value::Null) => return Ok(rows),
					Some(Value::String(next)) if valid_id(next) && cursors.insert(next.clone()) =>
						cursor = Some(next.clone()),
					_ => return Err(ClientError::InvalidFrame),
				}
			}
			Err(ClientError::CapacityExceeded)
		})
		.await
		.map_err(|_| ClientError::Io)?
	}
}

fn valid_id(id: &str) -> bool {
	!id.trim().is_empty() && id.len() <= 4096 && !id.chars().any(char::is_control)
}
fn required_id(value: &Value) -> Result<&str, ClientError> {
	value.as_str().filter(|id| valid_id(id)).ok_or(ClientError::InvalidFrame)
}

fn resolve_target(
	catalog: &Value,
	id: &str,
	expected_remote_id: Option<&str>,
) -> Result<Option<PluginInstallTarget>, ClientError> {
	if catalog.to_string().len() > MAX_FRAME_BYTES {
		return Err(ClientError::CapacityExceeded);
	}
	if catalog
		.get("marketplaceLoadErrors")
		.is_some_and(|v| v.as_array().is_none_or(|v| !v.is_empty()))
	{
		return Err(ClientError::InvalidFrame);
	}
	let mut found = None;
	for market in catalog["marketplaces"].as_array().ok_or(ClientError::InvalidFrame)? {
		for plugin in market["plugins"].as_array().ok_or(ClientError::InvalidFrame)? {
			if required_id(&plugin["id"])? != id {
				continue;
			}
			if found.is_some() {
				return Err(ClientError::InvalidFrame);
			}
			let remote = match plugin.get("remotePluginId") {
				None | Some(Value::Null) => None,
				Some(value) => Some(required_id(value)?),
			};
			if expected_remote_id.is_some() && expected_remote_id != remote {
				return Err(ClientError::InvalidFrame);
			}
			let selector = if let Some(path) = market["path"].as_str() {
				if !std::path::Path::new(path).is_absolute() || !valid_id(path) {
					return Err(ClientError::InvalidFrame);
				}
				json!({"marketplacePath":path,"pluginName":required_id(&plugin["name"])?})
			} else if market["path"].is_null() {
				json!({"remoteMarketplaceName":required_id(&market["name"])?,"pluginName":remote.ok_or(ClientError::InvalidFrame)?})
			} else {
				return Err(ClientError::InvalidFrame);
			};
			let installed = plugin["installed"].as_bool().ok_or(ClientError::InvalidFrame)?;
			let enabled = plugin["enabled"].as_bool().ok_or(ClientError::InvalidFrame)?;
			let install_allowed = plugin["availability"] == "AVAILABLE"
				&& matches!(
					plugin["installPolicy"].as_str(),
					Some("AVAILABLE" | "INSTALLED_BY_DEFAULT")
				);
			let interstitial_required = match plugin.get("mustShowInstallationInterstitial") {
				None | Some(Value::Null) => false,
				Some(v) => v.as_bool().ok_or(ClientError::InvalidFrame)?,
			};
			let details = json!({"pluginId":id,"marketplace":market["name"],"source":plugin["source"],"interface":plugin["interface"],"authPolicy":plugin["authPolicy"],"installPolicy":plugin["installPolicy"],"availability":plugin["availability"],"selector":selector});
			let review_details =
				serde_json::to_string_pretty(&details).map_err(|_| ClientError::InvalidFrame)?;
			if review_details.len() > 16384 {
				return Err(ClientError::CapacityExceeded);
			}
			found = Some(PluginInstallTarget {
				id: id.into(),
				selector,
				installed,
				enabled,
				install_allowed,
				interstitial_required,
				review_details,
			});
		}
	}
	Ok(found)
}

fn parse_receipt(result: Value) -> Result<PluginInstallReceipt, ClientError> {
	if result.to_string().len() > 65536 {
		return Err(ClientError::CapacityExceeded);
	}
	let auth_policy = result["authPolicy"]
		.as_str()
		.filter(|v| matches!(*v, "ON_INSTALL" | "ON_USE"))
		.ok_or(ClientError::InvalidFrame)?
		.to_owned();
	let apps = result["appsNeedingAuth"]
		.as_array()
		.filter(|a| a.len() <= 128)
		.ok_or(ClientError::InvalidFrame)?;
	let mut ids = HashSet::new();
	for app in apps {
		if !ids.insert(required_id(&app["id"])?)
			|| !app["name"].is_string()
			|| app.get("installUrl").is_some_and(|u| !u.is_null() && !u.is_string())
		{
			return Err(ClientError::InvalidFrame);
		}
	}
	Ok(PluginInstallReceipt { auth_policy, apps_needing_auth: apps.clone() })
}

#[cfg(test)]
mod tests {
	use super::*;
	fn catalog() -> Value {
		json!({"marketplaces":[{"name":"curated","path":null,"plugins":[{"id":"sample@curated","name":"Display name","remotePluginId":"plugins~exact","installed":false,"enabled":false,"installPolicy":"AVAILABLE","availability":"AVAILABLE","mustShowInstallationInterstitial":true}]}],"marketplaceLoadErrors":[]})
	}
	#[tokio::test]
	async fn authorization_receipts_are_plugin_bound_and_revoked_with_the_connection() {
		let (local, _remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		client.install_receipts.remember(
			"attempt",
			"plugin",
			PluginInstallReceipt {
				auth_policy: "ON_INSTALL".into(),
				apps_needing_auth: vec![
					json!({"id":"extra","installUrl":"https://example.com/private-link"}),
				],
			},
		);
		assert!(client.cached_plugin_install_receipt("attempt", "other-plugin").is_none());
		assert!(client.clone().cached_plugin_install_receipt("attempt", "plugin").is_some());
		client.close();
		assert!(client.cached_plugin_install_receipt("attempt", "plugin").is_none());
	}

	#[test]
	fn target_is_catalog_bound_and_preserves_installation_policy() {
		let target =
			resolve_target(&catalog(), "sample@curated", Some("plugins~exact")).unwrap().unwrap();
		assert_eq!(
			target.selector,
			json!({"remoteMarketplaceName":"curated","pluginName":"plugins~exact"})
		);
		assert!(target.install_allowed());
		assert!(target.interstitial_required());
		assert!(!target.installed());
		assert!(resolve_target(&catalog(), "sample@curated", Some("plugins~different")).is_err());
		let mut local = catalog();
		local["marketplaces"][0]["path"] = json!("/repo/marketplace.json");
		local["marketplaces"][0]["plugins"][0]["name"] = json!("local-name");
		let target = resolve_target(&local, "sample@curated", None).unwrap().unwrap();
		assert_eq!(
			target.selector,
			json!({"marketplacePath":"/repo/marketplace.json","pluginName":"local-name"})
		);
	}
	#[test]
	fn ambiguous_or_partial_catalog_never_authorizes_an_install() {
		let mut ambiguous = catalog();
		let copy = ambiguous["marketplaces"][0].clone();
		ambiguous["marketplaces"].as_array_mut().unwrap().push(copy);
		assert!(resolve_target(&ambiguous, "sample@curated", None).is_err());
		let mut partial = catalog();
		partial["marketplaceLoadErrors"] = json!([{"message":"unavailable"}]);
		assert!(resolve_target(&partial, "missing", None).is_err());
		assert!(resolve_target(&catalog(), "missing", None).unwrap().is_none());
		let mut disabled = catalog();
		disabled["marketplaces"][0]["plugins"][0]["availability"] = json!("DISABLED_BY_ADMIN");
		assert!(
			!resolve_target(&disabled, "sample@curated", None).unwrap().unwrap().install_allowed()
		);
	}
	#[tokio::test]
	async fn local_install_discovery_does_not_require_a_remote_account() {
		use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], "plugin/list");
			assert_eq!(request["params"]["marketplaceKinds"], json!(["local"]));
			let mut result = catalog();
			result["marketplaces"][0]["path"] = json!("/repo/marketplace.json");
			result["marketplaces"][0]["plugins"][0]["remotePluginId"] = Value::Null;
			writer
				.write_all(format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes())
				.await
				.unwrap();
		});
		assert!(
			client
				.suggested_plugin_target("/repo", "sample@curated", None)
				.await
				.unwrap()
				.is_some()
		);
		server.await.unwrap();
	}

	#[tokio::test]
	async fn connector_inventory_preserves_scope_and_rejects_repeated_pages() {
		use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			for cursor in [Value::Null, json!("next")] {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], "app/list");
				assert_eq!(request["params"]["threadId"], "exact-thread");
				assert_eq!(request["params"]["cursor"], cursor);
				let result = json!({"data":[{"id":"same","name":"Calendar","isAccessible":false,"isEnabled":true}],"nextCursor":"next"});
				writer
					.write_all(
						format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
					)
					.await
					.unwrap();
			}
		});
		assert!(matches!(
			client.apps_for_thread("exact-thread").await,
			Err(ClientError::InvalidFrame)
		));
		server.await.unwrap();
	}

	#[tokio::test]
	async fn installation_disconnect_is_not_retried_or_reported_as_success() {
		use tokio::io::{AsyncBufReadExt, BufReader};
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], "plugin/install");
			drop(writer);
		});
		let target = resolve_target(&catalog(), "sample@curated", None).unwrap().unwrap();
		assert!(client.install_catalog_plugin(&target, "attempt-1").await.is_err());
		server.await.unwrap();
		assert!(parse_receipt(json!({"authPolicy":"ON_INSTALL"})).is_err());
	}

	#[tokio::test]
	async fn installation_uses_resolved_identity_once_and_retains_auth_requirements() {
		use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			for method in ["plugin/list", "plugin/install"] {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], method);
				let result = if method == "plugin/list" {
					assert_eq!(
						request["params"],
						json!({"cwds":["/repo"],"forceRefetch":true,
						"marketplaceKinds":["vertical","workspace-directory","shared-with-me","created-by-me-remote"]})
					);
					catalog()
				} else {
					assert_eq!(
						request["params"],
						json!({"remoteMarketplaceName":"curated","pluginName":"plugins~exact","installAttemptId":"attempt-1"})
					);
					json!({"authPolicy":"ON_INSTALL","appsNeedingAuth":[{"id":"connector-1","name":"Calendar","installUrl":"https://chatgpt.com/apps/calendar"}]})
				};
				writer
					.write_all(
						format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes(),
					)
					.await
					.unwrap();
			}
		});
		let target = client
			.suggested_plugin_target("/repo", "sample@curated", Some("plugins~exact"))
			.await
			.unwrap()
			.unwrap();
		let receipt = client.install_catalog_plugin(&target, "attempt-1").await.unwrap();
		assert_eq!(receipt.auth_policy, "ON_INSTALL");
		assert_eq!(receipt.apps_needing_auth[0]["id"], "connector-1");
		server.await.unwrap();
	}
}
