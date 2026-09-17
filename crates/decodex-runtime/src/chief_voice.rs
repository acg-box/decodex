//! Memory-only voice signaling mailbox. The Chief actor owns every provider operation.
use decodex_protocol::{
	ChiefVoicePhase, ChiefVoiceRequest, ChiefVoiceStatus, EntityId, VoiceSdp, WireText,
};
use std::{
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};
use tokio::sync::mpsc;

struct Call {
	signature: (EntityId, [u8; 32]),
	status: ChiefVoiceStatus,
	seen: Instant,
	stopping: bool,
}
#[derive(Clone)]
pub(crate) struct VoiceGateway {
	call: Arc<Mutex<Option<Call>>>,
	sender: mpsc::Sender<ChiefVoiceRequest>,
	receiver: Arc<tokio::sync::Mutex<Option<mpsc::Receiver<ChiefVoiceRequest>>>>,
}
impl VoiceGateway {
	pub(crate) fn new() -> Self {
		let (sender, receiver) = mpsc::channel(8);
		Self {
			call: Arc::new(Mutex::new(None)),
			sender,
			receiver: Arc::new(tokio::sync::Mutex::new(Some(receiver))),
		}
	}

	pub(crate) async fn take_receiver(&self) -> Option<mpsc::Receiver<ChiefVoiceRequest>> {
		self.receiver.lock().await.take()
	}

	pub(crate) fn exchange(&self, request: &ChiefVoiceRequest) -> ChiefVoiceStatus {
		let unavailable = || failed(request.session_id().clone(), "Voice service is unavailable.");
		let Ok(mut slot) = self.call.lock() else { return unavailable() };
		match request {
			ChiefVoiceRequest::Start { session_id, work_id, offer } => {
				use sha2::{Digest as _, Sha256};
				let signature = (work_id.clone(), Sha256::digest(offer.as_str().as_bytes()).into());
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
						ChiefVoicePhase::Ended | ChiefVoicePhase::Failed
					) {
						return failed(session_id.clone(), "End the current voice call first.");
					}
				}
				if self.sender.try_send(request.clone()).is_err() {
					return unavailable();
				}
				let status = ChiefVoiceStatus {
					session_id: session_id.clone(),
					phase: ChiefVoicePhase::Connecting,
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
			ChiefVoiceRequest::Poll { session_id } | ChiefVoiceRequest::Stop { session_id } => {
				let Some(call) = slot.as_mut().filter(|c| c.status.session_id == *session_id)
				else {
					return failed(
						session_id.clone(),
						"This voice session is no longer available. Its input was not replayed.",
					);
				};
				call.seen = Instant::now();
				if matches!(request, ChiefVoiceRequest::Stop { .. }) && !call.stopping {
					if self.sender.try_send(request.clone()).is_err() {
						return unavailable();
					}
					call.stopping = true;
				}
				call.status.clone()
			},
		}
	}

	pub(crate) fn update(
		&self,
		id: &str,
		phase: ChiefVoicePhase,
		answer: Option<VoiceSdp>,
		message: Option<&str>,
	) {
		if let Ok(mut slot) = self.call.lock()
			&& let Some(call) = slot.as_mut().filter(|c| c.status.session_id.as_str() == id)
		{
			call.status.phase = phase;
			call.status.answer = answer;
			call.status.message = message.and_then(|v| WireText::new(v).ok());
		}
	}

	pub(crate) fn expire(&self) -> Option<ChiefVoiceRequest> {
		let mut slot = self.call.lock().ok()?;
		let call = slot.as_mut()?;
		if call.stopping
			|| matches!(call.status.phase, ChiefVoicePhase::Ended)
			|| call.seen.elapsed() < Duration::from_secs(15)
		{
			return None;
		}
		call.stopping = true;
		Some(ChiefVoiceRequest::Stop { session_id: call.status.session_id.clone() })
	}
}
pub(crate) fn failed(session_id: EntityId, message: &str) -> ChiefVoiceStatus {
	ChiefVoiceStatus {
		session_id,
		phase: ChiefVoicePhase::Failed,
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
	use super::*;
	#[tokio::test]
	async fn lost_start_response_is_observed_without_replaying_a_call() {
		let gateway = VoiceGateway::new();
		let mut commands = gateway.take_receiver().await.unwrap();
		let id = EntityId::new("call-one").unwrap();
		let request = ChiefVoiceRequest::Start {
			session_id: id.clone(),
			work_id: EntityId::new("chief").unwrap(),
			offer: VoiceSdp::new("offer".into()).unwrap(),
		};
		assert_eq!(gateway.exchange(&request).phase, ChiefVoicePhase::Connecting);
		assert!(commands.recv().await.is_some());
		assert_eq!(gateway.exchange(&request).phase, ChiefVoicePhase::Connecting);
		assert!(commands.try_recv().is_err());
		gateway.exchange(&ChiefVoiceRequest::Stop { session_id: id.clone() });
		gateway.exchange(&ChiefVoiceRequest::Stop { session_id: id });
		assert!(commands.try_recv().is_ok());
		assert!(commands.try_recv().is_err());
	}
}
