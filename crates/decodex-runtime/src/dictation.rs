//! One ephemeral subscription dictation session. No audio, token or draft enters SQLite.
use decodex_codex::app_server_client::AppServerClient;
use decodex_protocol::{
	DictationBuffer, DictationPhase, DictationRequest, DictationStatus, EntityId, WireText,
};
use serde_json::json;
use std::{
	sync::Arc,
	time::{Duration, Instant},
};
use tokio::sync::Mutex;

#[path = "dictation_native.rs"] mod native;

struct Session {
	status: DictationStatus,
	transport: Option<native::Stream>,
	seen: Instant,
}
#[derive(Clone, Default)]
pub(crate) struct DictationGateway(Arc<Mutex<Option<Session>>>);
impl DictationGateway {
	pub(crate) async fn exchange(
		&self,
		request: &DictationRequest,
		client: Option<AppServerClient>,
	) -> DictationStatus {
		let id = request.session_id().clone();
		let mut slot = self.0.lock().await;
		if let DictationRequest::Start { .. } = request {
			if let Some(session) = slot.as_mut() {
				if session.status.session_id == id {
					session.seen = Instant::now();
					return session.status.clone();
				}
				if session.transport.is_some() {
					return failed(id, "Finish or cancel the current dictation first.");
				}
			}
			let Some(client) = client else {
				return failed(
					id,
					"The account is reconnecting. Try dictation when the Chief is ready.",
				);
			};
			// Native authentication stays in this service and its same-process URLSession adapter.
			let Ok(mut auth) = client
				.request("getAuthStatus", json!({"includeToken":true,"refreshToken":false}))
				.await
			else {
				return failed(id, "The native account connection could not authorize dictation.");
			};
			if !matches!(auth["authMethod"].as_str(), Some("chatgpt" | "chatgptAuthTokens")) {
				return failed(id, "Dictation requires a ChatGPT subscription connection.");
			}
			let token = auth.get_mut("authToken").map(serde_json::Value::take);
			let Some(serde_json::Value::String(token)) = token else {
				return failed(id, "Sign in with a ChatGPT subscription to use dictation.");
			};
			let Ok(transport) = native::Stream::new(&token) else {
				return failed(id, "Dictation requires the current signed macOS application.");
			};
			let status = DictationStatus {
				session_id: id,
				phase: DictationPhase::Connecting,
				text: DictationBuffer::new("").expect("empty draft"),
				message: None,
			};
			*slot = Some(Session {
				status: status.clone(),
				transport: Some(transport),
				seen: Instant::now(),
			});
			return status;
		}
		let Some(session) = slot.as_mut().filter(|s| s.status.session_id == id) else {
			return failed(id, "This dictation session ended. Audio was not replayed.");
		};
		session.seen = Instant::now();
		session.poll();
		match request {
			DictationRequest::Audio { audio, .. }
				if session.status.phase == DictationPhase::Listening =>
			{
				if !session
					.transport
					.as_mut()
					.is_some_and(|s| s.command(json!({"operation":"audio","audio":audio.as_str()})))
				{
					session.fail(
						"Audio could not be delivered. Your received text remains in the draft.",
					);
				}
			},
			DictationRequest::Audio { .. } =>
				if session.transport.is_some() {
					session
						.fail("Audio arrived before dictation was ready. Start a new recording.");
				},
			DictationRequest::Finish { .. } => {
				if matches!(
					session.status.phase,
					DictationPhase::Connecting | DictationPhase::Listening
				) {
					if session
						.transport
						.as_mut()
						.is_some_and(|s| s.command(json!({"operation":"finish"})))
					{
						session.status.phase = DictationPhase::Finalizing;
					} else {
						session.fail(
							"Dictation could not request final correction. Your received text remains in the draft.",
						);
					}
				}
			},
			DictationRequest::Cancel { .. } => {
				session.transport = None;
				session.status.phase = DictationPhase::Complete;
			},
			_ => {},
		}
		session.status.clone()
	}

	pub(crate) async fn expire(&self) {
		if let Some(session) = self.0.lock().await.as_mut() {
			session.poll();
			if session.transport.is_some() && session.seen.elapsed() > Duration::from_secs(15) {
				session.fail("Dictation stopped after its window disconnected.");
			}
		}
	}

	pub(crate) async fn active(&self) -> bool {
		self.0.lock().await.as_ref().is_some_and(|s| s.transport.is_some())
	}
}
impl Session {
	fn poll(&mut self) {
		for _ in 0..64 {
			let Some(event) = self.transport.as_mut().and_then(native::Stream::poll) else { break };
			match event["kind"].as_str() {
				Some("ready") if self.status.phase == DictationPhase::Connecting =>
					self.status.phase = DictationPhase::Listening,
				Some("transcript" | "complete") => {
					if let Some(text) =
						event["text"].as_str().and_then(|s| DictationBuffer::new(s).ok())
					{
						self.status.text = text;
					}
					if event["kind"] == "complete" {
						self.status.phase = DictationPhase::Complete;
						self.transport = None;
					}
				},
				Some("error") =>
					self.fail(event["message"].as_str().unwrap_or("Dictation disconnected.")),
				_ => {},
			}
		}
	}

	fn fail(&mut self, message: &str) {
		self.transport = None;
		self.status.phase = DictationPhase::Failed;
		self.status.message = WireText::new(message).ok();
	}
}
pub(crate) fn failed(session_id: EntityId, message: &str) -> DictationStatus {
	DictationStatus {
		session_id,
		phase: DictationPhase::Failed,
		text: DictationBuffer::new("").expect("empty draft"),
		message: WireText::new(message).ok(),
	}
}
