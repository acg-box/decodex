//! Explicit online acceptance with synthetic PCM only. Never opens a microphone.
use std::{fs, path::PathBuf};

use decodex_protocol::{
	AgentClient, AgentSnapshotResult, AgentVoiceOptions, AgentVoicePhase, AgentVoiceRequest,
	ClientProfile, DictationAudio, DictationPhase, DictationRequest, EntityId, VoiceSdp,
};
use tokio::time::{self, Duration, Instant};

use super::{
	super::{Caption, update_caption},
	Command, Pcm, Transport,
};

const WORDS: [&str; 3] = ["blue", "garden", "seven"];

fn session_id(prefix: &str) -> EntityId {
	let nonce =
		std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
	EntityId::new(format!("{prefix}-{nonce}")).unwrap()
}

async fn fixture() -> (AgentClient, PathBuf, EntityId) {
	let root = PathBuf::from(
		std::env::var_os("DECODEX_VOICE_ACCEPTANCE_ROOT").expect("isolated service root"),
	);
	assert_eq!(
		fs::read_to_string(root.join("synthetic-voice-fixture")).unwrap(),
		"Decodex synthetic voice acceptance\n",
		"online acceptance requires an explicitly prepared isolated service"
	);
	let client = AgentClient::new(ClientProfile::load(&root, None).unwrap());
	let agent = EntityId::new("voice-acceptance").unwrap();
	let AgentSnapshotResult::Available(snapshot) = client.query().await.unwrap() else {
		panic!("isolated Agent is unavailable")
	};
	assert_eq!(snapshot.work_items.len(), 1, "never send real conversation context");
	assert_eq!(snapshot.work_items[0].id, agent.as_str());
	assert!(snapshot.work_items[0].active_turn_id.is_none());
	let input = PathBuf::from(
		std::env::var_os("DECODEX_VOICE_ACCEPTANCE_INPUT").expect("synthetic PCM directory"),
	);
	(client, input, agent)
}

fn contains_words(text: &str) -> bool {
	let text = text.to_ascii_lowercase();
	WORDS.iter().all(|word| text.contains(word))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Requires explicit online synthetic-audio permission and an isolated voice fixture"]
async fn online_synthetic_dictation_returns_final_text_without_sending_an_agent_turn() {
	let (client, input, agent) = fixture().await;
	let before = client.history(agent.clone()).await.unwrap();
	let id = session_id("dictation");
	let mut status =
		client.dictation(DictationRequest::Start { session_id: id.clone() }).await.unwrap();
	let deadline = Instant::now() + Duration::from_secs(30);
	while status.phase == DictationPhase::Connecting && Instant::now() < deadline {
		time::sleep(Duration::from_millis(100)).await;
		status = client.dictation(DictationRequest::Poll { session_id: id.clone() }).await.unwrap();
	}
	if status.phase == DictationPhase::Listening {
		let pcm = fs::read(input.join("input-24000.pcm")).unwrap();
		for chunk in pcm.chunks(9_600) {
			status = client
				.dictation(DictationRequest::Audio {
					session_id: id.clone(),
					audio: DictationAudio::from_pcm(chunk.to_vec()).unwrap(),
				})
				.await
				.unwrap();
			if status.phase == DictationPhase::Failed {
				break;
			}
			time::sleep(Duration::from_millis(200)).await;
		}
		if status.phase != DictationPhase::Failed {
			status = client
				.dictation(DictationRequest::Finish { session_id: id.clone() })
				.await
				.unwrap();
			let deadline = Instant::now() + Duration::from_secs(30);
			while status.phase == DictationPhase::Finalizing && Instant::now() < deadline {
				time::sleep(Duration::from_millis(100)).await;
				status = client
					.dictation(DictationRequest::Poll { session_id: id.clone() })
					.await
					.unwrap();
			}
		}
	}
	let _ = client.dictation(DictationRequest::Cancel { session_id: id }).await;
	assert_eq!(status.phase, DictationPhase::Complete, "{status:?}");
	assert!(contains_words(status.text.as_str()), "synthetic words were not transcribed");
	assert_eq!(client.history(agent).await.unwrap(), before, "dictation must not send a task");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Requires explicit online synthetic-audio permission and an isolated voice fixture"]
async fn online_synthetic_live_voice_returns_speech_through_the_production_bridge() {
	let (client, input, agent) = fixture().await;
	let id = session_id("voice");
	let (mut producer, captured) = rtrb::RingBuffer::new(4_800);
	let (playback, mut output) = rtrb::RingBuffer::new(48_000);
	let transport = Transport::start(Pcm { captured, playback }).unwrap();
	let pcm = fs::read(input.join("input-48000.pcm")).unwrap();
	let samples: Vec<f32> = pcm
		.as_chunks::<2>()
		.0
		.iter()
		.map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32_768.0)
		.collect();
	let result = time::timeout(Duration::from_secs(90), async {
		let mut started = false;
		let mut connected = false;
		let mut answer_applied = false;
		let mut sent = 0;
		let mut spoken = Vec::new();
		let mut captions: Vec<Caption> = Vec::new();
		let mut events = String::new();
		let mut last_signal = None;
		let mut last_poll = Instant::now() - Duration::from_secs(1);
		let mut clock = time::interval(Duration::from_millis(10));
		clock.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
		loop {
			clock.tick().await;
			while let Some(event) = transport.poll() {
				match event["type"].as_str() {
					Some("offer") => {
						let status = client
							.voice(AgentVoiceRequest::Start {
								session_id: id.clone(),
								work_id: agent.clone(),
								offer: VoiceSdp::new(event["sdp"].as_str().unwrap().into())
									.unwrap(),
								options: AgentVoiceOptions::default(),
							})
							.await
							.map_err(|error| format!("voice signaling: {error:?}"))?;
						if status.phase == AgentVoicePhase::Failed {
							return Err(format!("{status:?}"));
						}
						started = true;
					},
					Some("connected") => connected = true,
					Some("caption") => {
						update_caption(&event["event"], &mut captions);
						events.push_str(&event["event"].to_string());
						events.push('\n');
					},
					Some("error") =>
						return Err(event["message"].as_str().unwrap_or("media error").to_owned()),
					_ => {},
				}
			}
			if started && last_poll.elapsed() >= Duration::from_millis(100) {
				last_poll = Instant::now();
				let status = client
					.voice(AgentVoiceRequest::Poll { session_id: id.clone() })
					.await
					.map_err(|error| format!("voice observation: {error:?}"))?;
				if status.phase == AgentVoicePhase::Failed {
					return Err(format!("{status:?}"));
				}
				if !answer_applied && let Some(answer) = status.answer {
					if !transport.command(Command::Answer(answer.as_str().into())) {
						return Err("answer was not accepted".into());
					}
					answer_applied = true;
				}
			}
			if connected {
				for _ in 0..480 {
					let sample = samples.get(sent).copied().unwrap_or(0.0);
					if producer.push(sample).is_ok() {
						sent += 1;
					}
				}
			}
			while let Ok(sample) = output.pop() {
				if sample.abs() > 0.01 {
					last_signal = Some(Instant::now());
				}
				spoken.push(sample);
			}
			let returned = captions
				.iter()
				.any(|c| c.role == "assistant" && c.complete && contains_words(&c.text));
			if returned
				&& spoken.len() > 48_000
				&& last_signal.is_some_and(|t| t.elapsed() > Duration::from_millis(500))
			{
				let pcm: Vec<u8> =
					spoken.iter().flat_map(|v| ((*v * 32_767.0) as i16).to_le_bytes()).collect();
				fs::write(input.join("returned-48000.pcm"), pcm).unwrap();
				fs::write(input.join("live-captions.jsonl"), events).unwrap();
				return Ok(());
			}
		}
	})
	.await;
	let mut stopped =
		client.voice(AgentVoiceRequest::Stop { session_id: id.clone() }).await.unwrap();
	let deadline = Instant::now() + Duration::from_secs(10);
	while matches!(stopped.phase, AgentVoicePhase::Connecting | AgentVoicePhase::Ready)
		&& Instant::now() < deadline
	{
		time::sleep(Duration::from_millis(100)).await;
		stopped = client.voice(AgentVoiceRequest::Poll { session_id: id.clone() }).await.unwrap();
	}
	drop(transport);
	assert!(matches!(result, Ok(Ok(()))), "online voice result: {result:?}");
	assert_eq!(stopped.phase, AgentVoicePhase::Ended, "{stopped:?}");
}
