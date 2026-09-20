//! Fresh installation evidence. Reading this module never initiates installation.
use decodex_codex::app_server_client::{AppServerClient, PluginInstallTarget};
use decodex_database::SqliteStore;
use decodex_protocol::{
	ChiefInstallApp, ChiefInstallState, McpInstallSuggestion, McpInstallTarget,
};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

pub(crate) struct Inspection {
	pub state: ChiefInstallState,
	pub target: Option<PluginInstallTarget>,
	pub thread: String,
}

pub(crate) async fn inspect(
	store: &SqliteStore,
	client: &AppServerClient,
	work: &str,
	event_id: i64,
) -> Option<Inspection> {
	let event = store.get_chief_inbox_event(event_id).await.ok()?;
	let owner = store.get_chief_work_item(work.into()).await.ok()?;
	if event.work_item_id != work
		|| event.disposition.is_some()
		|| event.event_kind != "server_request_pending"
	{
		return None;
	}
	let thread = owner.codex_thread_id?;
	let value: Value = serde_json::from_str(&event.payload).ok()?;
	let params = &value["params"];
	if value["method"] != "mcpServer/elicitation/request"
		|| params["threadId"] != thread
		|| (!params["turnId"].is_null()
			&& (params["turnId"].as_str() != owner.active_turn_id.as_deref()
				|| owner.dispatch_state != decodex_database::ChiefDispatchState::Running))
	{
		return None;
	}
	let suggestion = McpInstallSuggestion::from_request(params).ok()??;
	let request_id = serde_json::from_value(value["id"].clone()).ok()?;
	let guard =
		client.server_request_guard(&request_id, "mcpServer/elicitation/request", params)?;
	let native = client.thread_read(json!({"threadId":thread})).await.ok()?;
	if native["thread"]["id"] != thread {
		return None;
	}
	let cwd = native["thread"]["cwd"].as_str()?;
	let target = match &suggestion.target {
		McpInstallTarget::Plugin { remote_plugin_id, .. } => Some(
			client
				.suggested_plugin_target(cwd, &suggestion.tool_id, remote_plugin_id.as_deref())
				.await
				.ok()??,
		),
		McpInstallTarget::Connector => None,
	};
	let mut expected = match &suggestion.target {
		McpInstallTarget::Plugin { app_connector_ids, .. } => app_connector_ids.clone(),
		McpInstallTarget::Connector => vec![suggestion.tool_id.clone()],
	};
	let mut plugin_details = None;
	if let Some(target) = &target {
		let detail = client.catalog_plugin_details(target).await.ok()?;
		for app in detail["apps"].as_array()? {
			let id = app["id"].as_str().filter(|s| !s.is_empty() && s.len() <= 1024)?;
			if !expected.iter().any(|v| v == id) {
				expected.push(id.into());
			}
		}
		if expected.len() > 128 {
			return None;
		}
		plugin_details = Some(detail);
	}
	let attempt_id = store.chief_install_attempt_id(event_id).await.ok()?;
	let requirements = store.chief_install_requirements(event_id).await.ok()?;
	let receipt = attempt_id
		.as_deref()
		.and_then(|id| client.cached_plugin_install_receipt(id, &suggestion.tool_id));
	if let Some(requirements) = &requirements {
		for id in &requirements.connector_ids {
			if !expected.contains(id) {
				expected.push(id.clone());
			}
		}
	}
	if expected.len() > 128 {
		return None;
	}
	let apps = inspect_required_apps(
		client,
		&thread,
		expected,
		receipt.as_ref(),
		plugin_details.as_ref(),
		&suggestion,
	)
	.await?;
	let state = installation_state(
		(work, event_id, &thread),
		suggestion,
		target.as_ref(),
		plugin_details.as_ref(),
		apps,
		attempt_id.is_some(),
		requirements.is_some(),
	)?;
	// Recheck the durable request after remote reads; a peer may have resolved it.
	let current = store.get_chief_inbox_event(event_id).await.ok()?;
	let current_owner = store.get_chief_work_item(work.into()).await.ok()?;
	if !guard.is_live()
		|| current.disposition.is_some()
		|| current.payload != event.payload
		|| current_owner.codex_thread_id.as_deref() != Some(thread.as_str())
		|| current_owner.active_turn_id != owner.active_turn_id
	{
		return None;
	}
	Some(Inspection { thread, target, state })
}

fn installation_state(
	identity: (&str, i64, &str),
	suggestion: McpInstallSuggestion,
	target: Option<&PluginInstallTarget>,
	plugin_details: Option<&Value>,
	apps: Vec<ChiefInstallApp>,
	attempted: bool,
	has_requirements: bool,
) -> Option<ChiefInstallState> {
	let (work, event_id, thread) = identity;
	let authorization_requirements_known = !attempted
		|| has_requirements
		|| !matches!(
			&suggestion.target,
			McpInstallTarget::Plugin { remote_plugin_id: Some(_), .. }
		);
	let installed = target.map(PluginInstallTarget::installed);
	let mut details = target
		.as_ref()
		.map(|t| t.review_details().to_owned())
		.unwrap_or_else(|| format!("Connect {} ({})", suggestion.tool_name, suggestion.tool_id));
	if let Some(detail) = plugin_details {
		let capabilities = json!({"description":detail["description"],"apps":expected_ids(&apps),"mcpServers":detail["mcpServers"],"skills":detail["skills"],"hooks":detail["hooks"],"scheduledTasks":detail["scheduledTasks"]});
		details.push_str(&format!("\n{}", serde_json::to_string_pretty(&capabilities).ok()?));
	}
	if details.len() > 16384 {
		return None;
	}
	if decodex_core::contains_credential_material(&details) {
		return None;
	}
	let review_token = Sha256::digest(
		json!([work, event_id, thread, suggestion.tool_id, details]).to_string().as_bytes(),
	)
	.iter()
	.map(|b| format!("{b:02x}"))
	.collect();
	let can_install = target.is_some_and(|t| !t.installed() && t.install_allowed()) && !attempted;
	let can_continue = authorization_requirements_known
		&& target.is_none_or(|t| t.installed() && t.enabled())
		&& apps.iter().all(|a| a.accessible && a.enabled);
	let review_details = match (target, plugin_details) {
		(Some(target), Some(detail)) => installation_summary(target, detail)?,
		_ => details,
	};
	Some(ChiefInstallState::Available {
		event_id,
		tool_id: suggestion.tool_id,
		tool_name: suggestion.tool_name,
		installed,
		attempted,
		authorization_requirements_known,
		can_install,
		can_continue,
		review_token,
		review_details,
		apps,
	})
}

async fn inspect_required_apps(
	client: &AppServerClient,
	thread: &str,
	expected: Vec<String>,
	receipt: Option<&decodex_codex::app_server_client::PluginInstallReceipt>,
	plugin_details: Option<&Value>,
	suggestion: &McpInstallSuggestion,
) -> Option<Vec<ChiefInstallApp>> {
	let catalog =
		if expected.is_empty() { Vec::new() } else { client.apps_for_thread(thread).await.ok()? };
	let mut apps = Vec::new();
	for id in expected {
		let row = catalog.iter().find(|row| row["id"] == id);
		let accessible = row.is_some_and(|row| row["isAccessible"] == true);
		let enabled = row.is_some_and(|row| row.get("isEnabled").is_none_or(|v| v == true));
		let receipt_app = receipt
			.and_then(|receipt| receipt.apps_needing_auth.iter().find(|app| app["id"] == id));
		let name = row
			.and_then(|row| row["name"].as_str())
			.or_else(|| receipt_app.and_then(|app| app["name"].as_str()))
			.unwrap_or(&id);
		if name.len() > 2048 || decodex_core::contains_credential_material(name) {
			return None;
		}
		let url = row
			.and_then(|row| row["installUrl"].as_str())
			.or_else(|| receipt_app.and_then(|app| app["installUrl"].as_str()))
			.or_else(|| {
				plugin_details?
						.get("apps")?
						.as_array()?
						.iter()
						.find(|a| a["id"] == id)?["installUrl"]
						.as_str()
			})
			.or_else(|| {
				matches!(&suggestion.target, McpInstallTarget::Connector)
					.then(|| suggestion.install_url())
					.flatten()
			})
			.and_then(authorization_url);
		apps.push(ChiefInstallApp {
			id: id.clone(),
			name: name.into(),
			accessible,
			enabled,
			install_url: url,
		});
	}
	Some(apps)
}

fn installation_summary(target: &PluginInstallTarget, detail: &Value) -> Option<String> {
	let catalog: Value = serde_json::from_str(target.review_details()).ok()?;
	let mut lines = vec![format!("Plugin: {}", target.id())];
	if let Some(description) = detail["description"].as_str() {
		lines.push(description.into());
	}
	if let Some(marketplace) = catalog["marketplace"].as_str() {
		lines.push(format!("Marketplace: {marketplace}"));
	}
	let source = &catalog["source"];
	for (field, label) in [
		("type", "Source"),
		("path", "Path"),
		("url", "Repository"),
		("refName", "Revision"),
		("sha", "Commit"),
		("package", "Package"),
		("version", "Version"),
		("registry", "Registry"),
	] {
		if let Some(value) = source[field].as_str() {
			lines.push(format!("{label}: {value}"));
		}
	}
	lines.push(
		if target.installed() && !target.enabled() {
			"Installed but disabled in Codex configuration. Enable this plugin there, then check status again."
		} else if target.installed() {
			"Installed and enabled."
		} else if !target.install_allowed() {
			"Installation is unavailable under the current catalog policy."
		} else {
			"Ready to install."
		}
		.into(),
	);
	for (field, label) in
		[("installPolicy", "Installation policy"), ("availability", "Availability")]
	{
		if let Some(value) = catalog[field].as_str() {
			lines.push(format!("{label}: {value}"));
		}
	}
	lines.push(
		match catalog["authPolicy"].as_str() {
			Some("ON_INSTALL") => "Authentication policy: connect required services during setup.",
			Some("ON_USE") =>
				"Authentication policy: services can request authentication when used.",
			_ => "Authentication policy: not reported by the catalog.",
		}
		.into(),
	);
	for (field, label) in [
		("skills", "Skills"),
		("mcpServers", "MCP servers"),
		("hooks", "Hooks"),
		("scheduledTasks", "Scheduled tasks"),
	] {
		if let Some(items) = detail[field].as_array() {
			let names: Vec<_> = items
				.iter()
				.filter_map(|item| {
					item.as_str()
						.or_else(|| item["name"].as_str())
						.or_else(|| item["eventName"].as_str())
				})
				.collect();
			lines.push(if names.is_empty() {
				format!("{label}: {}", items.len())
			} else {
				format!("{label}: {} ({})", items.len(), names.join(", "))
			});
		}
	}
	let text = lines.join("\n");
	(text.len() <= 16384 && !decodex_core::contains_credential_material(&text)).then_some(text)
}

fn expected_ids(apps: &[ChiefInstallApp]) -> Vec<&str> {
	apps.iter().map(|app| app.id.as_str()).collect()
}

fn authorization_url(raw: &str) -> Option<decodex_protocol::McpAuthorizationUrl> {
	let url = reqwest::Url::parse(raw).ok()?;
	if raw.chars().any(char::is_control)
		|| url.scheme() != "https"
		|| url.host_str().is_none()
		|| !url.username().is_empty()
		|| url.password().is_some()
	{
		return None;
	}
	decodex_protocol::McpAuthorizationUrl::new(raw.into()).ok()
}
