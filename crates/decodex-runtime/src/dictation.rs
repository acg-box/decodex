//! One ephemeral subscription dictation session. No audio, token or draft enters SQLite.
#[path = "dictation_native.rs"] mod native;
#[path = "dictation_transcript.rs"] mod transcript;

use std::{
	sync::Arc,
	time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{self, Value};
use tokio::{sync::Mutex, time};

use decodex_codex::app_server_client::AppServerClient;
use decodex_protocol::{
	DictationBuffer, DictationPhase, DictationRequest, DictationStatus, EntityId, WireText,
};
use transcript::Transcript;

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
					"The account is reconnecting. Try dictation when the Agent is ready.",
				);
			};
			// Native authentication stays in this service and its same-process URLSession adapter.
			// Release the session lock before the caller's five-second query deadline.
			let Ok(Ok(mut auth)) = time::timeout(
				Duration::from_secs(4),
				client.request(
					"getAuthStatus",
					serde_json::json!({"includeToken":true,"refreshToken":false}),
				),
			)
			.await
			else {
				return failed(id, "The native account connection could not authorize dictation.");
			};

			if !matches!(auth["authMethod"].as_str(), Some("chatgpt" | "chatgptAuthTokens")) {
				return failed(id, "Dictation requires a ChatGPT subscription connection.");
			}

			let token = auth.get_mut("authToken").map(Value::take);
			let Some(Value::String(token)) = token else {
				return failed(id, "Sign in with a ChatGPT subscription to use dictation.");
			};
			let Ok(transport) = native::Stream::new(&token, start_message()) else {
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
				transcript: Default::default(),
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
				if !valid_audio(audio.as_str())
					|| !session.transport.as_mut().is_some_and(|s| {
						s.command(serde_json::json!({"type":"audio.append","audio":audio.as_str()}))
					}) {
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
						.is_some_and(|s| s.command(serde_json::json!({"type":"session.close"})))
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

struct Session {
	status: DictationStatus,
	transport: Option<native::Stream>,
	seen: Instant,
	transcript: Transcript,
}
impl Session {
	fn poll(&mut self) {
		for _ in 0..64 {
			let Some(event) = self.transport.as_mut().and_then(native::Stream::poll) else { break };

			self.receive(event);
		}
	}

	fn receive(&mut self, event: Value) {
		if event["kind"] == "error" {
			self.fail(event["message"].as_str().unwrap_or("Dictation disconnected."));

			return;
		}

		let Some(mut message) =
			event["message"].as_str().and_then(|value| serde_json::from_str::<Value>(value).ok())
		else {
			self.fail("Invalid dictation response. Your received text remains in the draft.");

			return;
		};

		match message["type"].as_str() {
			Some("session.started") if self.status.phase == DictationPhase::Connecting =>
				self.status.phase = DictationPhase::Listening,
			Some("transcript.segment" | "transcript.final") => {
				let finalized = message["type"] == "transcript.final";

				message["finalized"] = finalized.into();
				message["id"] = message["utterance_id"].take();

				match self.transcript.apply(message) {
					Ok(text) => self.status.text = text,
					Err(()) => self.fail(
						"Invalid or oversized dictation transcript. Your received text remains in the draft.",
					),
				}
			},
			Some("session.closed") => self.complete(),
			Some("session.updated") if message["session"]["status"] == "closed" => self.complete(),
			Some("session.error" | "error") =>
				self.fail("The subscription dictation service could not finish this recording."),
			Some(_) => {},
			None =>
				self.fail("Invalid dictation response. Your received text remains in the draft."),
		}
	}

	fn complete(&mut self) {
		self.status.phase = DictationPhase::Complete;
		self.transport = None;
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

fn start_message() -> Value {
	serde_json::json!({"type":"session.start", "config":{
		"input_audio_format":"pcm16", "sample_rate_hz":24_000, "num_channels":1,
		"max_buffer_size_bytes":4_194_304, "max_utterance_duration_ms":30_000,
		"session_ttl_ms":300_000, "provider_mode":"streaming_sse", "transcript_delivery_mode":"segment",
		"vad":{"type":"server_vad", "threshold":0.5, "prefix_padding_ms":300, "silence_duration_ms":500}
	}})
}

fn valid_audio(audio: &str) -> bool {
	STANDARD
		.decode(audio)
		.is_ok_and(|pcm| !pcm.is_empty() && pcm.len() <= 32_768 && pcm.len().is_multiple_of(2))
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use tokio::{sync::mpsc, time};

	use crate::dictation::{DictationGateway, DictationPhase, DictationRequest};
	use decodex_codex::app_server_client::AppServerClient;

	#[test]
	fn subscription_frames_update_one_draft_and_preserve_it_on_disconnect() {
		let mut session = super::Session {
			status: super::failed(decodex_protocol::EntityId::new("test").unwrap(), ""),
			transport: None,
			seen: std::time::Instant::now(),
			transcript: Default::default(),
		};

		session.status.phase = DictationPhase::Connecting;
		session.status.message = None;

		let frame = |value: serde_json::Value| serde_json::json!({"kind":"message","message":value.to_string()});

		session.receive(frame(serde_json::json!({"type":"session.started"})));

		assert_eq!(session.status.phase, DictationPhase::Listening);

		session.receive(frame(
			serde_json::json!({"type":"transcript.segment","utterance_id":"a","revision":1,"text":"draft"}),
		));
		session.receive(frame(
			serde_json::json!({"type":"transcript.final","utterance_id":"a","revision":2,"text":"Final draft."}),
		));

		assert_eq!(session.status.text.as_str(), "Final draft.");

		session.receive(frame(
			serde_json::json!({"type":"session.updated","session":{"status":"closed"}}),
		));

		assert_eq!(session.status.phase, DictationPhase::Complete);
		assert_eq!(session.status.text.as_str(), "Final draft.");

		session.status.phase = DictationPhase::Listening;

		session.receive(serde_json::json!({"kind":"message","message":"invalid JSON"}));

		assert_eq!(session.status.phase, DictationPhase::Failed);
		assert_eq!(session.status.text.as_str(), "Final draft.");
	}

	#[test]
	fn pcm_validation_rejects_invalid_or_partial_samples() {
		use base64::Engine as _;

		for audio in ["", "not base64", "AQ=="] {
			assert!(!super::valid_audio(audio));
		}

		assert!(super::valid_audio("AQI="));
		assert!(super::valid_audio(&super::STANDARD.encode(vec![0; 32_768])));
		assert!(!super::valid_audio(&super::STANDARD.encode(vec![0; 32_770])));
	}

	#[tokio::test]
	async fn stalled_authorization_releases_gateway_without_replay() {
		let (_incoming, frames) = mpsc::channel(8);
		let (outgoing, mut writes) = mpsc::channel(8);
		let (client, _events) = AppServerClient::from_framed(1, frames, outgoing).unwrap();
		let gateway = DictationGateway::default();
		let owner = gateway.clone();
		let session_id = decodex_protocol::EntityId::new("dictation-timeout").unwrap();
		let request = DictationRequest::Start { session_id };
		let start = tokio::spawn(async move { owner.exchange(&request, Some(client)).await });
		let frame = time::timeout(Duration::from_secs(1), writes.recv()).await.unwrap().unwrap();

		assert_eq!(frame["method"], "getAuthStatus");
		assert_eq!(frame["params"], serde_json::json!({"includeToken":true,"refreshToken":false}));

		let status = time::timeout(Duration::from_secs(5), start)
			.await
			.expect("authorization must finish before the client deadline")
			.unwrap();

		assert_eq!(status.phase, DictationPhase::Failed);
		assert!(status.text.as_str().is_empty());
		assert!(status.message.is_some());

		time::timeout(Duration::from_secs(1), gateway.expire()).await.unwrap();

		assert!(!time::timeout(Duration::from_secs(1), gateway.active()).await.unwrap());

		let poll =
			gateway.exchange(&DictationRequest::Poll { session_id: status.session_id }, None).await;

		assert_eq!(poll.phase, DictationPhase::Failed);
		assert!(writes.try_recv().is_err(), "authorization must not be replayed");
	}
}
