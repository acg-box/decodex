//! One private Chrome profile. CDP never attaches to a user's ordinary browser.

use std::{
	collections::BTreeSet,
	fs,
	path::Path,
	process::{Command, Stdio},
	time::Duration,
};

use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use tokio::{
	net::TcpStream,
	time::{Instant, sleep, timeout},
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::Message};

use crate::{Config, Result, identity::ExecutionIdentity, store::Store};

pub(crate) struct Chrome {
	socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
	owned: bool,
	next_id: u64,
	session: Option<String>,
	pub target: Option<String>,
	responses: BTreeSet<String>,
	finished: BTreeSet<String>,
	pub identities: Vec<ExecutionIdentity>,
}

impl Chrome {
	/// Chrome 144+ owns the permission prompt for an existing browser. Do not
	/// change its launch options, profile settings, cookies or automation signals.
	pub async fn attach_existing(profile: &Path) -> Result<Self> {
		let endpoint = endpoint(profile).map_err(|_| "Open Chrome and enable Remote Debugging at chrome://inspect/#remote-debugging before requesting a connection.")?;
		let mut browser = timeout(Duration::from_secs(90), Self::attach(&endpoint))
			.await
			.map_err(|_| "Chrome did not approve the connection in time.")??;
		browser.owned = false;
		// Some Chrome versions accept the socket before resolving the approval UI.
		// Keep the same connection and let Chrome allow or refuse the first command.
		browser
			.command_with_timeout("Browser.getVersion", json!({}), Duration::from_secs(90))
			.await?;
		Ok(browser)
	}

	pub async fn existing(root: &Path) -> Result<Option<Self>> {
		let profile = root.join("chrome-profile");
		if !profile.join("DevToolsActivePort").is_file() {
			return Ok(None);
		}
		let endpoint = endpoint(&profile)?;
		match timeout(Duration::from_secs(2), Self::attach(&endpoint)).await {
			Ok(Ok(browser)) => Ok(Some(browser)),
			_ => Ok(None),
		}
	}

	pub async fn connect(config: &Config, root: &Path) -> Result<Self> {
		let profile = root.join("chrome-profile");
		Store::open(&profile)?;
		let store = Store::open(root)?;
		let deadline = Instant::now() + Duration::from_secs(20);
		let _launch = loop {
			if let Some(lease) = store.browser_lease()? {
				break lease;
			}
			if Instant::now() >= deadline {
				return Err("The research browser is still starting. Try again shortly.".into());
			}
			sleep(Duration::from_millis(100)).await;
		};
		if let Ok(endpoint) = endpoint(&profile)
			&& let Ok(Ok(socket)) = timeout(Duration::from_secs(2), Self::attach(&endpoint)).await
		{
			return Ok(socket);
		}
		if !config.chrome.is_file() {
			return Err("Google Chrome is not installed at the configured path. Install Chrome to connect ChatGPT.".into());
		}
		// Chrome's own profile lock handles an interrupted launch. No live profile is copied.
		let mut command = Command::new(&config.chrome);
		command
			.arg(format!("--user-data-dir={}", profile.display()))
			.args([
				"--remote-debugging-port=0",
				"--remote-debugging-address=127.0.0.1",
				"--no-first-run",
				"--no-default-browser-check",
			])
			.stdin(Stdio::null())
			.stdout(Stdio::null())
			.stderr(Stdio::null());
		if config.headless {
			command.arg("--headless=new");
		}
		#[cfg(unix)]
		{
			use std::os::unix::process::CommandExt as _;
			command.process_group(0);
		}
		command.arg("about:blank");
		let mut child = command.spawn().map_err(|e| format!("Chrome could not start: {e}"))?;
		let deadline = Instant::now() + Duration::from_secs(20);
		loop {
			if let Ok(endpoint) = endpoint(&profile)
				&& let Ok(Ok(socket)) =
					timeout(Duration::from_secs(2), Self::attach(&endpoint)).await
			{
				// Reap an eventual exit without terminating the profile or its in-flight answer.
				std::thread::spawn(move || {
					let _ = child.wait();
				});
				return Ok(socket);
			}
			if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
				return Err(format!("The dedicated Chrome exited before it was ready: {status}"));
			}
			if Instant::now() >= deadline {
				std::thread::spawn(move || {
					let _ = child.wait();
				});
				return Err("The dedicated Chrome did not become ready. Its profile and saved requests were retained.".into());
			}
			sleep(Duration::from_millis(100)).await;
		}
	}

	async fn attach(endpoint: &str) -> Result<Self> {
		let (socket, _) =
			tokio_tungstenite::connect_async(endpoint).await.map_err(|e| e.to_string())?;
		Ok(Self {
			socket,
			owned: true,
			next_id: 0,
			session: None,
			target: None,
			responses: BTreeSet::new(),
			finished: BTreeSet::new(),
			identities: Vec::new(),
		})
	}

	pub async fn command(&mut self, method: &str, params: Value) -> Result<Value> {
		self.command_with_timeout(method, params, Duration::from_secs(30)).await
	}

	async fn command_with_timeout(
		&mut self,
		method: &str,
		params: Value,
		wait: Duration,
	) -> Result<Value> {
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
		timeout(wait, async {
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
		})
		.await
		.map_err(|_| {
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
			Some("Network.responseReceived") if conversation_response(&params["response"]) => {
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

	pub async fn collect_identity(&mut self) -> Result<()> {
		for id in std::mem::take(&mut self.finished) {
			self.responses.remove(&id);
			// A reload can evict an old body. The page observation remains recoverable.
			if let Ok(body) = self.command("Network.getResponseBody", json!({"requestId":id})).await
				&& body["base64Encoded"] != true
				&& let Some(body) = body["body"].as_str()
			{
				self.identities.extend(crate::identity::from_response(body));
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

	pub async fn close_browser(&mut self) -> Result<()> {
		self.session = None;
		if !self.owned {
			return self.socket.close(None).await.map_err(|e| e.to_string());
		}
		// Browser.close can close the socket before its acknowledgement.
		self.next_id += 1;
		self.socket
			.send(Message::Text(
				json!({"id":self.next_id,"method":"Browser.close"}).to_string().into(),
			))
			.await
			.map_err(|e| e.to_string())
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

fn endpoint(profile: &Path) -> Result<String> {
	let text = fs::read_to_string(profile.join("DevToolsActivePort")).map_err(|e| e.to_string())?;
	let mut lines = text.lines();
	let port: u16 = lines.next().unwrap_or_default().parse().map_err(|_| "Invalid Chrome port.")?;
	let path = lines
		.next()
		.filter(|path| {
			path.starts_with("/devtools/browser/") && !path.contains(['?', '#', ' ', '\\'])
		})
		.ok_or("Invalid Chrome endpoint.")?;
	if port == 0 {
		return Err("Invalid Chrome port.".into());
	}
	Ok(format!("ws://127.0.0.1:{port}{path}"))
}

fn conversation_response(response: &Value) -> bool {
	let Some(raw) = response["url"].as_str() else { return false };
	let Ok(url) = url::Url::parse(raw) else { return false };
	url.scheme() == "https"
		&& url.host_str() == Some("chatgpt.com")
		&& url.path().starts_with("/backend-api/")
		&& url.path().split('/').any(|part| part == "conversation")
		&& response["status"].as_u64().is_some_and(|status| status == 200)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn browser_endpoint_is_bound_to_its_private_loopback_profile() {
		let directory = tempfile::tempdir().unwrap();
		fs::write(directory.path().join("DevToolsActivePort"), "12345\n/devtools/browser/owned\n")
			.unwrap();
		assert_eq!(
			endpoint(directory.path()).unwrap(),
			"ws://127.0.0.1:12345/devtools/browser/owned"
		);
		fs::write(directory.path().join("DevToolsActivePort"), "0\nws://other\n").unwrap();
		assert!(endpoint(directory.path()).is_err());
	}

	#[test]
	fn identity_capture_excludes_credentials_and_other_origins() {
		assert!(conversation_response(
			&json!({"url":"https://chatgpt.com/backend-api/conversation/one","status":200})
		));
		for url in [
			"https://chatgpt.com/backend-api/me",
			"https://chatgpt.com/backend-api/conversations?limit=20",
			"https://chatgpt.com.evil.test/backend-api/conversation/one",
		] {
			assert!(!conversation_response(&json!({"url":url,"status":200})));
		}
	}

	#[test]
	fn login_follows_one_auth_tab_without_selecting_a_research_or_foreign_tab() {
		let pages = json!([
			{"type":"page","targetId":"work","url":"https://chatgpt.com/c/research"},
			{"type":"page","targetId":"foreign","url":"https://auth.openai.com.evil.test/"},
			{"type":"page","targetId":"login","url":"https://auth.openai.com/oauth/authorize?state=private"}
		]);
		assert_eq!(select_login_target(&pages, Some("closed")).unwrap().as_deref(), Some("login"));
		assert_eq!(select_login_target(&pages, Some("work")).unwrap().as_deref(), Some("work"));
		assert!(select_login_target(&json!([]), Some("closed")).unwrap().is_none());
		let ambiguous = json!([
			{"type":"page","targetId":"one","url":"https://auth.openai.com/oauth/authorize"},
			{"type":"page","targetId":"two","url":"https://chatgpt.com/"}
		]);
		assert!(select_login_target(&ambiguous, None).is_err());
	}

	#[tokio::test]
	async fn borrowed_browser_disconnect_never_sends_browser_close() {
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let port = listener.local_addr().unwrap().port();
		let profile = tempfile::tempdir().unwrap();
		fs::write(
			profile.path().join("DevToolsActivePort"),
			format!("{port}\n/devtools/browser/approved\n"),
		)
		.unwrap();
		let server = tokio::spawn(async move {
			let (stream, _) = listener.accept().await.unwrap();
			let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
			let frame = socket.next().await.unwrap().unwrap();
			let command: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
			assert_eq!(command["method"], "Browser.getVersion");
			socket
				.send(Message::Text(
					json!({"id":command["id"],"result":{"product":"Chrome/156"}})
						.to_string()
						.into(),
				))
				.await
				.unwrap();
			assert!(
				matches!(socket.next().await.unwrap().unwrap(), Message::Close(_)),
				"Borrowed Chrome must only disconnect, never quit"
			);
		});
		let mut browser = Chrome::attach_existing(profile.path()).await.unwrap();
		assert!(!browser.owned);
		browser.close_browser().await.unwrap();
		server.await.unwrap();
	}

	#[tokio::test]
	#[ignore = "Runs installed Chrome in a new headless test profile. No account or network requests."]
	async fn managed_browser_retains_owned_tab_across_cdp_disconnect() {
		let directory = tempfile::tempdir().unwrap();
		let mut config =
			Config::chrome("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
		config.headless = true;
		let mut first = Chrome::connect(&config, directory.path()).await.unwrap();
		first.open(None, Some("about:blank"), false).await.unwrap();
		let target = first.target.clone().unwrap();
		first
			.evaluate("globalThis.recoveryMarker='kept';document.title='Decodex isolated CDP';true")
			.await
			.unwrap();
		drop(first);
		let mut second = Chrome::connect(&config, directory.path()).await.unwrap();
		second.open(Some(&target), None, false).await.unwrap();
		let outcome: Result<()> = async {
			if second.target.as_deref() != Some(target.as_str()) || second.evaluate("globalThis.recoveryMarker").await? != "kept" {
				return Err("The recovered browser target changed.".into());
			}
			// Exercise real Chromium input and result extraction without a provider account.
			second.evaluate(r#"document.body.innerHTML='<div id="prompt-textarea" contenteditable="true"></div><button data-testid="model-switcher-dropdown-button">Instant</button><button data-testid="send-button">Send</button>';
                document.querySelector('[data-testid="model-switcher-dropdown-button"]').onclick=()=>{
                  const choice=document.createElement('button');choice.setAttribute('role','menuitem');choice.innerText='Pro';
                  choice.onclick=()=>{document.querySelector('[data-testid="model-switcher-dropdown-button"]').innerText='Pro';choice.remove()};document.body.append(choice);
                };true"#).await?;
			crate::page::select_pro(&mut second).await?;
			let store = Store::open(directory.path())?;
			let record = store.prepare(crate::Request { id:"unicode-input".into(), prompt:"研究资料🧪".repeat(20_000), previous:None })?;
			let material = store.material(&record)?;
			crate::page::fill(&mut second, &material).await?;
			second.evaluate(r#"document.querySelector('[data-testid="send-button"]').onclick=()=>{globalThis.sends=(globalThis.sends??0)+1};true"#).await?;
			crate::page::send(&mut second).await?;
			if second.evaluate("globalThis.sends").await? != 1 { return Err("Send was not dispatched exactly once.".into()); }
			second.evaluate("document.title='Just a moment...';document.body.innerHTML='<h1>Performing security verification</h1><p>Cloudflare</p>';true").await?;
			let challenged = crate::page::connection(&mut second, Duration::ZERO).await?;
			if challenged.state != crate::ConnectionState::VerificationRequired {
				return Err("Provider verification was reported as login readiness.".into());
			}
			if crate::page::ready(&mut second).await.is_ok() || second.evaluate("globalThis.sends").await? != 1 {
				return Err("A challenged page must remain blocked without another Send.".into());
			}
			// A vanished target without a saved conversation must not open a blank
			// replacement that could later send the request a second time.
			let count = second.command("Target.getTargets", json!({})).await?["targetInfos"].as_array().unwrap().len();
			if second.open(Some("missing-target"), None, false).await.is_ok() { return Err("Lost submission target was replaced.".into()); }
			let after = second.command("Target.getTargets", json!({})).await?["targetInfos"].as_array().unwrap().len();
			if count != after { return Err("Recovery opened an unrelated tab.".into()); }
			Ok(())
		}.await;
		second.close_browser().await.unwrap();
		sleep(Duration::from_millis(300)).await;
		outcome.expect("real Chrome input, ownership and reconnect");
	}
}
