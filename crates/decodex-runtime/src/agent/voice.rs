//! Bind native live voice to the existing Agent and observe its real task turns.
use super::{
	AgentCoordinator, AgentError, ClientError, ServerEvent, Value, exact, json, resume_error,
};
use crate::agent_voice::VoiceGateway;
use decodex_protocol::{AgentVoicePhase, AgentVoiceRequest, VoiceSdp};

pub(super) struct VoiceConnection {
	generation: String,
	transcript_sequence: u64,
	answer_seen: bool,
	precaution_retired: bool,
	transcript_tail: [String; 2],
	transcript_complete: [bool; 2],
	gateway: VoiceGateway,
	session: Option<(String, String)>,
}
impl VoiceConnection {
	async fn save_transcript_tail(
		&mut self,
		store: &decodex_database::SqliteStore,
		id: &str,
		index: usize,
	) -> Result<(), AgentError> {
		if self.transcript_tail[index].is_empty() {
			self.transcript_complete[index] = false;
			return Ok(());
		}
		let sequence = self.transcript_sequence + 1;
		store
			.record_agent_voice_transcript(
				id.into(),
				sequence,
				["user", "assistant"][index].into(),
				self.transcript_tail[index].clone(),
				self.transcript_complete[index],
			)
			.await?;
		self.transcript_sequence = sequence;
		self.transcript_tail[index].clear();
		self.transcript_complete[index] = false;
		Ok(())
	}

	async fn save_transcript_tails(
		&mut self,
		store: &decodex_database::SqliteStore,
		id: &str,
	) -> Result<(), AgentError> {
		for index in 0..2 {
			self.save_transcript_tail(store, id, index).await?;
		}
		Ok(())
	}
}

impl AgentCoordinator {
	pub(super) async fn stop_voice_for_precaution(
		&mut self,
		thread: &str,
	) -> Result<(), AgentError> {
		let Some(voice) = self.voice.as_mut() else {
			return Ok(());
		};
		let Some((id, active_thread)) =
			voice.session.as_ref().filter(|(_, active)| active == thread).cloned()
		else {
			return Ok(());
		};
		voice.precaution_retired = true;
		voice.gateway.update(
			&id,
			AgentVoicePhase::Failed,
			None,
			Some("Conversation paused. Review the provider findings before continuing."),
		);
		// Retire local microphone authority even if native stop acknowledgment is lost.
		let persisted = self.store.retire_agent_misalignment_voice(thread.into()).await;
		let result =
			self.client.request("thread/realtime/stop", json!({"threadId":active_thread})).await;
		// Retain the session until the durable cause and transcript tails are saved.
		voice.save_transcript_tails(&self.store, &id).await?;
		persisted?;
		if result.is_ok() {
			self.store.close_agent_voice_call(id).await?;
			voice.session = None;
		}
		Ok(())
	}

	pub(crate) fn attach_voice_host(&mut self, generation: String, gateway: VoiceGateway) {
		self.voice = Some(VoiceConnection {
			generation,
			transcript_sequence: 0,
			answer_seen: false,
			precaution_retired: false,
			transcript_tail: Default::default(),
			transcript_complete: Default::default(),
			gateway,
			session: None,
		});
	}

	pub(crate) async fn voice_request(
		&mut self,
		request: AgentVoiceRequest,
	) -> Result<(), AgentError> {
		match request {
			AgentVoiceRequest::Start { session_id, work_id, offer, options } => {
				if self.store.agent_misalignment(work_id.as_str().into()).await?.is_some() {
					return Err(AgentError::Invalid(
						"This conversation is paused for provider findings.".into(),
					));
				}
				if !self.is_manager(work_id.as_str()).await? {
					return Err(AgentError::Invalid("voice requires a Agent".into()));
				}
				let item = self.store.get_agent_work_item(work_id.as_str().into()).await?;
				if !matches!(
					item.dispatch_state,
					decodex_database::AgentDispatchState::Idle
						| decodex_database::AgentDispatchState::Running
				) {
					return Err(AgentError::Busy);
				}
				let thread = item
					.codex_thread_id
					.ok_or_else(|| AgentError::Invalid("Agent thread is not ready".into()))?;
				let generation = self
					.voice
					.as_ref()
					.ok_or_else(|| AgentError::Invalid("voice host unavailable".into()))?
					.generation
					.clone();
				if !self.loaded_threads.contains(&thread) {
					let mut params = Self::resume_params(&thread);
					params["config"]["features.realtime_conversation"] = json!(true);
					let resumed = self
						.client
						.thread_resume(params)
						.await
						.map_err(|error| resume_error(error, &thread))?;
					if exact(&resumed, "/thread/id")? != thread {
						return Err(AgentError::Invalid("voice thread differs".into()));
					}
					self.loaded_threads.insert(thread.clone());
				}
				let selected_voice = self.client.realtime_voice_for_thread(&thread).await?;
				let baseline = self.client.thread_latest_turn_id(&thread).await?;
				self.store
					.begin_agent_voice_call(decodex_database::AgentVoiceCall {
						session_id: session_id.as_str().into(),
						work_id: work_id.as_str().into(),
						thread_id: thread.clone(),
						generation_id: generation,
						baseline_turn_id: baseline,
					})
					.await?;
				self.voice.as_mut().expect("voice host").answer_seen = false;
				self.voice.as_mut().expect("voice host").precaution_retired = false;
				self.voice.as_mut().expect("voice host").transcript_tail = Default::default();
				self.voice.as_mut().expect("voice host").transcript_complete = Default::default();
				self.voice.as_mut().expect("voice host").session =
					Some((session_id.as_str().into(), thread.clone()));
				let mut params = json!({
					"threadId":thread,"version":"v3","outputModality":"audio","voice":selected_voice,
					"includeStartupContext":true,"flushTranscriptTailOnSessionEnd":true,
					"prompt":"Continue this Agent conversation by voice. Wait for the user's new spoken request before starting new work. Use the existing conversation and its tools when the user asks for work.",
					"transport":{"type":"webrtc","sdp":offer.as_str()}
				});
				if let Some(model) = options.model {
					params["model"] = json!(model.as_str());
				}
				if let Some(instructions) = options.start_instructions {
					params["realtimeStartInstructions"] = json!(instructions.as_str());
				}
				if let Some(instructions) = options.end_instructions {
					params["realtimeEndInstructions"] = json!(instructions.as_str());
				}
				let result = self.client.request("thread/realtime/start", params).await;
				if matches!(&result, Err(ClientError::Remote(_))) {
					self.store.close_agent_voice_call(session_id.as_str().into()).await?;
					self.voice.as_mut().expect("voice host").session = None;
				}
				result?;
			},
			AgentVoiceRequest::Speak { session_id, text } => {
				let voice = self.voice.as_ref().filter(|v| v.answer_seen && !v.precaution_retired);
				let Some((voice, thread)) = voice.and_then(|voice| {
					voice
						.session
						.as_ref()
						.filter(|(id, _)| id == session_id.as_str())
						.map(|(_, thread)| (voice, thread))
				}) else {
					return Err(AgentError::Invalid(
						"Read-aloud call is no longer available.".into(),
					));
				};
				if text.as_str().trim().is_empty() {
					return Err(AgentError::Invalid("Read-aloud text is empty.".into()));
				}
				self.client
					.request(
						"thread/realtime/appendSpeech",
						json!({
							"threadId":thread, "text":text.as_str()
						}),
					)
					.await?;
				voice.gateway.notice(session_id.as_str(), "Read-aloud request sent.");
			},
			AgentVoiceRequest::Stop { session_id } => {
				let session = self
					.voice
					.as_ref()
					.and_then(|v| v.session.as_ref())
					.filter(|(id, _)| id == session_id.as_str());
				if let Some((_, thread)) = session {
					self.client.request("thread/realtime/stop", json!({"threadId":thread})).await?;
				}
			},
			AgentVoiceRequest::Poll { .. } => {},
		}
		Ok(())
	}

	pub(super) async fn voice_event(&mut self, event: &ServerEvent) -> Result<(), AgentError> {
		let Some(voice) = self.voice.as_mut() else { return Ok(()) };
		let ServerEvent::Notification { method, params } = event else {
			if matches!(event, ServerEvent::Closed(_))
				&& let Some((id, _)) = voice.session.clone()
			{
				voice.gateway.update(
					&id,
					AgentVoicePhase::Failed,
					None,
					Some("Voice disconnected. Spoken input will not be replayed."),
				);
				voice.save_transcript_tails(&self.store, &id).await?;
			}
			return Ok(());
		};
		let Some(thread) = params["threadId"].as_str() else { return Ok(()) };
		if method == "turn/started"
			&& let Some(turn) = params.pointer("/turn/id").and_then(Value::as_str)
		{
			self.store
				.observe_agent_voice_turn(voice.generation.clone(), thread.into(), turn.into())
				.await?;
		}
		let Some((id, _current)) = voice.session.clone().filter(|(_, t)| t == thread) else {
			return Ok(());
		};
		if voice.precaution_retired && method != "thread/realtime/closed" {
			return Ok(());
		}
		match method.as_str() {
			"thread/realtime/transcript/delta" => {
				if let Some(index) =
					transcript_role_index(params["role"].as_str().unwrap_or_default())
				{
					if voice.transcript_complete[index] {
						voice.save_transcript_tail(&self.store, &id, index).await?;
					}
					let delta = params["delta"].as_str().unwrap_or_default();
					append_transcript_tail(&mut voice.transcript_tail[index], delta);
				}
			},
			"thread/realtime/transcript/done" => {
				let role = params["role"].as_str().unwrap_or_default();
				let text = params["text"].as_str().unwrap_or_default();
				if let Some(index) = transcript_role_index(role) {
					if voice.transcript_complete[index] {
						voice.save_transcript_tail(&self.store, &id, index).await?;
					}
					voice.transcript_tail[index].clear();
					append_transcript_tail(&mut voice.transcript_tail[index], text);
					voice.transcript_complete[index] = text.len() <= TRANSCRIPT_TAIL_BYTES;
					voice.save_transcript_tail(&self.store, &id, index).await?;
				}
			},
			"thread/realtime/sdp" => {
				let answer = params["sdp"]
					.as_str()
					.and_then(|s| VoiceSdp::new(s.into()).ok())
					.ok_or_else(|| AgentError::Invalid("invalid voice answer".into()))?;
				voice.answer_seen = true;
				voice.gateway.update(&id, AgentVoicePhase::Ready, Some(answer), None);
			},
			"thread/realtime/error" => {
				let detail = crate::agent_voice::provider_error_message(
					params["message"].as_str().unwrap_or_default(),
				);
				voice.gateway.update(&id, AgentVoicePhase::Failed, None, Some(detail));
				// Without a remote answer no client audio can reach this call.
				if !voice.answer_seen {
					self.store.close_agent_voice_call(id).await?;
					voice.session = None;
				}
			},

			"thread/realtime/closed" => {
				// Native stop can close a reply before a final transcript event. Preserve the
				// text already received without replaying it as a new user instruction.
				voice.save_transcript_tails(&self.store, &id).await?;
				self.store.close_agent_voice_call(id.clone()).await?;
				voice.gateway.update(&id, AgentVoicePhase::Ended, None, None);
				voice.session = None;
			},
			_ => {},
		}
		Ok(())
	}

	/// Recover observed native work after a lost call without replaying audio or instructions.
	pub(super) async fn recover_voice_calls(&mut self) -> Result<(), AgentError> {
		for call in self.store.open_agent_voice_calls().await? {
			let params = Self::resume_params(&call.thread_id);
			let resumed = self
				.client
				.thread_resume(params)
				.await
				.map_err(|error| resume_error(error, &call.thread_id))?;
			if exact(&resumed, "/thread/id")? != call.thread_id {
				return Err(AgentError::Invalid("voice recovery thread differs".into()));
			}
			let turns = self
				.client
				.thread_turns_since(&call.thread_id, call.baseline_turn_id.as_deref())
				.await?;
			for turn in &turns {
				let turn_id = exact(turn, "/id")?;
				let observed = self
					.store
					.observe_agent_voice_turn(
						call.generation_id.clone(),
						call.thread_id.clone(),
						turn_id,
					)
					.await?;
				if observed
					&& matches!(
						turn["status"].as_str(),
						Some("completed" | "failed" | "interrupted")
					) {
					self.record_terminal(
						json!({"threadId":call.thread_id,"turn":turn}),
						self.client
							.thread_read_turn(&call.thread_id, exact(turn, "/id")?.as_str())
							.await,
						false,
					)
					.await?;
				}
			}
			// A new admitted process can exist only after the old generation is positively dead.
			let changed_generation =
				self.voice.as_ref().is_some_and(|v| v.generation != call.generation_id);
			if !changed_generation {
				return Err(AgentError::Invalid("old voice process is still owned".into()));
			}
			self.store.close_agent_voice_call(call.session_id).await?;
		}
		Ok(())
	}
}

const TRANSCRIPT_TAIL_BYTES: usize = 32_768;

fn append_transcript_tail(tail: &mut String, delta: &str) {
	if delta.len() >= TRANSCRIPT_TAIL_BYTES {
		let mut start = delta.len() - TRANSCRIPT_TAIL_BYTES;
		while !delta.is_char_boundary(start) {
			start += 1;
		}
		tail.clear();
		tail.push_str(&delta[start..]);
	} else {
		let mut start = (tail.len() + delta.len()).saturating_sub(TRANSCRIPT_TAIL_BYTES);
		while !tail.is_char_boundary(start) {
			start += 1;
		}
		tail.drain(..start);
		tail.push_str(delta);
	}
}

fn transcript_role_index(role: &str) -> Option<usize> {
	match role {
		"user" => Some(0),
		"assistant" => Some(1),
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn selected_speech_uses_only_the_existing_native_call() {
		use decodex_protocol::{EntityId, HistoryText};
		let (mut agent, mut sent, _directory) =
			super::super::tests::fixture_with_history(json!({})).await;
		agent.start_agent("agent", "Coordinate").await.unwrap();
		agent.attach_voice_host("generation".into(), VoiceGateway::new());
		let voice = agent.voice.as_mut().unwrap();
		voice.session = Some(("call".into(), "opaque thread/1".into()));
		voice.answer_seen = true;
		while sent.try_recv().is_ok() {}
		let request = |id| AgentVoiceRequest::Speak {
			session_id: EntityId::new(id).unwrap(),
			text: HistoryText::new("Selected reply").unwrap(),
		};
		assert!(agent.voice_request(request("other")).await.is_err());
		assert!(sent.try_recv().is_err());
		agent.voice_request(request("call")).await.unwrap();
		let frames: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
		assert_eq!(frames.len(), 1, "no resume, start, or new user turn");
		assert_eq!(frames[0]["method"], "thread/realtime/appendSpeech");
		assert_eq!(
			frames[0]["params"],
			json!({"threadId":"opaque thread/1","text":"Selected reply"})
		);
		agent.voice.as_mut().unwrap().precaution_retired = true;
		assert!(agent.voice_request(request("call")).await.is_err());
		assert!(sent.try_recv().is_err());
	}

	#[tokio::test]
	async fn precaution_storage_failure_still_retires_voice_and_requests_native_stop() {
		use decodex_protocol::EntityId;
		for disconnected in [false, true] {
			let (mut agent, mut sent, directory) = super::super::tests::fixture_with_history(
				json!({"_voice_stop_disconnect":disconnected}),
			)
			.await;
			agent.start_agent("agent", "Coordinate").await.unwrap();
			let gateway = crate::agent_voice::VoiceGateway::new();
			agent.attach_voice_host("generation".into(), gateway.clone());
			gateway.exchange(&AgentVoiceRequest::Start {
				session_id: EntityId::new("voice").unwrap(),
				work_id: EntityId::new("agent").unwrap(),
				offer: VoiceSdp::new("offer".into()).unwrap(),
				options: Default::default(),
			});
			agent.voice.as_mut().unwrap().session =
				Some(("voice".into(), "opaque thread/1".into()));
			let root = decodex_core::DecodexRoot::new(
				directory.path().canonicalize().unwrap().join("root"),
			)
			.unwrap();
			let database =
				rusqlite::Connection::open(root.paths().product_database_file()).unwrap();
			database.execute_batch("CREATE TRIGGER fail_voice_retirement BEFORE UPDATE OF retired_voice ON agent_misalignment BEGIN SELECT RAISE(FAIL, 'injected retirement write failure'); END;").unwrap();
			while sent.try_recv().is_ok() {}
			assert!(
				agent
					.observe_misalignment(
						"opaque thread/1",
						"opaque turn/1",
						&json!({"codexErrorInfo":"misalignmentPolicyViolation"})
					)
					.await
					.is_err()
			);
			agent
				.voice_event(&ServerEvent::Notification {
					method: "thread/realtime/sdp".into(),
					params: json!({"threadId":"opaque thread/1","sdp":"late-answer"}),
				})
				.await
				.unwrap();
			assert_eq!(
				gateway
					.exchange(&AgentVoiceRequest::Poll {
						session_id: EntityId::new("voice").unwrap()
					})
					.phase,
				AgentVoicePhase::Failed
			);
			assert!(agent.voice.as_ref().unwrap().precaution_retired);
			assert!(agent.voice.as_ref().unwrap().session.is_some());
			assert!(agent.store.agent_misalignment("agent".into()).await.unwrap().is_some());
			let requests: Vec<_> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
			assert_eq!(requests.len(), 1);
			assert_eq!(requests[0]["method"], "thread/realtime/stop");
		}
	}

	#[tokio::test]
	async fn misalignment_retires_voice_even_when_stop_acknowledgment_is_lost() {
		use decodex_protocol::{AgentVoicePhase, AgentVoiceRequest, EntityId, VoiceSdp};
		for disconnected in [false, true] {
			let (mut agent, mut sent, directory) = super::super::tests::fixture_with_history(
				json!({"_voice_stop_disconnect":disconnected}),
			)
			.await;
			agent.start_agent("agent", "Coordinate").await.unwrap();
			let gateway = crate::agent_voice::VoiceGateway::new();
			agent.attach_voice_host("generation".into(), gateway.clone());
			let start = AgentVoiceRequest::Start {
				session_id: EntityId::new("voice").unwrap(),
				work_id: EntityId::new("agent").unwrap(),
				offer: VoiceSdp::new("offer".into()).unwrap(),
				options: Default::default(),
			};
			gateway.exchange(&start);
			agent.voice.as_mut().unwrap().session =
				Some(("voice".into(), "opaque thread/1".into()));
			while sent.try_recv().is_ok() {}
			agent
				.observe_misalignment(
					"opaque thread/1",
					"opaque turn/1",
					&json!({"codexErrorInfo":"misalignmentPolicyViolation"}),
				)
				.await
				.unwrap();
			agent
				.voice_event(&ServerEvent::Notification {
					method: "thread/realtime/sdp".into(),
					params: json!({"threadId":"opaque thread/1","sdp":"late-answer"}),
				})
				.await
				.unwrap();
			assert_eq!(
				gateway
					.exchange(&AgentVoiceRequest::Poll {
						session_id: EntityId::new("voice").unwrap()
					})
					.phase,
				AgentVoicePhase::Failed
			);
			assert!(agent.voice_request(start).await.is_err());
			agent.store.complete_agent_turn("agent".into(), "opaque turn/1".into()).await.unwrap();
			let review = agent.store.agent_misalignment("agent".into()).await.unwrap().unwrap();
			let root = decodex_core::DecodexRoot::new(
				directory.path().canonicalize().unwrap().join("root"),
			)
			.unwrap();
			let reopened = decodex_database::SqliteStore::open(&root.paths()).unwrap();
			// Even fully current later-turn evidence cannot clear a voice-retired precaution.
			reopened
				.reconcile_agent_misalignment("agent".into(), review.clone(), || true)
				.await
				.unwrap();
			assert_eq!(reopened.agent_misalignment("agent".into()).await.unwrap(), Some(review));
			let mut stops = 0;
			while let Ok(request) = sent.try_recv() {
				assert_eq!(request["method"], "thread/realtime/stop");
				stops += 1;
			}
			assert_eq!(stops, 1);
		}
	}
}

#[cfg(test)]
#[path = "voice_persistence_tests.rs"]
mod persistence_tests;
