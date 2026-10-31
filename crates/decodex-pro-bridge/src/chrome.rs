//! CDP adapter over the Chrome extension's private native-host socket.
//! Browser launch, remote debugging ports, and official desktop routing are absent.

use std::{
	collections::{BTreeMap, BTreeSet},
	path::Path,
	time::Duration,
};

use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use tokio::{net::UnixStream, time::timeout};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

use crate::{Result, identity::ExecutionIdentity};

pub(crate) struct Chrome {
	socket: WebSocketStream<UnixStream>,
	next_id: u64,
	session: Option<String>,
	pub target: Option<String>,
	conversation_url: Option<String>,
	responses: BTreeSet<String>,
	finished: BTreeSet<String>,
	pub identities: Vec<ExecutionIdentity>,
	pub messages: BTreeMap<String, crate::identity::NativeMessage>,
}

impl Chrome {
	pub async fn connect(root: &Path) -> Result<Self> {
		let stream = UnixStream::connect(crate::native_host::socket_path(root)).await
			.map_err(|_| "Decodex Research is not connected. Enable its Chrome extension, then recover the saved request.")?;
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
			identities: Vec::new(),
			messages: BTreeMap::new(),
		};
		let hello = browser.command("Decodex.ping", json!({})).await?;
		if hello["protocol"] != 1 || hello["extensionId"] != crate::native_host::EXTENSION_ID {
			return Err(
				"The research extension identity or protocol does not match this component.".into(),
			);
		}
		Ok(browser)
	}

	pub async fn command(&mut self, method: &str, params: Value) -> Result<Value> {
		self.next_id += 1;
		let id = self.next_id;
		let mut request = json!({"id":id,"method":method,"params":params});
		if let Some(session) = &self.session {
			request["sessionId"] = json!(session);
		}
		self.socket
			.send(Message::Text(request.to_string().into()))
			.await
			.map_err(|e| format!("Chrome disconnected: {e}"))?;
		let response = async {
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
		timeout(Duration::from_secs(30), response).await.map_err(|_| {
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
					&& params["request"]["url"].as_str().is_some_and(|url| {
						conversation_endpoint(url, self.conversation_url.as_deref())
					}) =>
			{
				if let Some(body) = params["request"]["postData"].as_str() {
					// Observe only the user message the page actually submitted. Requested
					// models and assistant context in a request never prove execution identity.
					for message in crate::identity::from_response(body)
						.messages
						.into_iter()
						.filter(|message| message.role == "user")
					{
						self.messages.insert(message.id.clone(), message);
					}
				}
			},
			Some("Network.responseReceived")
				if conversation_response(&params["response"], self.conversation_url.as_deref()) =>
			{
				self.responses.insert(id.into());
			},
			Some("Network.loadingFinished") if self.responses.contains(id) => {
				self.finished.insert(id.into());
			},
			Some("Network.loadingFailed") => {
				self.responses.remove(id);
				self.finished.remove(id);
			},
			_ => {},
		}
	}

	pub async fn login_target(&mut self, saved: Option<&str>) -> Result<Option<String>> {
		let targets = self.command("Target.getTargets", json!({})).await?;
		select_login_target(&targets["targetInfos"], saved)
	}

	pub async fn open(
		&mut self,
		saved_target: Option<&str>,
		url: Option<&str>,
		visible: bool,
	) -> Result<()> {
		self.bind_conversation_url(url.unwrap_or_default());
		let targets = self.command("Target.getTargets", json!({})).await?;
		let existing = saved_target.and_then(|id| {
			targets["targetInfos"]
				.as_array()?
				.iter()
				.find(|target| target["targetId"] == id && target["type"] == "page")
		});
		let target = if let Some(target) = existing {
			target["targetId"].as_str().ok_or("Chrome returned no target identity.")?.to_owned()
		} else {
			if url.is_none() {
				return Err("The pending request tab is gone and no conversation URL was saved. The request was not resent.".into());
			}
			self.command("Target.createTarget", json!({"url":"about:blank","background":!visible}))
				.await?["targetId"]
				.as_str()
				.ok_or("Chrome returned no target identity.")?
				.to_owned()
		};
		self.target = Some(target.clone());
		let attached = self
			.command("Target.attachToTarget", json!({"targetId":target,"flatten":true}))
			.await?;
		self.session = Some(
			attached["sessionId"].as_str().ok_or("Chrome returned no session identity.")?.into(),
		);
		self.responses.clear();
		self.finished.clear();
		self.identities.clear();
		self.messages.clear();
		self.command("Page.enable", json!({})).await?;
		self.command("Runtime.enable", json!({})).await?;
		self.command("Network.enable", json!({})).await?;
		if existing.is_none() {
			self.command("Page.navigate", json!({"url":url.ok_or("Missing target URL.")?})).await?;
		}
		if visible {
			self.session = None;
			self.command("Target.activateTarget", json!({"targetId":target})).await?;
			self.session = Some(attached["sessionId"].as_str().unwrap_or_default().into());
			self.command("Page.bringToFront", json!({})).await?;
		}
		Ok(())
	}

	pub async fn evaluate(&mut self, expression: &str) -> Result<Value> {
		let result = self
			.command(
				"Runtime.evaluate",
				json!({"expression":expression,"awaitPromise":true,"returnByValue":true}),
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
		self.conversation_url = crate::page::conversation_url(url).ok();
	}

	pub async fn collect_identity(&mut self) -> Result<()> {
		for id in std::mem::take(&mut self.finished) {
			self.responses.remove(&id);
			// A reload can evict an old body. The page observation remains recoverable.
			if let Ok(body) = self.command("Network.getResponseBody", json!({"requestId":id})).await
				&& body["base64Encoded"] != true
				&& let Some(body) = body["body"].as_str()
			{
				let data = crate::identity::from_response(body);
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
			self.command("Target.closeTarget", json!({"targetId":target})).await?;
		}
		Ok(())
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
			let Ok(url) = url::Url::parse(raw) else { return false };
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

fn conversation_endpoint(raw: &str, owned: Option<&str>) -> bool {
	let Ok(url) = url::Url::parse(raw) else { return false };
	url.scheme() == "https"
		&& url.host_str() == Some("chatgpt.com")
		&& url.path().starts_with("/backend-api/")
		&& (url.path().split('/').any(|part| part == "conversation")
			|| owned
				.and_then(|url| url.strip_prefix("https://chatgpt.com/c/"))
				.is_some_and(|id| url.path() == format!("/backend-api/conversations/{id}")))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn pro_selection_waits_for_the_closed_picker_and_refuses_an_unconfirmed_maximum() {
		for pro_after_close in [true, false] {
			let root = tempfile::tempdir().unwrap();
			let listener =
				tokio::net::UnixListener::bind(crate::native_host::socket_path(root.path()))
					.unwrap();
			let server = tokio::spawn(async move {
				let (stream, _) = listener.accept().await.unwrap();
				let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
				let mut closed = false;
				while let Some(Ok(Message::Text(text))) = socket.next().await {
					let req: Value = serde_json::from_str(&text).unwrap();
					let result = match req["method"].as_str().unwrap() {
						"Decodex.ping" =>
							json!({"protocol":1,"extensionId":crate::native_host::EXTENSION_ID}),
						"Runtime.evaluate" => {
							let expression = req["params"]["expression"].as_str().unwrap();
							let value = if expression.contains("const label=model.cloneNode") {
								json!(closed && pro_after_close)
							} else if expression.contains("const choices=") {
								json!({"power":true,"options":["Power"]})
							} else if expression.contains("value:Number(slider.getAttribute") {
								// The range has reached Pro while the open picker still has its old
								// label.
								json!({"value":4,"max":4})
							} else {
								json!({"x":10,"y":10})
							};
							json!({"result":{"value":value}})
						},
						"Input.dispatchMouseEvent" => json!({}),
						"Input.dispatchKeyEvent" => {
							assert_eq!(req["params"]["key"], "Escape");
							closed = true;
							json!({})
						},
						other => panic!("Unexpected operation {other}"),
					};
					socket
						.send(Message::Text(
							json!({"id":req["id"],"result":result}).to_string().into(),
						))
						.await
						.unwrap();
				}
			});
			let mut browser = Chrome::connect(root.path()).await.unwrap();
			let result = crate::page::select_pro(&mut browser).await;
			assert_eq!(result.is_ok(), pro_after_close, "{result:?}");
			drop(browser);
			server.await.unwrap();
		}
	}
}
