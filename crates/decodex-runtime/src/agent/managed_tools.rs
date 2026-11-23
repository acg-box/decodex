//! The native MCP tools shared by newly created and registered Agents.
//! Native item/started binds the MCP call ID to the actual turn and arguments.
use std::{
	collections::{HashMap, HashSet},
	time::Duration,
};

use rmcp::{
	ErrorData, RoleServer, ServerHandler,
	model::{
		CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
		PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
	},
	service::RequestContext,
	transport::streamable_http_server::{
		StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
	},
};
use serde_json::{Value, json};
use tokio::{
	net::TcpListener,
	sync::{OnceCell, mpsc, oneshot, watch},
	task::JoinHandle,
	time::timeout,
};

use super::{AgentCoordinator, AgentError, ServerEvent};

type CallKey = (String, String);
#[derive(Clone, PartialEq)]
struct NativeCall {
	turn: String,
	tool: String,
	arguments: Value,
}

#[derive(Clone)]
struct Catalog {
	tx: mpsc::Sender<Call>,
	observed: watch::Receiver<HashMap<CallKey, NativeCall>>,
}

pub(crate) struct Call {
	key: CallKey,
	native: NativeCall,
	admit: oneshot::Sender<ToolResponse>,
}

type ToolResponse = oneshot::Receiver<Result<Value, String>>;

impl ServerHandler for Catalog {
	fn get_info(&self) -> ServerConfig {
		ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
	}

	async fn list_tools(
		&self,
		_: Option<PaginatedRequestParams>,
		_: RequestContext<RoleServer>,
	) -> Result<ListToolsResult, ErrorData> {
		let tools: Vec<Tool> = serde_json::from_value(super::tools(true))
			.map_err(|_| ErrorData::internal_error("Invalid Decodex tool catalog", None))?;
		Ok(ListToolsResult::with_all_items(tools))
	}

	async fn call_tool(
		&self,
		req: CallToolRequestParams,
		ctx: RequestContext<RoleServer>,
	) -> Result<CallToolResponse, ErrorData> {
		let (admit, mut admitted) = oneshot::channel();
		let enqueue = async {
			let id = |name| {
				ctx.meta
					.get(name)
					.and_then(Value::as_str)
					.map(str::to_owned)
					.ok_or("Missing native call identity")
			};
			let key = (id("threadId")?, id("callId")?);
			let mut observed = self.observed.clone();
			let native = timeout(
				Duration::from_secs(5),
				observed.wait_for(|calls| calls.contains_key(&key)),
			)
			.await
			.map_err(|_| "No matching native tool call")?
			.map_err(|_| "Connection closed")?
			.get(&key)
			.cloned()
			.ok_or("Native call ended")?;
			if native.tool != req.name
				|| native.arguments != Value::Object(req.arguments.unwrap_or_default())
			{
				return Err("Tool call does not match its native item");
			}
			self.tx.send(Call { key, native, admit }).await.map_err(|_| "Connection closed")?;
			(&mut admitted).await.map_err(|_| "Connection closed")
		};
		let admission = tokio::select! {
			_ = ctx.ct.cancelled() => Err("Tool call cancelled before dispatch"),
			result = timeout(Duration::from_secs(30), enqueue) =>
				result.unwrap_or(Err("Decodex tool queue timed out")),
		};
		let response = admission.or_else(|reason| {
			// Closing excludes a late dispatch. A concurrent successful admission
			// already supplied the response and must return its actual outcome.
			admitted.close();
			admitted.try_recv().map_err(|_| reason)
		});
		let result = match response {
			Ok(response) => response.await.unwrap_or_else(|_| Err("Connection closed".into())),
			Err(reason) => Err(reason.into()),
		};
		Ok(match result {
			Ok(value) => CallToolResult::success(vec![ContentBlock::text(value.to_string())]),
			Err(reason) => CallToolResult::error(vec![ContentBlock::text(reason)]),
		}
		.into())
	}
}

struct Endpoint {
	url: String,
	task: JoinHandle<()>,
}
impl Drop for Endpoint {
	fn drop(&mut self) {
		self.task.abort();
	}
}

pub(super) struct ManagedTools {
	releasing: HashSet<String>,
	endpoint: OnceCell<Endpoint>,
	catalog: Catalog,
	observed: watch::Sender<HashMap<CallKey, NativeCall>>,
	rx: mpsc::Receiver<Call>,
}

impl Default for ManagedTools {
	fn default() -> Self {
		let (observed, rx_observed) = watch::channel(HashMap::new());
		let (tx, rx) = mpsc::channel(32);
		Self {
			releasing: Default::default(),
			endpoint: OnceCell::new(),
			catalog: Catalog { tx, observed: rx_observed },
			observed,
			rx,
		}
	}
}
impl ManagedTools {
	pub(super) async fn configure(
		&self,
		params: &mut Value,
		personal_assistant: bool,
	) -> Result<(), AgentError> {
		let endpoint = self
			.endpoint
			.get_or_try_init(|| async {
				let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
					.await
					.map_err(|e| AgentError::Invalid(e.to_string()))?;
				let addr = listener.local_addr().map_err(|e| AgentError::Invalid(e.to_string()))?;
				let path = format!(
					"/{}",
					decodex_core::ServerIdentity::generate()
						.map_err(|e| AgentError::Invalid(e.to_string()))?
				);
				let catalog = self.catalog.clone();
				let config = StreamableHttpServerConfig::default().enforce_origin_validation();
				let service: StreamableHttpService<Catalog, LocalSessionManager> =
					StreamableHttpService::new(
						move || Ok(catalog.clone()),
						Default::default(),
						config,
					);
				let app = axum::Router::new().nest_service(&path, service);
				let task = tokio::spawn(async move {
					let _ = axum::serve(listener, app).await;
				});
				Ok::<_, AgentError>(Endpoint { url: format!("http://{addr}{path}"), task })
			})
			.await?;
		params["config"]["mcp_servers.decodex"] =
			json!({"url":endpoint.url,"required":true,"default_tools_approval_mode":"approve"});
		if !personal_assistant {
			let private = super::assistant::tools()
				.into_iter()
				.map(|tool| tool["name"].clone())
				.collect::<Vec<_>>();
			params["config"]["mcp_servers.decodex"]["disabled_tools"] = json!(private);
		}

		Ok(())
	}

	pub(super) fn observe(&self, method: &str, params: &Value) {
		let item = &params["item"];
		if item["type"] != "mcpToolCall" || item["server"] != "decodex" {
			return;
		}
		let (Some(thread), Some(turn), Some(id), Some(tool)) = (
			params["threadId"].as_str(),
			params["turnId"].as_str(),
			item["id"].as_str(),
			item["tool"].as_str(),
		) else {
			return;
		};
		let key = (thread.to_owned(), id.to_owned());
		self.observed.send_modify(|calls| {
			if method == "item/started" {
				calls.insert(
					key,
					NativeCall {
						turn: turn.into(),
						tool: tool.into(),
						arguments: item["arguments"].clone(),
					},
				);
			} else {
				calls.remove(&key);
			}
		});
	}
}

pub(crate) enum Input {
	Native(ServerEvent),
	Tool(Call),
}
impl AgentCoordinator {
	pub(super) async fn release_promoted_thread(&mut self, thread: &str) {
		if self.loaded_threads.remove(thread)
			|| self
				.client
				.configured_task_permissions(thread)
				.is_some_and(|(_, guard)| guard.is_live())
		{
			// Native ignores resume overrides for a loaded thread. Relinquish our
			// previous role's subscription. The next send waits for its release.
			self.managed_tools.releasing.insert(thread.into());
			self.release_previous_role(thread).await;
		}
	}

	async fn release_previous_role(&mut self, thread: &str) {
		let released = timeout(Duration::from_secs(2), async {
			self.client.request("thread/unsubscribe", json!({"threadId":thread})).await.ok()?;
			loop {
				let row = self.client.thread_read(json!({"threadId":thread})).await.ok()?;
				if row["thread"]["id"] == thread && row["thread"]["status"]["type"] == "notLoaded" {
					return Some(());
				}
				tokio::time::sleep(Duration::from_millis(30)).await;
			}
		})
		.await;
		if matches!(released, Ok(Some(()))) {
			self.managed_tools.releasing.remove(thread);
		}
	}

	pub(super) async fn ensure_role_released(&mut self, thread: &str) -> Result<(), AgentError> {
		if self.managed_tools.releasing.contains(thread) {
			self.release_previous_role(thread).await;
			if self.managed_tools.releasing.contains(thread) {
				return Err(AgentError::RoleInUse);
			}
		}
		Ok(())
	}

	pub(crate) async fn next_input(&mut self, events: &mut mpsc::Receiver<ServerEvent>) -> Input {
		tokio::select! {
			event = events.recv() => Input::Native(event.unwrap_or(ServerEvent::Closed(super::ClientError::Closed))),
			Some(call) = self.managed_tools.rx.recv() => Input::Tool(call),
		}
	}

	#[cfg(test)]
	pub(crate) async fn next_event(
		&mut self,
		events: &mut mpsc::Receiver<ServerEvent>,
	) -> ServerEvent {
		loop {
			match self.next_input(events).await {
				Input::Native(event) => return event,
				Input::Tool(call) => self.finish_managed_tool(call).await,
			}
		}
	}

	pub(crate) async fn finish_managed_tool(&mut self, call: Call) {
		let mut live = false;
		self.managed_tools.observed.send_modify(|calls| {
			if calls.get(&call.key) == Some(&call.native) {
				calls.remove(&call.key);
				live = true;
			}
		});
		let (reply, response) = oneshot::channel();
		if call.admit.send(response).is_err() {
			return;
		}
		let result = async {
			if !live {
				return Err(AgentError::Rejected("Native call has ended".into()));
			}
			let (thread, _) = &call.key;
			let item =
				self.store.agent_work_item_for_thread(thread.clone()).await?.ok_or_else(|| {
					AgentError::Rejected("This conversation has no Agent tools.".into())
				})?;
			let params = json!({
				"threadId": thread,
				"turnId": call.native.turn,
				"tool": call.native.tool,
				"arguments": call.native.arguments,
			});
			self.authorized_tool(&item, &params, true).await
		}
		.await;
		let _ = reply.send(result.map_err(|e| e.to_string()));
	}
}

#[cfg(test)]
#[path = "managed_tools_tests.rs"]
mod tests;
