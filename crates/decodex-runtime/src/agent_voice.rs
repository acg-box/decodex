//! Memory-only voice signaling mailbox. The Agent actor owns every provider operation.
use std::{
	sync::Arc,
	time::{Duration, Instant},
};

use tokio::sync::mpsc::{self, Receiver, Sender};

use decodex_protocol::{
	AgentVoicePhase, AgentVoiceRequest, AgentVoiceStatus, EntityId, VoiceSdp, WireText,
};

#[derive(Clone)]
pub(crate) struct VoiceGateway {
	call: Arc<std::sync::Mutex<Option<Call>>>,
	sender: Sender<AgentVoiceRequest>,
	receiver: Arc<tokio::sync::Mutex<Option<Receiver<AgentVoiceRequest>>>>,
}
impl VoiceGateway {
	pub(crate) fn new() -> Self {
		let (sender, receiver) = mpsc::channel(8);

		Self {
			call: Arc::new(std::sync::Mutex::new(None)),
			sender,
			receiver: Arc::new(tokio::sync::Mutex::new(Some(receiver))),
		}
	}

	pub(crate) async fn take_receiver(&self) -> Option<Receiver<AgentVoiceRequest>> {
		self.receiver.lock().await.take()
	}

	pub(crate) fn exchange(&self, request: &AgentVoiceRequest) -> AgentVoiceStatus {
		let unavailable = || failed(request.session_id().clone(), "Voice service is unavailable.");
		let Ok(mut slot) = self.call.lock() else { return unavailable() };

		match request {
			AgentVoiceRequest::Start { session_id, work_id, offer, options } => {
				use sha2::{Digest as _, Sha256};

				let signature = (
					work_id.clone(),
					Sha256::digest(
						serde_json::to_vec(&(offer, options)).expect("serializable call settings"),
					)
					.into(),
				);

				if let Some(call) = slot.as_mut() {
					if call.status.session_id == *session_id {
						return if call.signature == signature {
							call.status.clone()
						} else {
							failed(session_id.clone(), "This call identity was already used.")
						};
					}
					if !matches!(
						call.status.phase,
						AgentVoicePhase::Ended | AgentVoicePhase::Failed
					) {
						return failed(session_id.clone(), "End the current voice call first.");
					}
				}

				if self.sender.try_send(request.clone()).is_err() {
					return unavailable();
				}

				let status = AgentVoiceStatus {
					session_id: session_id.clone(),
					phase: AgentVoicePhase::Connecting,
					answer: None,
					message: None,
				};

				*slot = Some(Call {
					signature,
					status: status.clone(),
					seen: Instant::now(),
					stopping: false,
				});

				status
			},
			AgentVoiceRequest::Speak { session_id, text } => {
				let Some(call) = slot.as_mut().filter(|c| c.status.session_id == *session_id)
				else {
					return failed(
						session_id.clone(),
						"This voice session is no longer available.",
					);
				};
				let mut status = call.status.clone();
				let message = if call.stopping || status.phase != AgentVoicePhase::Ready {
					"Read-aloud requires a connected voice call."
				} else if text.as_str().trim().is_empty() {
					"There is no reply text to read aloud."
				} else if self.sender.try_send(request.clone()).is_err() {
					"Read-aloud could not be queued. It was not retried."
				} else {
					call.seen = Instant::now();
					"Read-aloud request queued."
				};

				status.message = WireText::new(message).ok();

				status
			},
			AgentVoiceRequest::Poll { session_id } | AgentVoiceRequest::Stop { session_id } => {
				let Some(call) = slot.as_mut().filter(|c| c.status.session_id == *session_id)
				else {
					return failed(
						session_id.clone(),
						"This voice session is no longer available. Its input was not replayed.",
					);
				};

				call.seen = Instant::now();

				if matches!(request, AgentVoiceRequest::Stop { .. }) && !call.stopping {
					if self.sender.try_send(request.clone()).is_err() {
						return unavailable();
					}

					call.stopping = true;
				}

				let status = call.status.clone();

				if matches!(request, AgentVoiceRequest::Poll { .. })
					&& status.phase == AgentVoicePhase::Ready
				{
					call.status.message = None;
				}

				status
			},
		}
	}

	pub(crate) fn update(
		&self,
		id: &str,
		phase: AgentVoicePhase,
		answer: Option<VoiceSdp>,
		message: Option<&str>,
	) {
		if let Ok(mut slot) = self.call.lock()
			&& let Some(call) = slot.as_mut().filter(|c| c.status.session_id.as_str() == id)
		{
			// Cleanup acknowledgement must not erase a failure before the client polls.
			if phase == AgentVoicePhase::Ended {
				call.stopping = true;

				if call.status.phase == AgentVoicePhase::Failed {
					call.status.answer = None;

					return;
				}
			}

			call.status.phase = phase;
			call.status.answer = answer;
			call.status.message = message.and_then(|v| WireText::new(v).ok());
		}
	}

	/// Report a one-shot speech result without changing media connection state.
	pub(crate) fn notice(&self, id: &str, message: &str) {
		if let Ok(mut slot) = self.call.lock()
			&& let Some(call) = slot.as_mut().filter(|c| {
				c.status.session_id.as_str() == id && c.status.phase == AgentVoicePhase::Ready
			}) {
			call.status.message = WireText::new(message).ok();
		}
	}

	pub(crate) fn expire(&self) -> Option<AgentVoiceRequest> {
		let mut slot = self.call.lock().ok()?;
		let call = slot.as_mut()?;

		if call.stopping
			|| matches!(call.status.phase, AgentVoicePhase::Ended)
			|| call.seen.elapsed() < Duration::from_secs(15)
		{
			return None;
		}

		call.stopping = true;

		Some(AgentVoiceRequest::Stop { session_id: call.status.session_id.clone() })
	}
}

struct Call {
	signature: (EntityId, [u8; 32]),
	status: AgentVoiceStatus,
	seen: Instant,
	stopping: bool,
}

pub(crate) fn failed(session_id: EntityId, message: &str) -> AgentVoiceStatus {
	AgentVoiceStatus {
		session_id,
		phase: AgentVoicePhase::Failed,
		answer: None,
		message: WireText::new(message).ok(),
	}
}

/// Use fixed, actionable categories; never display raw provider payloads.
pub(crate) fn provider_error_message(message: &str) -> &'static str {
	let message = message.to_ascii_lowercase();

	if message.contains("429") || message.contains("quota") {
		"Voice reached the account limit. End the call before changing accounts."
	} else if message.contains("401") || message.contains("authentication") {
		"Voice authentication failed. Check the account connection in Settings."
	} else if message.contains("403") {
		"The voice connection was refused. Your subscription eligibility is not determined by this error."
	} else if message.contains("sdp") || message.contains("webrtc") {
		"The audio connection could not be negotiated. Start a new call."
	} else {
		"Voice disconnected. Start a new call; spoken input will not be replayed."
	}
}

#[cfg(test)]
mod tests {
	use crate::agent_voice::{
		AgentVoicePhase, AgentVoiceRequest, EntityId, VoiceGateway, VoiceSdp, WireText,
	};
	#[tokio::test]
	async fn failure_survives_cleanup_before_poll_without_another_stop() {
		let gateway = VoiceGateway::new();
		let mut commands = gateway.take_receiver().await.unwrap();
		let id = EntityId::new("failed-call").unwrap();

		gateway.exchange(&AgentVoiceRequest::Start {
			session_id: id.clone(),
			work_id: EntityId::new("agent").unwrap(),
			offer: VoiceSdp::new("offer".into()).unwrap(),
			options: Default::default(),
		});
		commands.recv().await.unwrap();
		gateway.update(
			id.as_str(),
			AgentVoicePhase::Failed,
			None,
			Some("Audio connection failed."),
		);
		gateway.update(id.as_str(), AgentVoicePhase::Ended, None, None);

		let result = gateway.exchange(&AgentVoiceRequest::Poll { session_id: id.clone() });

		assert_eq!(result.phase, AgentVoicePhase::Failed);
		assert_eq!(result.message.unwrap().as_str(), "Audio connection failed.");

		gateway.exchange(&AgentVoiceRequest::Stop { session_id: id });

		assert!(commands.try_recv().is_err());
		assert!(gateway.expire().is_none());
	}

	#[tokio::test]
	async fn selected_speech_is_one_shot_and_notices_do_not_end_media() {
		let gateway = VoiceGateway::new();
		let mut commands = gateway.take_receiver().await.unwrap();
		let id = EntityId::new("call").unwrap();

		gateway.exchange(&AgentVoiceRequest::Start {
			session_id: id.clone(),
			work_id: EntityId::new("agent").unwrap(),
			offer: VoiceSdp::new("offer".into()).unwrap(),
			options: Default::default(),
		});
		commands.recv().await.unwrap();

		let speak = AgentVoiceRequest::Speak {
			session_id: id.clone(),
			text: decodex_protocol::HistoryText::new("Reply").unwrap(),
		};

		gateway.exchange(&speak);

		assert!(commands.try_recv().is_err(), "connecting cannot speak");

		gateway.update(
			"call",
			AgentVoicePhase::Ready,
			Some(VoiceSdp::new("answer".into()).unwrap()),
			None,
		);
		gateway.exchange(&AgentVoiceRequest::Speak {
			session_id: EntityId::new("other").unwrap(),
			text: decodex_protocol::HistoryText::new("Foreign").unwrap(),
		});

		assert!(commands.try_recv().is_err(), "foreign call cannot speak");
		assert_eq!(gateway.exchange(&speak).phase, AgentVoicePhase::Ready);
		assert_eq!(commands.recv().await.unwrap(), speak);

		gateway.notice("call", "Read-aloud could not be confirmed. It was not retried.");

		let poll = AgentVoiceRequest::Poll { session_id: id.clone() };
		let status = gateway.exchange(&poll);

		assert_eq!(status.phase, AgentVoicePhase::Ready);
		assert!(status.answer.is_some());
		assert!(status.message.unwrap().as_str().contains("not retried"));
		assert!(gateway.exchange(&poll).message.is_none());
		assert!(commands.try_recv().is_err(), "poll never replays speech");

		gateway.exchange(&AgentVoiceRequest::Stop { session_id: id });
		commands.recv().await.unwrap();
		gateway.exchange(&speak);

		assert!(commands.try_recv().is_err(), "stopping cannot speak");
	}

	#[tokio::test]
	async fn lost_start_response_is_observed_without_replaying_a_call() {
		let gateway = VoiceGateway::new();
		let mut commands = gateway.take_receiver().await.unwrap();
		let id = EntityId::new("call-one").unwrap();
		let request = AgentVoiceRequest::Start {
			session_id: id.clone(),
			work_id: EntityId::new("agent").unwrap(),
			offer: VoiceSdp::new("offer".into()).unwrap(),
			options: Default::default(),
		};

		assert_eq!(gateway.exchange(&request).phase, AgentVoicePhase::Connecting);
		assert!(commands.recv().await.is_some());
		assert_eq!(gateway.exchange(&request).phase, AgentVoicePhase::Connecting);
		assert!(commands.try_recv().is_err());

		let mut changed = request.clone();

		if let AgentVoiceRequest::Start { options, .. } = &mut changed {
			options.model = Some(WireText::new("different-model").unwrap());
		}

		assert_eq!(gateway.exchange(&changed).phase, AgentVoicePhase::Failed);
		assert!(commands.try_recv().is_err(), "changed call options cannot replay signaling");

		gateway.exchange(&AgentVoiceRequest::Stop { session_id: id.clone() });
		gateway.exchange(&AgentVoiceRequest::Stop { session_id: id });

		assert!(commands.try_recv().is_ok());
		assert!(commands.try_recv().is_err());
	}
}
