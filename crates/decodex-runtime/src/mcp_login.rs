//! One in-memory native OAuth intent. No authorization material enters SQLite.
use decodex_codex::app_server_client::{AppServerClient, ClientError, ServerEvent};
use decodex_core::ProcessGenerationId;
use decodex_protocol::{
	McpAuthorizationUrl, McpLoginPhase, McpLoginRequest, McpLoginStatus, WireText,
};
use serde_json::json;
use std::{
	sync::Arc,
	time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub(crate) struct Source {
	pub generation: ProcessGenerationId,
	pub thread: String,
	pub client: AppServerClient,
}
struct Session {
	status: McpLoginStatus,
	aliases: Vec<decodex_protocol::EntityId>,
	work: String,
	thread: String,
	server: String,
	generation: ProcessGenerationId,
	pending: bool,
	started: Instant,
}
#[derive(Clone, Default)]
pub(crate) struct McpLoginGateway(Arc<Mutex<Option<Session>>>);

pub(crate) fn status(
	request: &McpLoginRequest,
	phase: McpLoginPhase,
	message: &str,
) -> McpLoginStatus {
	McpLoginStatus {
		session_id: request.session_id().clone(),
		phase,
		authorization_url: None,
		message: WireText::new(message).expect("bounded local message"),
	}
}
fn project(session: &Session, request: &McpLoginRequest) -> McpLoginStatus {
	McpLoginStatus { session_id: request.session_id().clone(), ..session.status.clone() }
}

impl McpLoginGateway {
	pub(crate) async fn exchange(
		&self,
		request: &McpLoginRequest,
		source: Option<Source>,
	) -> McpLoginStatus {
		let Some(source) = source else {
			return status(
				request,
				McpLoginPhase::Disconnected,
				"The native task connection is unavailable.",
			);
		};
		{
			let mut slot = self.0.lock().await;
			if let Some(session) = slot.as_mut() {
				if session.generation != source.generation {
					session.pending = false;
					session.status.authorization_url = None;
					session.status.phase = McpLoginPhase::Disconnected;
					session.status.message =
						WireText::new("The native connection changed. No request was replayed.")
							.expect("static OAuth status fits wire bounds");
				}
				if session.pending && session.started.elapsed() > Duration::from_secs(180) {
					session.status.phase = McpLoginPhase::Expired;
					session.status.authorization_url = None;
					session.status.message = WireText::new(
						"Waiting expired. Native sign-in may still finish; refresh server status.",
					)
					.expect("static OAuth status fits wire bounds");
				}
				if session.status.session_id == *request.session_id()
					|| session.aliases.contains(request.session_id())
				{
					if session.work != request.work_id().as_str() || session.thread != source.thread
					{
						return status(
							request,
							McpLoginPhase::Disconnected,
							"This sign-in belongs to a different task.",
						);
					}
					if let McpLoginRequest::Start { server_name, .. } = request
						&& session.server != server_name.as_str()
					{
						return status(
							request,
							McpLoginPhase::Failed,
							"This sign-in identity belongs to a different server.",
						);
					}

					return project(session, request);
				}
				if session.pending {
					if let McpLoginRequest::Start { server_name, .. } = request
						&& session.work == request.work_id().as_str()
						&& session.thread == source.thread
						&& session.server == server_name.as_str()
						&& session.aliases.len() < 16
					{
						session.aliases.push(request.session_id().clone());
						return project(session, request);
					}
					return status(
						request,
						McpLoginPhase::Failed,
						"Another native sign-in is still pending. Finish it before starting another.",
					);
				}
			}
			let McpLoginRequest::Start { server_name, .. } = request else {
				return status(
					request,
					McpLoginPhase::Disconnected,
					"This sign-in is no longer available. No request was replayed.",
				);
			};
			if server_name.as_str().trim().is_empty() {
				return status(request, McpLoginPhase::Failed, "Select an MCP server.");
			}
			*slot = Some(Session {
				status: status(request, McpLoginPhase::Starting, "Starting native sign-in…"),
				aliases: Vec::new(),
				work: request.work_id().as_str().into(),
				thread: source.thread.clone(),
				server: server_name.as_str().into(),
				generation: source.generation.clone(),
				pending: true,
				started: Instant::now(),
			});
		}
		let McpLoginRequest::Start { server_name, .. } = request else { unreachable!() };
		let reply = tokio::time::timeout(
			Duration::from_secs(30),
			source.client.request(
				"mcpServer/oauth/login",
				json!({"name":server_name.as_str(),"threadId":source.thread,"timeoutSecs":120}),
			),
		)
		.await;
		self.finish_login_reply(request, &source, reply).await
	}

	async fn finish_login_reply(
		&self,
		request: &McpLoginRequest,
		source: &Source,
		reply: Result<Result<serde_json::Value, ClientError>, tokio::time::error::Elapsed>,
	) -> McpLoginStatus {
		let mut slot = self.0.lock().await;
		let Some(session) = slot.as_mut().filter(|session| {
			session.status.session_id == *request.session_id()
				&& session.generation == source.generation
		}) else {
			return status(request, McpLoginPhase::Disconnected, "The sign-in connection changed.");
		};
		// Completion can arrive before the initiating RPC reply; never overwrite it.
		if session.status.phase != McpLoginPhase::Starting {
			return project(session, request);
		}
		match reply {
			Ok(Ok(value)) => {
				let url = value["authorizationUrl"]
					.as_str()
					.and_then(|value| reqwest::Url::parse(value).ok())
					.filter(|url| {
						matches!(url.scheme(), "https" | "http")
							&& url.host_str().is_some()
							&& url.username().is_empty()
							&& url.password().is_none()
					});
				if let Some(url) =
					url.and_then(|url| McpAuthorizationUrl::new(url.to_string()).ok())
				{
					session.status.phase = McpLoginPhase::AwaitingUser;
					session.status.authorization_url = Some(url);
					session.status.message = WireText::new(
						"Open the authorization page to continue. Opening it does not confirm sign-in.",
					)
					.expect("static OAuth status fits wire bounds");
				} else {
					session.status = status(
						request,
						McpLoginPhase::Unknown,
						"Native sign-in returned no supported authorization link. It was not opened.",
					);
				}
			},
			Ok(Err(ClientError::Remote(_))) => {
				session.pending = false;
				session.status = status(
					request,
					McpLoginPhase::Failed,
					"Native Codex could not start sign-in for this server. Check its current status.",
				);
			},
			_ => {
				session.status = status(
					request,
					McpLoginPhase::Unknown,
					"Sign-in could not be confirmed. The request will not be replayed.",
				);
			},
		}
		project(session, request)
	}

	pub(crate) async fn observe(&self, generation: &ProcessGenerationId, event: &ServerEvent) {
		let ServerEvent::Notification { method, params } = event else {
			return;
		};
		if method != "mcpServer/oauthLogin/completed" {
			return;
		}
		let mut slot = self.0.lock().await;
		let Some(session) = slot.as_mut().filter(|session| {
			session.pending
				&& &session.generation == generation
				&& params["threadId"].as_str() == Some(session.thread.as_str())
				&& params["name"].as_str() == Some(session.server.as_str())
		}) else {
			return;
		};
		let Some(success) = params["success"].as_bool() else {
			return;
		};
		session.pending = false;
		session.status.authorization_url = None;
		session.status.phase =
			if success { McpLoginPhase::NativeCompleted } else { McpLoginPhase::Failed };
		session.status.message = WireText::new(if success {
			"Codex reported sign-in complete. Refresh tool status to verify the connection."
		} else {
			"Codex reported sign-in failure. Refresh server status before trying again."
		})
		.expect("static OAuth status fits wire bounds");
	}

	pub(crate) async fn expire(&self) {
		if let Some(session) = self.0.lock().await.as_mut()
			&& session.pending
			&& session.started.elapsed() > Duration::from_secs(180)
		{
			session.status.phase = McpLoginPhase::Expired;
			session.status.authorization_url = None;
			session.status.message = WireText::new(
				"Waiting expired. Native sign-in may still finish; refresh server status.",
			)
			.expect("static OAuth status fits wire bounds");
		}
	}

	pub(crate) async fn disconnect(&self, generation: Option<&ProcessGenerationId>) {
		if let Some(session) =
			self.0.lock().await.as_mut().filter(|session| {
				generation.is_none_or(|generation| generation == &session.generation)
			}) {
			session.pending = false;
			session.status.authorization_url = None;
			session.status.phase = McpLoginPhase::Disconnected;
			session.status.message =
				WireText::new("The native connection changed. No sign-in request was replayed.")
					.expect("static OAuth status fits wire bounds");
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_protocol::EntityId;
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
	fn generation() -> ProcessGenerationId {
		ProcessGenerationId::new("10000000-0000-4000-8000-000000000001").unwrap()
	}
	fn request() -> McpLoginRequest {
		McpLoginRequest::Start {
			session_id: EntityId::new("intent").unwrap(),
			work_id: EntityId::new("work").unwrap(),
			server_name: WireText::new("server").unwrap(),
		}
	}
	fn source(client: &AppServerClient) -> Source {
		Source { generation: generation(), thread: "native-thread".into(), client: client.clone() }
	}
	#[tokio::test]
	async fn sign_in_is_once_only_and_completion_requires_exact_native_scope() {
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			let request: serde_json::Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], "mcpServer/oauth/login");
			assert_eq!(
				request["params"],
				json!({"name":"server","threadId":"native-thread","timeoutSecs":120})
			);
			writer.write_all(format!("{}\n",json!({"id":request["id"],"result":{"authorizationUrl":"https://example.test/authorize?state=private-test"}})).as_bytes()).await.unwrap();
			assert!(
				tokio::time::timeout(Duration::from_millis(100), lines.next_line()).await.is_err(),
				"no duplicate native sign-in request"
			);
		});
		let gateway = McpLoginGateway::default();
		let request = request();
		let result = gateway.exchange(&request, Some(source(&client))).await;
		assert_eq!(result.phase, McpLoginPhase::AwaitingUser);
		assert!(!format!("{result:?}").contains("private-test"));
		assert_eq!(gateway.exchange(&request, Some(source(&client))).await, result);
		let recovered = McpLoginRequest::Start {
			session_id: EntityId::new("reopened-window").unwrap(),
			work_id: request.work_id().clone(),
			server_name: WireText::new("server").unwrap(),
		};
		let recovered_status = gateway.exchange(&recovered, Some(source(&client))).await;
		assert_eq!(recovered_status.session_id, *recovered.session_id());
		assert_eq!(recovered_status.phase, McpLoginPhase::AwaitingUser);
		assert!(recovered_status.authorization_url.is_some());

		for (thread, name) in [("other", "server"), ("native-thread", "other")] {
			gateway
				.observe(
					&generation(),
					&ServerEvent::Notification {
						method: "mcpServer/oauthLogin/completed".into(),
						params: json!({"threadId":thread,"name":name,"success":true}),
					},
				)
				.await;
		}
		let wrong = ProcessGenerationId::new("20000000-0000-4000-8000-000000000002").unwrap();
		let completed = ServerEvent::Notification {
			method: "mcpServer/oauthLogin/completed".into(),
			params: json!({"threadId":"native-thread","name":"server","success":true}),
		};
		gateway.observe(&wrong, &completed).await;
		gateway.disconnect(Some(&wrong)).await;
		assert_eq!(
			gateway.exchange(&request, Some(source(&client))).await.phase,
			McpLoginPhase::AwaitingUser
		);
		gateway.observe(&generation(), &completed).await;
		let complete = gateway.exchange(&request, Some(source(&client))).await;
		assert_eq!(complete.phase, McpLoginPhase::NativeCompleted);
		assert!(complete.authorization_url.is_none());
		let alias_poll = McpLoginRequest::Poll {
			session_id: recovered.session_id().clone(),
			work_id: recovered.work_id().clone(),
		};
		assert_eq!(
			gateway.exchange(&alias_poll, Some(source(&client))).await.phase,
			McpLoginPhase::NativeCompleted
		);
		let poll = McpLoginRequest::Poll {
			session_id: request.session_id().clone(),
			work_id: request.work_id().clone(),
		};
		assert_eq!(
			McpLoginGateway::default().exchange(&poll, Some(source(&client))).await.phase,
			McpLoginPhase::Disconnected
		);
		server.await.unwrap();
	}

	#[tokio::test]
	async fn expiration_removes_private_url_but_does_not_replay_an_uncertain_flow() {
		let gateway = McpLoginGateway::default();
		let request = request();
		*gateway.0.lock().await = Some(Session {
			status: McpLoginStatus {
				authorization_url: Some(
					McpAuthorizationUrl::new("https://example.test/private".into()).unwrap(),
				),
				..status(&request, McpLoginPhase::AwaitingUser, "Waiting")
			},
			aliases: Vec::new(),
			work: "work".into(),
			thread: "native-thread".into(),
			server: "server".into(),
			generation: generation(),
			pending: true,
			started: Instant::now() - Duration::from_secs(181),
		});
		gateway.expire().await;
		let slot = gateway.0.lock().await;
		let session = slot.as_ref().unwrap();
		assert!(session.pending);
		assert_eq!(session.status.phase, McpLoginPhase::Expired);
		assert!(session.status.authorization_url.is_none());
		drop(slot);
		gateway.disconnect(None).await;
		assert!(!gateway.0.lock().await.as_ref().unwrap().pending);
	}
}
