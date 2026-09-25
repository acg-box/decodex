//! Isolated native structured requests for task recaps. The runtime owns event routing.
use super::{AppServerClient, ClientError, ServerEvent};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path, time::Duration};
use tokio::sync::{mpsc, watch};

const DEADLINE: Duration = Duration::from_secs(30);
const MAX_RESPONSE: usize = 8 * 1024;

/// Existing task settings used to start a tool-isolated ephemeral thread.
pub struct TemporaryStructuredOptions {
	/// The selected task's model.
	pub model: String,
	/// The selected task's native provider identifier.
	pub model_provider: String,
	/// Absolute native working directory used for effective configuration.
	pub cwd: String,
	/// Preserve a custom permission profile instead of replacing its restrictions.
	pub active_permission_profile: Option<String>,
	/// Already observed MCP names, combined with the effective native configuration.
	pub mcp_server_names: Vec<String>,
}

/// An isolated temporary thread. Run or cancel it and await cleanup; do not abort its owner.
#[must_use = "Run or cancel the temporary thread and await native cleanup"]
pub struct TemporaryStructuredThread {
	client: AppServerClient,
	id: String,
}

impl AppServerClient {
	/// Start an ephemeral thread with tools disabled, then verify native permissions.
	/// Cancellation during startup must wait for this result so the thread can be detached.
	pub async fn start_temporary_structured(
		&self,
		options: TemporaryStructuredOptions,
	) -> Result<TemporaryStructuredThread, ClientError> {
		if !Path::new(&options.cwd).is_absolute()
			|| options.model.is_empty()
			|| options.model_provider.is_empty()
		{
			return Err(ClientError::InvalidFrame);
		}
		let config = tokio::time::timeout(
			DEADLINE,
			self.request("config/read", json!({"cwd":options.cwd,"includeLayers":false})),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		let config = isolation_config(&config["config"], &options.mcp_server_names)?;
		let profile = options.active_permission_profile.filter(|id| !id.starts_with(':'));
		let mut params = json!({"model":options.model,"modelProvider":options.model_provider,
			"cwd":options.cwd,"approvalPolicy":"never","runtimeWorkspaceRoots":[],
			"ephemeral":true,"threadSource":"system","environments":[],
			"dynamicTools":[],"selectedCapabilityRoots":[],"config":config});
		if let Some(profile) = &profile {
			params["permissions"] = json!(profile);
		} else {
			params["sandbox"] = json!("read-only");
		}
		let response = tokio::time::timeout(DEADLINE, self.thread_start(params))
			.await
			.map_err(|_| ClientError::Io)??;
		let id = response["thread"]["id"]
			.as_str()
			.filter(|id| !id.is_empty())
			.ok_or(ClientError::InvalidFrame)?
			.to_owned();
		let thread = TemporaryStructuredThread { client: self.clone(), id };
		let permission_matches = match profile {
			Some(profile) => response["activePermissionProfile"]["id"] == profile,
			None => response["sandbox"]["type"] == "readOnly",
		};
		if !permission_matches || response["thread"]["ephemeral"] != true {
			let _ = thread.detach().await;
			return Err(ClientError::InvalidFrame);
		}
		Ok(thread)
	}
}

fn isolation_config(effective: &Value, known: &[String]) -> Result<Value, ClientError> {
	if !effective.is_object() {
		return Err(ClientError::InvalidFrame);
	}
	let mut names: BTreeSet<_> = known.iter().cloned().collect();
	match &effective["mcp_servers"] {
		Value::Null => {},
		Value::Object(servers) => names.extend(servers.keys().cloned()),
		_ => return Err(ClientError::InvalidFrame),
	}
	let mut config = json!({"web_search":"disabled","mcp_servers": names.into_iter()
		.map(|name|(name,json!({"enabled":false}))).collect::<serde_json::Map<_,_>>()});
	for key in [
		"features.apps",
		"features.code_mode",
		"features.code_mode_only",
		"features.context_management",
		"features.current_time_reminder",
		"features.deferred_executor",
		"features.enable_fanout",
		"features.goals",
		"features.hooks",
		"features.image_generation",
		"features.memories",
		"features.multi_agent",
		"features.multi_agent_v2",
		"features.plugins",
		"features.request_permissions_tool",
		"features.shell_snapshot",
		"features.shell_tool",
		"features.standalone_web_search",
		"features.token_budget",
		"features.tool_suggest",
		"features.unified_exec",
		"features.view_image",
		"orchestrator.skills.enabled",
		"skills.include_instructions",
		"tools.experimental_request_user_input.enabled",
		"tools.update_plan.enabled",
	] {
		config[key] = json!(false);
	}
	Ok(config)
}

impl TemporaryStructuredThread {
	/// Exact identity for the runtime's temporary-thread event route.
	pub fn id(&self) -> &str {
		&self.id
	}

	async fn detach(&self) -> Result<(), ClientError> {
		let response = tokio::time::timeout(
			DEADLINE,
			self.client.request("thread/unsubscribe", json!({"threadId":self.id})),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		match response["status"].as_str() {
			Some("unsubscribed" | "notSubscribed" | "notLoaded") => Ok(()),
			_ => Err(ClientError::InvalidFrame),
		}
	}

	/// Cancel a late thread start before submitting inference.
	pub async fn cancel(self) -> Result<(), ClientError> {
		self.detach().await
	}

	/// Collect only the selected temporary turn; interrupt failures and detach on exit.
	/// The runtime must register its event route before calling this method and keep it
	/// alive until cleanup returns. Dropping the cancellation sender also cancels work.
	pub async fn run(
		self,
		prompt: String,
		output_schema: Value,
		effort: Option<String>,
		mut events: mpsc::Receiver<ServerEvent>,
		mut cancellation: watch::Receiver<bool>,
	) -> Result<String, ClientError> {
		let mut turn_id = None;
		let result = tokio::time::timeout(DEADLINE, async {
			if *cancellation.borrow() || cancellation.has_changed().is_err() {
				return Err(ClientError::Closed);
			}
			let mut params = json!({"threadId":self.id,"input":[{"type":"text","text":prompt}],
				"outputSchema":output_schema});
			if let Some(effort) = effort {
				params["effort"] = json!(effort);
			}
			// Do not drop turn/start on cancellation: the returned ID owns interruption.
			let started = self.client.turn_start(params).await?;
			let id = started["turn"]["id"]
				.as_str()
				.filter(|id| !id.is_empty())
				.ok_or(ClientError::InvalidFrame)?
				.to_owned();
			turn_id = Some(id.clone());
			if *cancellation.borrow() || cancellation.has_changed().is_err() {
				return Err(ClientError::Closed);
			}
			tokio::select! {
				biased;
				_ = cancellation.changed() => Err(ClientError::Closed),
				result = collect(&mut events, &self.id, &id) => result,
			}
		})
		.await
		.unwrap_or(Err(ClientError::Io));
		if result.is_err()
			&& let Some(turn) = turn_id
		{
			let _ = tokio::time::timeout(
				DEADLINE,
				self.client.turn_interrupt(json!({"threadId":self.id,"turnId":turn})),
			)
			.await;
		}
		let detached = self.detach().await;
		match result {
			Ok(value) => detached.map(|()| value),
			Err(error) => Err(error),
		}
	}
}

async fn collect(
	events: &mut mpsc::Receiver<ServerEvent>,
	thread: &str,
	turn: &str,
) -> Result<String, ClientError> {
	let mut output = None;
	while let Some(event) = events.recv().await {
		match event {
			ServerEvent::Closed(error) => return Err(error),
			ServerEvent::Request { params, .. } if params["threadId"] == thread =>
				return Err(ClientError::InvalidFrame),
			ServerEvent::Notification { method, params } if params["threadId"] == thread =>
				if method == "item/completed"
					&& params["turnId"] == turn
					&& params["item"]["type"] == "agentMessage"
				{
					let text = params["item"]["text"].as_str().ok_or(ClientError::InvalidFrame)?;
					if text.len() > MAX_RESPONSE {
						return Err(ClientError::FrameTooLarge);
					}
					output = Some(text.to_owned());
				} else if method == "turn/completed" && params["turn"]["id"] == turn {
					return if params["turn"]["status"] == "completed" {
						output.ok_or(ClientError::InvalidFrame)
					} else {
						Err(ClientError::InvalidFrame)
					};
				},
			_ => {},
		}
	}
	Err(ClientError::Closed)
}

#[cfg(test)]
#[path = "temporary_structured_tests.rs"]
mod tests;
