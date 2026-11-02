//! CDP adapter over the Chrome extension's private native-host socket.
//! Browser launch, remote debugging ports, and official desktop routing are absent.

use std::{
	collections::{BTreeMap, BTreeSet},
	mem,
	path::Path,
	time::Duration,
};

use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::Value;
use tokio::{
	net::UnixStream,
	time::{self},
};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};
use url::Url;

use crate::{
	Result,
	identity::{self, ExecutionIdentity, NativeMessage},
	native_host::{self, EXTENSION_ID},
	page,
};

pub(crate) struct Chrome {
	socket: WebSocketStream<UnixStream>,
	next_id: u64,
	session: Option<String>,
	pub target: Option<String>,
	conversation_url: Option<String>,
	responses: BTreeSet<String>,
	finished: BTreeSet<String>,
	submissions: BTreeSet<String>,
	pub submission_failure: Option<String>,
	pub resume_failure: Option<String>,
	pub identities: Vec<ExecutionIdentity>,
	pub messages: BTreeMap<String, NativeMessage>,
}
impl Chrome {
	pub async fn connect(root: &Path) -> Result<Self> {
		let stream = UnixStream::connect(native_host::socket_path(root)).await
			.map_err(|_| "Decodex Pro Research is not connected. Enable its Chrome extension, then recover the saved request.")?;
		let (socket, _) = tokio_tungstenite::client_async("ws://localhost/research", stream)
			.await
			.map_err(|e| e.to_string())?;
		let mut browser = Self {
			socket,
			next_id: 0,
			session: None,
			target: None,
			conversation_url: None,
			responses: BTreeSet::new(),
			finished: BTreeSet::new(),
			submissions: BTreeSet::new(),
			submission_failure: None,
			resume_failure: None,
			identities: Vec::new(),
			messages: BTreeMap::new(),
		};
		let hello = browser.command("Decodex.ping", serde_json::json!({})).await?;

		if hello["protocol"] != 1 || hello["extensionId"] != EXTENSION_ID {
			return Err(
				"The research extension identity or protocol does not match this component.".into(),
			);
		}

		Ok(browser)
	}

	pub async fn command(&mut self, method: &str, params: Value) -> Result<Value> {
		self.next_id += 1;

		let id = self.next_id;
		let mut request = serde_json::json!({"id":id,"method":method,"params":params});

		if let Some(session) = &self.session {
			request["sessionId"] = serde_json::json!(session);
		}

		let response = async {
			self.socket
				.send(Message::Text(request.to_string().into()))
				.await
				.map_err(|e| format!("Chrome disconnected: {e}"))?;

			loop {
				let message = self
					.socket
					.next()
					.await
					.ok_or("Chrome disconnected. Resume the saved request.")?
					.map_err(|e| e.to_string())?;
				let Message::Text(text) = message else { continue };
				let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;

				if value["id"] == id {
					if let Some(error) = value.get("error") {
						return Err(format!("Chrome {method}: {error}"));
					}

					return Ok(value["result"].clone());
				}

				self.event(&value);
			}
		};

		time::timeout(Duration::from_secs(30), response).await.map_err(|_| {
			format!("Chrome timed out during {method}. The saved request can be resumed.")
		})?
	}

	fn event(&mut self, value: &Value) {
		if value["sessionId"].as_str() != self.session.as_deref() {
			return;
		}

		let params = &value["params"];
		let Some(id) = params["requestId"].as_str() else { return };

		match value["method"].as_str() {
			Some("Network.requestWillBeSent")
				if params["request"]["method"] == "POST"
					&& params["request"]["url"].as_str().is_some_and(submission_endpoint) =>
			{
				self.submissions.insert(id.into());

				if let Some(body) = params["request"]["postData"].as_str() {
					// Observe only the user message the page actually submitted. Requested
					// models and assistant context in a request never prove execution identity.
					for message in identity::from_response(body)
						.messages
						.into_iter()
						.filter(|message| message.role == "user")
					{
						self.messages.insert(message.id.clone(), message);
					}
				}
			},
			Some("Network.responseReceived") => {
				if let Some(status) = params["response"]["status"].as_f64().filter(|s| *s >= 400.)
					&& params["response"]["url"].as_str().is_some_and(|url| {
						conversation_endpoint(url, None) && url.ends_with("/conversation/resume")
					}) {
					self.resume_failure = Some(format!(
						"ChatGPT response-stream recovery returned HTTP {status:.0}."
					));
				}

				if self.submissions.contains(id)
					&& let Some(status) =
						params["response"]["status"].as_f64().filter(|s| *s >= 400.)
				{
					self.submission_failure =
						Some(format!("ChatGPT returned HTTP {status:.0} for this submission."));
				}
				if conversation_response(&params["response"], self.conversation_url.as_deref()) {
					self.responses.insert(id.into());
				}
			},
			Some("Network.loadingFinished") => {
				self.submissions.remove(id);

				if self.responses.contains(id) {
					self.finished.insert(id.into());
				}
			},
			Some("Network.loadingFailed") => {
				if self.submissions.remove(id) && params["canceled"] != true {
					self.submission_failure.get_or_insert_with(|| {
						format!(
							"ChatGPT submission transport failed: {}.",
							params["errorText"].as_str().unwrap_or("connection interrupted")
						)
					});
				}

				self.responses.remove(id);
				self.finished.remove(id);
			},
			_ => {},
		}
	}

	pub async fn login_target(&mut self, saved: Option<&str>) -> Result<Option<String>> {
		let targets = self.command("Target.getTargets", serde_json::json!({})).await?;

		select_login_target(&targets["targetInfos"], saved)
	}

	pub async fn open(
		&mut self,
		saved_target: Option<&str>,
		url: Option<&str>,
		visible: bool,
	) -> Result<()> {
		let targets = self.command("Target.getTargets", serde_json::json!({})).await?;
		let mut existing = saved_target.and_then(|id| {
			targets["targetInfos"]
				.as_array()?
				.iter()
				.find(|target| target["targetId"] == id && target["type"] == "page")
				.cloned()
		});
		let mut destination = url.map(str::to_owned);

		if let Some(target) = &existing {
			let id = target["targetId"].as_str().ok_or("Chrome returned no target identity.")?;

			self.attach(id).await?;

			let scope = self
				.evaluate("globalThis[Symbol.for('decodex.research.browser-state')]?.scope")
				.await?;

			if scope.as_str() != Some(id) {
				// Do not reload or clear a pre-isolation page: it can hold the user's
				// restored draft. Recover its exact conversation in a new owned tab.
				if destination.is_none() {
					destination =
						target["url"].as_str().and_then(|url| page::conversation_url(url).ok());
				}

				existing = None;
			}
		}

		if existing.is_none() {
			if destination.is_none() {
				return Err("The pending request has no isolated tab or saved conversation URL. The request was not resent.".into());
			}

			self.session = None;

			let created = self
				.command(
					"Target.createTarget",
					serde_json::json!({"url":"about:blank","background":!visible}),
				)
				.await?;
			let id = created["targetId"]
				.as_str()
				.ok_or("Chrome returned no target identity.")?
				.to_owned();

			self.attach(&id).await?;
		}

		let target =
			self.target.as_deref().ok_or("Chrome returned no target identity.")?.to_owned();

		self.bind_conversation_url(destination.as_deref().unwrap_or_default());
		self.responses.clear();
		self.finished.clear();
		self.identities.clear();
		self.messages.clear();
		self.submissions.clear();

		self.submission_failure = None;
		self.resume_failure = None;

		self.command("Page.enable", serde_json::json!({})).await?;
		self.command("Runtime.enable", serde_json::json!({})).await?;

		let script = format!(
			"{}({});",
			include_str!("../assets/browser-state.js"),
			serde_json::to_string(&target).map_err(|e| e.to_string())?
		);

		self.command("Page.addScriptToEvaluateOnNewDocument", serde_json::json!({"source":script}))
			.await?;
		// Retain the entire submitted body for input identity, including long research
		// material. Chrome's default post-data capture can omit a large input.
		self.command("Network.enable", serde_json::json!({"maxPostDataSize":32*1_024*1_024}))
			.await?;

		if existing.is_none()
			|| destination.as_ref().is_some_and(|url| {
				existing.as_ref().is_some_and(|target| target["url"].as_str() != Some(url.as_str()))
			}) {
			self.command(
				"Page.navigate",
				serde_json::json!({"url":destination.ok_or("Missing target URL.")?}),
			)
			.await?;
		}
		if visible {
			let session = self.session.take();

			self.command("Target.activateTarget", serde_json::json!({"targetId":target})).await?;

			self.session = session;

			self.command("Page.bringToFront", serde_json::json!({})).await?;
		}

		Ok(())
	}

	// Passive setup inspection must not replace a pre-isolation draft page.
	pub(crate) async fn attach(&mut self, target: &str) -> Result<()> {
		self.session = None;
		self.target = Some(target.into());

		let attached = self
			.command("Target.attachToTarget", serde_json::json!({"targetId":target,"flatten":true}))
			.await?;

		self.session = Some(
			attached["sessionId"].as_str().ok_or("Chrome returned no session identity.")?.into(),
		);

		Ok(())
	}

	pub async fn evaluate(&mut self, expression: &str) -> Result<Value> {
		let result = self
			.command(
				"Runtime.evaluate",
				serde_json::json!({"expression":expression,"awaitPromise":true,"returnByValue":true}),
			)
			.await?;

		if result.get("exceptionDetails").is_some() {
			let description = result["exceptionDetails"]["exception"]["description"]
				.as_str()
				.or_else(|| result["exceptionDetails"]["text"].as_str())
				.unwrap_or("Page operation failed.");

			return Err(description.into());
		}

		Ok(result["result"]["value"].clone())
	}

	pub fn bind_conversation_url(&mut self, url: &str) {
		self.conversation_url = page::conversation_url(url).ok();
	}

	pub async fn collect_identity(&mut self) -> Result<()> {
		for id in mem::take(&mut self.finished) {
			self.responses.remove(&id);

			// A reload can evict an old body. The page observation remains recoverable.
			if let Ok(body) =
				self.command("Network.getResponseBody", serde_json::json!({"requestId":id})).await
				&& body["base64Encoded"] != true
				&& let Some(body) = body["body"].as_str()
			{
				let data = identity::from_response(body);

				self.identities.extend(data.identities);

				for message in data.messages {
					if !self
						.messages
						.get(&message.id)
						.is_some_and(|known| known.complete && !message.complete)
					{
						self.messages.insert(message.id.clone(), message);
					}
				}
			}
		}

		Ok(())
	}

	pub async fn close_tab(&mut self) -> Result<()> {
		self.session = None;

		if let Some(target) = self.target.take() {
			self.command("Target.closeTarget", serde_json::json!({"targetId":target})).await?;
		}

		Ok(())
	}

	pub async fn close_completed_tab(&mut self) -> Result<()> {
		if !page::can_close(self).await? {
			return Ok(());
		}

		self.evaluate("globalThis[Symbol.for('decodex.research.browser-state')]?.release()")
			.await?;

		self.close_tab().await
	}

	pub async fn detach(&mut self) -> Result<()> {
		self.session = None;

		self.socket.close(None).await.map_err(|e| e.to_string())
	}
}

fn select_login_target(targets: &Value, saved: Option<&str>) -> Result<Option<String>> {
	let pages: Vec<_> = targets
		.as_array()
		.ok_or("Chrome returned no page inventory.")?
		.iter()
		.filter(|target| target["type"] == "page")
		.collect();

	if let Some(target) = pages.iter().find(|target| target["targetId"].as_str() == saved) {
		return Ok(target["targetId"].as_str().map(str::to_owned));
	}

	let candidates: Vec<_> = pages
		.into_iter()
		.filter(|target| {
			let Some(raw) = target["url"].as_str() else { return false };
			let Ok(url) = Url::parse(raw) else { return false };

			url.scheme() == "https"
				&& match url.host_str() {
					Some("auth.openai.com") => true,
					Some("chatgpt.com") => url.path() == "/" || url.path().starts_with("/auth/"),
					_ => false,
				}
		})
		.collect();

	match candidates.as_slice() {
		[] => Ok(None),
		[target] => Ok(target["targetId"].as_str().map(str::to_owned)),
		_ => Err("Multiple ChatGPT login pages are open in the research browser. Keep one login page; no new window was opened.".into()),
	}
}

fn conversation_response(response: &Value, owned: Option<&str>) -> bool {
	let Some(raw) = response["url"].as_str() else { return false };

	conversation_endpoint(raw, owned) && response["status"].as_u64() == Some(200)
}

fn submission_endpoint(raw: &str) -> bool {
	let Ok(url) = Url::parse(raw) else { return false };

	// Preparation and response-stream recovery are separate page requests. In
	// particular, a missing resume stream does not prove the original Send failed.
	url.scheme() == "https"
		&& url.host_str() == Some("chatgpt.com")
		&& matches!(url.path(), "/backend-api/conversation" | "/backend-api/f/conversation")
}

fn conversation_endpoint(raw: &str, owned: Option<&str>) -> bool {
	let Ok(url) = Url::parse(raw) else { return false };

	url.scheme() == "https"
		&& url.host_str() == Some("chatgpt.com")
		&& url.path().starts_with("/backend-api/")
		&& (url.path().split('/').any(|part| part == "conversation")
			|| owned
				.and_then(|url| page::conversation_url(url).ok())
				.and_then(|url| url.rsplit('/').next().map(str::to_owned))
				.is_some_and(|id| url.path() == format!("/backend-api/conversations/{id}")))
}

#[cfg(test)]
mod tests {
	use futures_util::{SinkExt as _, StreamExt as _};

	use crate::{
		chrome::{Chrome, Message, Value},
		native_host, page,
	};

	#[tokio::test]
	async fn submission_failures_keep_the_network_cause_and_ignore_unrelated_traffic() {
		let root = tempfile::tempdir().unwrap();
		let listener =
			tokio::net::UnixListener::bind(native_host::socket_path(root.path())).unwrap();
		let server = tokio::spawn(async move {
			let (stream, _) = listener.accept().await.unwrap();
			let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
			let Message::Text(request) = socket.next().await.unwrap().unwrap() else {
				panic!("Missing request")
			};
			let request: Value = serde_json::from_str(&request).unwrap();

			socket.send(Message::Text(serde_json::json!({"id":request["id"],"result":{"protocol":1,"extensionId":crate::native_host::EXTENSION_ID}}).to_string().into())).await.unwrap();
		});
		let mut browser = Chrome::connect(root.path()).await.unwrap();

		browser.session = Some("owned".into());

		let event = |method: &str, params: Value| serde_json::json!({"sessionId":"owned","method":method,"params":params});

		browser.event(&event(
			"Network.loadingFailed",
			serde_json::json!({"requestId":"unrelated","errorText":"net::ERR_FAILED"}),
		));

		assert_eq!(browser.submission_failure, None);

		browser.event(&serde_json::json!({"sessionId":"another-tab","method":"Network.requestWillBeSent","params":{"requestId":"foreign","request":{"method":"POST","url":"https://chatgpt.com/backend-api/f/conversation"}}}));

		assert!(browser.submissions.is_empty());

		for (id, path) in [
			("resume", "f/conversation/resume"),
			("prepare", "f/conversation/prepare"),
			("feedback", "conversation/owned/feedback"),
		] {
			let url = format!("https://chatgpt.com/backend-api/{path}");

			browser.event(&event(
				"Network.requestWillBeSent",
				serde_json::json!({"requestId":id,"request":{"method":"POST","url":url}}),
			));
			browser.event(&event(
				"Network.responseReceived",
				serde_json::json!({"requestId":id,"response":{"status":404,"url":url,"mimeType":"application/json"}}),
			));

			assert!(browser.submission_failure.is_none(), "{path} is not a new submission");
		}

		assert_eq!(
			browser.resume_failure.as_deref(),
			Some("ChatGPT response-stream recovery returned HTTP 404.")
		);

		browser.event(&event("Network.requestWillBeSent", serde_json::json!({"requestId":"own","request":{"method":"POST","url":"https://chatgpt.com/backend-api/f/conversation"}})));
		browser.event(&event("Network.responseReceived", serde_json::json!({"requestId":"own","response":{"status":413,"url":"https://chatgpt.com/backend-api/f/conversation","mimeType":"text/html"}})));

		assert_eq!(
			browser.submission_failure.as_deref(),
			Some("ChatGPT returned HTTP 413 for this submission.")
		);

		browser.event(&event(
			"Network.loadingFailed",
			serde_json::json!({"requestId":"own","errorText":"net::ERR_FAILED"}),
		));

		assert_eq!(
			browser.submission_failure.as_deref(),
			Some("ChatGPT returned HTTP 413 for this submission.")
		);

		browser.submission_failure = None;

		browser.event(&event("Network.requestWillBeSent", serde_json::json!({"requestId":"next","request":{"method":"POST","url":"https://chatgpt.com/backend-api/f/conversation"}})));
		browser.event(&event(
			"Network.loadingFailed",
			serde_json::json!({"requestId":"next","errorText":"net::ERR_FAILED"}),
		));

		assert_eq!(
			browser.submission_failure.as_deref(),
			Some("ChatGPT submission transport failed: net::ERR_FAILED.")
		);

		server.await.unwrap();
	}

	#[tokio::test]
	async fn retry_navigates_the_owned_projects_view_back_to_the_requested_page() {
		let root = tempfile::tempdir().unwrap();
		let listener =
			tokio::net::UnixListener::bind(native_host::socket_path(root.path())).unwrap();
		let server = tokio::spawn(async move {
			let (stream, _) = listener.accept().await.unwrap();
			let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
			let mut navigations = 0;

			while let Some(Ok(Message::Text(text))) = socket.next().await {
				let request: Value = serde_json::from_str(&text).unwrap();
				let result = match request["method"].as_str().unwrap() {
					"Decodex.ping" =>
						serde_json::json!({"protocol":1,"extensionId":crate::native_host::EXTENSION_ID}),
					"Target.getTargets" =>
						serde_json::json!({"targetInfos":[{"targetId":"owned","type":"page","url":"https://chatgpt.com/projects"}]}),
					"Target.attachToTarget" => serde_json::json!({"sessionId":"owned"}),
					"Runtime.evaluate" => serde_json::json!({"result":{"value":"owned"}}),
					"Page.addScriptToEvaluateOnNewDocument" =>
						serde_json::json!({"identifier":"scope"}),
					"Page.navigate" => {
						assert_eq!(request["params"]["url"], "https://chatgpt.com/");

						navigations += 1;

						serde_json::json!({})
					},
					"Network.enable" => {
						assert_eq!(request["params"]["maxPostDataSize"], 32 * 1_024 * 1_024);

						serde_json::json!({})
					},
					"Page.enable" | "Runtime.enable" => serde_json::json!({}),
					other => panic!("Unexpected operation {other}"),
				};

				socket
					.send(Message::Text(
						serde_json::json!({"id":request["id"],"result":result}).to_string().into(),
					))
					.await
					.unwrap();
			}

			navigations
		});
		let mut browser = Chrome::connect(root.path()).await.unwrap();

		browser.open(Some("owned"), Some("https://chatgpt.com/"), false).await.unwrap();

		assert_eq!(browser.target.as_deref(), Some("owned"));

		drop(browser);

		assert_eq!(server.await.unwrap(), 1);
	}

	#[tokio::test]
	async fn legacy_pages_are_preserved_and_recovered_only_from_a_known_destination() {
		for (saved_url, requested, destination) in [
			("https://chatgpt.com/", Some("https://chatgpt.com/"), Some("https://chatgpt.com/")),
			("https://chatgpt.com/c/owned", None, Some("https://chatgpt.com/c/owned")),
			("https://chatgpt.com/", None, None),
		] {
			let root = tempfile::tempdir().unwrap();
			let listener =
				tokio::net::UnixListener::bind(native_host::socket_path(root.path())).unwrap();
			let server = tokio::spawn(async move {
				let (stream, _) = listener.accept().await.unwrap();
				let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
				let mut registered = false;
				let mut created = false;
				let mut navigated = false;

				while let Some(Ok(Message::Text(text))) = socket.next().await {
					let request: Value = serde_json::from_str(&text).unwrap();
					let result = match request["method"].as_str().unwrap() {
						"Decodex.ping" =>
							serde_json::json!({"protocol":1,"extensionId":crate::native_host::EXTENSION_ID}),
						"Target.getTargets" =>
							serde_json::json!({"targetInfos":[{"targetId":"legacy","type":"page","url":saved_url}]}),
						"Target.attachToTarget" =>
							serde_json::json!({"sessionId":request["params"]["targetId"]}),
						"Runtime.evaluate" => serde_json::json!({"result":{}}),
						"Target.createTarget" => {
							assert!(destination.is_some());
							assert_eq!(request["params"]["url"], "about:blank");

							created = true;

							serde_json::json!({"targetId":"isolated"})
						},
						"Page.addScriptToEvaluateOnNewDocument" => {
							assert!(created);
							assert_eq!(request["sessionId"], "isolated");
							assert!(
								request["params"]["source"]
									.as_str()
									.unwrap()
									.contains("codex.browser.persistedAtomState")
							);

							registered = true;

							serde_json::json!({"identifier":"scope"})
						},
						"Page.navigate" => {
							assert!(
								registered,
								"The application loaded before its editor state was isolated"
							);
							assert_eq!(
								request["sessionId"], "isolated",
								"The legacy draft page was changed"
							);
							assert_eq!(request["params"]["url"].as_str(), destination);

							navigated = true;

							serde_json::json!({})
						},
						"Page.enable" | "Runtime.enable" | "Network.enable" =>
							serde_json::json!({}),
						other => panic!("Unexpected operation {other}"),
					};

					socket
						.send(Message::Text(
							serde_json::json!({"id":request["id"],"result":result})
								.to_string()
								.into(),
						))
						.await
						.unwrap();
				}

				assert_eq!(created, destination.is_some());
				assert_eq!(navigated, destination.is_some());
			});
			let mut browser = Chrome::connect(root.path()).await.unwrap();
			let result = browser.open(Some("legacy"), requested, false).await;

			if destination.is_some() {
				result.unwrap();

				assert_eq!(browser.target.as_deref(), Some("isolated"));
			} else {
				assert!(result.unwrap_err().contains("not resent"));
			}

			drop(browser);

			server.await.unwrap();
		}
	}

	#[tokio::test]
	async fn completed_tabs_release_their_state_but_keep_new_unsent_content() {
		for can_close in [true, false] {
			let root = tempfile::tempdir().unwrap();
			let listener =
				tokio::net::UnixListener::bind(native_host::socket_path(root.path())).unwrap();
			let server = tokio::spawn(async move {
				let (stream, _) = listener.accept().await.unwrap();
				let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
				let mut released = false;
				let mut closed = false;

				while let Some(Ok(Message::Text(text))) = socket.next().await {
					let request: Value = serde_json::from_str(&text).unwrap();
					let result = match request["method"].as_str().unwrap() {
						"Decodex.ping" =>
							serde_json::json!({"protocol":1,"extensionId":crate::native_host::EXTENSION_ID}),
						"Runtime.evaluate" => {
							if request["params"]["expression"]
								.as_str()
								.unwrap()
								.contains("?.release()")
							{
								assert!(can_close);

								released = true;
							}

							serde_json::json!({"result":{"value":can_close}})
						},
						"Target.closeTarget" => {
							assert!(
								released,
								"Closing a completed tab left its isolated state behind"
							);
							assert_eq!(request["params"]["targetId"], "owned");

							closed = true;

							serde_json::json!({"success":true})
						},
						other => panic!("Unexpected operation {other}"),
					};

					socket
						.send(Message::Text(
							serde_json::json!({"id":request["id"],"result":result})
								.to_string()
								.into(),
						))
						.await
						.unwrap();
				}

				assert_eq!(released, can_close);
				assert_eq!(closed, can_close);
			});
			let mut browser = Chrome::connect(root.path()).await.unwrap();

			browser.target = Some("owned".into());
			browser.session = Some("owned".into());

			browser.close_completed_tab().await.unwrap();

			assert_eq!(browser.target.is_some(), !can_close);

			drop(browser);

			server.await.unwrap();
		}
	}

	#[tokio::test]
	async fn pro_selection_waits_for_the_closed_picker_and_refuses_an_unconfirmed_maximum() {
		for pro_after_close in [true, false] {
			let root = tempfile::tempdir().unwrap();
			let listener =
				tokio::net::UnixListener::bind(native_host::socket_path(root.path())).unwrap();
			let server = tokio::spawn(async move {
				let (stream, _) = listener.accept().await.unwrap();
				let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
				let mut closed = false;

				while let Some(Ok(Message::Text(text))) = socket.next().await {
					let req: Value = serde_json::from_str(&text).unwrap();
					let result = match req["method"].as_str().unwrap() {
						"Decodex.ping" =>
							serde_json::json!({"protocol":1,"extensionId":crate::native_host::EXTENSION_ID}),
						"Runtime.evaluate" => {
							let expression = req["params"]["expression"].as_str().unwrap();
							let value = if expression.contains("const label=model.cloneNode") {
								serde_json::json!(closed && pro_after_close)
							} else if expression.contains("const choices=") {
								serde_json::json!({"power":true,"options":["Power"]})
							} else if expression.contains("value:Number(slider.getAttribute") {
								// The range has reached Pro while the open picker still has its old
								// label.
								serde_json::json!({"value":4,"max":4})
							} else {
								serde_json::json!({"x":10,"y":10})
							};

							serde_json::json!({"result":{"value":value}})
						},
						"Input.dispatchMouseEvent" => serde_json::json!({}),
						"Input.dispatchKeyEvent" => {
							assert_eq!(req["params"]["key"], "Escape");

							closed = true;

							serde_json::json!({})
						},
						other => panic!("Unexpected operation {other}"),
					};

					socket
						.send(Message::Text(
							serde_json::json!({"id":req["id"],"result":result}).to_string().into(),
						))
						.await
						.unwrap();
				}
			});
			let mut browser = Chrome::connect(root.path()).await.unwrap();
			let result = page::select_pro(&mut browser).await;

			assert_eq!(result.is_ok(), pro_after_close, "{result:?}");

			drop(browser);

			server.await.unwrap();
		}
	}
}
