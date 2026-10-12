//! One native WebRTC call. PCM runs independently of GPUI and service signaling.
#![cfg_attr(test, allow(dead_code))]
use std::{
	future,
	sync::{
		Arc,
		atomic::{AtomicBool, AtomicU32, Ordering},
		mpsc::{self, SyncSender},
	},
};

use futures_util::StreamExt as _;
use libwebrtc::{
	RtcError,
	audio_frame::AudioFrame,
	audio_source::{AudioSourceOptions, native::NativeAudioSource},
	audio_stream::native::NativeAudioStream,
	data_channel::{DataChannelInit, DataChannelState},
	media_stream_track::MediaStreamTrack,
	peer_connection::{IceGatheringState, OfferOptions, PeerConnection, PeerConnectionState},
	peer_connection_factory::{
		ContinualGatheringPolicy, PeerConnectionFactory, RtcConfiguration,
		native::PeerConnectionFactoryExt as _,
	},
	session_description::{SdpType, SessionDescription},
};
use serde_json::{self, Value};
use tokio::{
	sync::{self, oneshot},
	time::{self, Duration, Instant, MissedTickBehavior},
};

use crate::shell::agent_surface::voice::audio::{
	LIVE_FRAME_SAMPLES, LIVE_SAMPLE_RATE, Pcm,
	owner::{AudioError, Mode, Session, State},
};

#[cfg(test)]
type OfferPause = (oneshot::Sender<()>, oneshot::Receiver<()>);

pub(super) enum Command {
	Answer(String),
	Mute(bool),
}

enum Input {
	Device(u32),
	#[cfg(test)]
	Pcm(Pcm),
}

pub(super) struct Transport {
	commands: sync::mpsc::Sender<Command>,
	events: mpsc::Receiver<Value>,
	overflow: Arc<AtomicBool>,
	level: Arc<AtomicU32>,
	stop: Option<oneshot::Sender<()>>,
	cancelled: Arc<AtomicBool>,
}
impl Transport {
	pub(super) fn start_device(device: u32) -> Result<Self, AudioError> {
		Self::start_input(
			Input::Device(device),
			#[cfg(test)]
			None,
		)
	}

	#[cfg(test)]
	pub(super) fn start(pcm: Pcm) -> Result<Self, AudioError> {
		Self::start_inner(
			pcm,
			#[cfg(test)]
			None,
		)
	}

	#[cfg(test)]
	fn start_inner(pcm: Pcm, offer_pause: Option<OfferPause>) -> Result<Self, AudioError> {
		Self::start_input(Input::Pcm(pcm), offer_pause)
	}

	fn start_input(
		input: Input,
		#[cfg(test)] offer_pause: Option<OfferPause>,
	) -> Result<Self, AudioError> {
		let (commands, requests) = sync::mpsc::channel(8);
		let (sender, events) = mpsc::sync_channel(128);
		let overflow = Arc::new(AtomicBool::new(false));
		let output = Events { sender, overflow: overflow.clone() };
		let (stop, ended) = oneshot::channel();
		let cancelled = Arc::new(AtomicBool::new(false));
		let level = Arc::new(AtomicU32::new(0));
		let call = Call {
			requests,
			output,
			ended,
			level: level.clone(),
			#[cfg(test)]
			offer_pause,
		};
		let stopped = cancelled.clone();
		std::thread::Builder::new()
			.name("native-voice".into())
			.spawn(move || match input {
				Input::Device(id) => call.run(Audio::Pending { device: id, cancelled: stopped }),
				#[cfg(test)]
				Input::Pcm(pcm) => call.run(Audio::Pcm(pcm)),
			})
			.map_err(|_| AudioError::Worker)?;
		Ok(Self { commands, events, overflow, level, stop: Some(stop), cancelled })
	}

	pub(super) fn command(&self, command: Command) -> bool {
		self.commands.try_send(command).is_ok()
	}

	pub(super) fn level(&self) -> f32 {
		f32::from_bits(self.level.load(Ordering::Relaxed))
	}

	pub(super) fn poll(&self) -> Option<Value> {
		self.events.try_recv().ok().or_else(|| {
			self.overflow.swap(false, Ordering::AcqRel).then(
				|| serde_json::json!({"type":"error","message":"Audio updates could not be delivered. The call stopped."}),
			)
		})
	}
}
impl Drop for Transport {
	fn drop(&mut self) {
		self.cancelled.store(true, Ordering::Release);
		if let Some(stop) = self.stop.take() {
			let _ = stop.send(());
		}
	}
}

enum Audio {
	Pending {
		device: u32,
		cancelled: Arc<AtomicBool>,
	},
	Device(Session),
	#[cfg(test)]
	Pcm(Pcm),
}
impl Audio {
	fn activate(&mut self) -> Result<(), AudioError> {
		if let Self::Pending { device, cancelled } = self {
			*self = Self::Device(Session::start(*device, Mode::Live, cancelled.clone())?);
		}
		let pcm = self.pcm();
		for _ in 0..pcm.captured.slots() {
			let _ = pcm.captured.pop();
		}
		Ok(())
	}

	fn state(&mut self) -> Option<Arc<State>> {
		match self {
			Self::Pending { .. } => None,
			Self::Device(session) => Some(session.parts().0),
			#[cfg(test)]
			Self::Pcm(_) => None,
		}
	}

	fn pcm(&mut self) -> &mut Pcm {
		match self {
			Self::Pending { .. } => unreachable!("audio is activated before PCM delivery"),
			Self::Device(session) => session.parts().1,
			#[cfg(test)]
			Self::Pcm(pcm) => pcm,
		}
	}
}
struct Call {
	requests: sync::mpsc::Receiver<Command>,
	output: Events,
	ended: oneshot::Receiver<()>,
	level: Arc<AtomicU32>,
	#[cfg(test)]
	offer_pause: Option<OfferPause>,
}
impl Call {
	fn run(mut self, audio: Audio) {
		let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_time().build()
		else {
			self.output.send(
				serde_json::json!({"type":"error","message":"The native audio runtime could not start."}),
			);
			return;
		};
		if !matches!(self.ended.try_recv(), Err(oneshot::error::TryRecvError::Empty)) {
			return;
		}
		runtime.block_on(async {
			tokio::select! {
				_ = self.ended => {},
				_ = run(audio, self.requests, self.output.clone(), self.level, #[cfg(test)] self.offer_pause) => {},
			}
		});
	}
}

#[derive(Clone)]
struct Events {
	sender: SyncSender<Value>,
	overflow: Arc<AtomicBool>,
}
impl Events {
	fn send(&self, value: Value) {
		if matches!(self.sender.try_send(value), Err(mpsc::TrySendError::Full(_))) {
			self.overflow.store(true, Ordering::Release);
		}
	}
}
struct Peer(PeerConnection);
impl Drop for Peer {
	fn drop(&mut self) {
		self.0.close();
	}
}

async fn run(
	audio: Audio,
	commands: sync::mpsc::Receiver<Command>,
	events: Events,
	level: Arc<AtomicU32>,
	#[cfg(test)] offer_pause: Option<OfferPause>,
) {
	let factory = PeerConnectionFactory::default();

	// Apple owns capture, playback and DSP. No second audio device or processing chain.
	factory.set_adm_recording_enabled(false);
	factory.set_adm_playout_enabled(false);

	let mut config = RtcConfiguration::default();

	config.continual_gathering_policy = ContinualGatheringPolicy::GatherOnce;

	let peer = match factory.create_peer_connection(config) {
		Ok(peer) => Peer(peer),
		Err(_) => {
			events.send(
				serde_json::json!({"type":"error","message":"The audio connection could not start."}),
			);

			return;
		},
	};
	// Release capture/PCM before synchronous peer teardown, including cancellation.
	let mut audio = audio;
	let result = run_media(
		&factory,
		&peer,
		&mut audio,
		commands,
		events.clone(),
		level,
		#[cfg(test)]
		offer_pause,
	)
	.await;

	// Let the UI stop its audio device without waiting for native network teardown.
	if let Err(message) = result {
		events.send(serde_json::json!({"type":"error","message":message}));
	}
}

async fn gather_audio_offer(
	peer: &Peer,
	#[cfg(test)] offer_pause: Option<OfferPause>,
) -> Result<String, RtcError> {
	let options = || OfferOptions { offer_to_receive_audio: true, ..Default::default() };
	let offer = peer.0.create_offer(options()).await?;

	#[cfg(test)]
	if let Some((entered, resume)) = offer_pause {
		// Hold the native offer before applying it; cancellation must discard this
		// continuation.
		let _ = entered.send(());
		let _ = resume.await;
	}

	peer.0.set_local_description(offer).await?;

	let until = Instant::now() + Duration::from_secs(3);

	while peer.0.ice_gathering_state() != IceGatheringState::Complete && Instant::now() < until {
		time::sleep(Duration::from_millis(20)).await;
	}

	// The binding exposes only current (not pending) SDP. Regenerate through libwebrtc
	// to include gathered candidates while retaining the same ICE credentials.
	let offer = peer.0.create_offer(options()).await?;

	peer.0.set_local_description(offer.clone()).await?;

	Ok::<_, RtcError>(offer.to_string())
}

async fn run_media(
	factory: &PeerConnectionFactory,
	peer: &Peer,
	audio: &mut Audio,
	mut commands: sync::mpsc::Receiver<Command>,
	events: Events,
	level: Arc<AtomicU32>,
	#[cfg(test)] offer_pause: Option<OfferPause>,
) -> Result<(), &'static str> {
	let started = Instant::now();
	let source = NativeAudioSource::new(AudioSourceOptions::default(), LIVE_SAMPLE_RATE, 1, 40);
	let track = factory.create_audio_track("microphone", source.clone());

	peer.0
		.add_track(track.clone().into(), &["microphone"])
		.map_err(|_| "The microphone track could not start.")?;

	let (tracks, mut incoming) = sync::mpsc::channel(1);

	peer.0.on_track(Some(Box::new(move |event| {
		if let MediaStreamTrack::Audio(track) = event.track {
			let _ = tracks.try_send(track);
		}
	})));

	peer.0.on_data_channel(Some(Box::new(|channel| channel.close())));

	let data = peer
		.0
		.create_data_channel("oai-events", DataChannelInit::default())
		.map_err(|_| "The audio event channel could not start.")?;
	let captions = events.clone();

	data.on_message(Some(Box::new(move |buffer| {
		if !buffer.binary
			&& buffer.data.len() <= 65_536
			&& let Ok(value) = serde_json::from_slice::<Value>(buffer.data)
			&& matches!(
				value["type"].as_str(),
				Some(
					"input_transcript.added"
						| "output_transcript.added"
						| "turn.created"
						| "turn.done"
						| "turn.delta"
				)
			) {
			captions.send(serde_json::json!({"type":"caption","event":value}));
		}
	})));

	let offer = time::timeout(
		Duration::from_secs(10),
		gather_audio_offer(
			peer,
			#[cfg(test)]
			offer_pause,
		),
	)
	.await
	.map_err(|_| "The audio offer timed out.")?
	.map_err(|_| "The audio offer could not be created.")?;

	eprintln!("DecodexVoice offer_ready_ms={}", started.elapsed().as_millis());
	events.send(serde_json::json!({"type":"offer","sdp":offer}));

	let deadline = Instant::now() + Duration::from_secs(30);
	let mut announced = false;
	let mut remote: Option<NativeAudioStream> = None;
	let mut clock = time::interval(Duration::from_millis(10));

	clock.set_missed_tick_behavior(MissedTickBehavior::Skip);

	let mut frame = AudioFrame::new(LIVE_SAMPLE_RATE, 1, LIVE_FRAME_SAMPLES);

	loop {
		tokio::select! {
			command = commands.recv() => match command {
				Some(Command::Answer(sdp)) => {
					eprintln!("DecodexVoice answer_received_ms={}", started.elapsed().as_millis());
					let answer = SessionDescription::parse(&sdp, SdpType::Answer).map_err(|_| "The audio answer was invalid.")?;

					peer.0.set_remote_description(answer).await.map_err(|_| "The audio answer could not be applied.")?;
				},
				Some(Command::Mute(muted)) => { track.set_enabled(!muted); },
				None => return Ok(()),
			},
			Some(track) = incoming.recv() => remote = Some(NativeAudioStream::new(track, LIVE_SAMPLE_RATE as i32, 1)),
			decoded = async { match &mut remote {
				Some(stream) => stream.next().await,
				None => future::pending().await,
			}} => {
				let Some(decoded) = decoded else { return Err("The remote audio track ended."); };

				if announced {
					for &sample in decoded.data.iter() { let _ = audio.pcm().playback.push(f32::from(sample) / 32_768.0); }
				}
			},
			_ = clock.tick() => {
				if events.overflow.load(Ordering::Acquire) { return Err("Audio updates could not be delivered."); }

				let connected = peer.0.connection_state() == PeerConnectionState::Connected && data.state() == DataChannelState::Open;

				if !announced && connected {
					let audio_start = Instant::now();
					audio.activate().map_err(AudioError::message)?;
					eprintln!("DecodexVoice ready_ms={} audio_start_ms={}", started.elapsed().as_millis(), audio_start.elapsed().as_millis());
					announced = true;
					events.send(serde_json::json!({"type":"connected"}));
				}
				if matches!(peer.0.connection_state(), PeerConnectionState::Failed | PeerConnectionState::Disconnected | PeerConnectionState::Closed)
					|| matches!(data.state(), DataChannelState::Closed | DataChannelState::Closing) {
					return Err("The audio connection was lost.");
				}
				if !announced && Instant::now() >= deadline { return Err("The audio connection timed out."); }
				if !announced { continue; }
				if let Some(device) = audio.state() {
					if !device.running() { return Err("The audio device stopped. Select a device and start a new call."); }
					level.store(device.level().to_bits(), Ordering::Relaxed);
				}
				let pcm = audio.pcm();
				if pcm.captured.slots() >= LIVE_FRAME_SAMPLES as usize {
					for sample in frame.data.to_mut() {
						let value = pcm.captured.pop().unwrap_or(0.0);

						*sample = (value * if value < 0.0 { 32_768.0 } else { 32_767.0 }) as i16;
					}

					source.capture_frame(&frame).await.map_err(|_| "Microphone audio could not be delivered.")?;
				}
			},
		}
	}
}

#[cfg(test)]
mod tests {
	use futures_util::StreamExt as _;
	use libwebrtc::{
		peer_connection::AnswerOptions,
		peer_connection_factory::native::PeerConnectionFactoryExt as _,
	};
	use tokio::{sync::mpsc, time};

	use crate::shell::agent_surface::voice::transport::{
		AudioFrame, AudioSourceOptions, Command, ContinualGatheringPolicy, Duration,
		IceGatheringState, MediaStreamTrack, NativeAudioSource, NativeAudioStream, Pcm, Peer,
		PeerConnectionFactory, RtcConfiguration, SdpType, SessionDescription, Transport, Value,
		oneshot,
	};

	async fn event(transport: &Transport, kind: &str) -> Value {
		time::timeout(Duration::from_secs(15), async {
			loop {
				if let Some(value) = transport.poll() {
					assert_ne!(value["type"], "error", "{value}");

					if value["type"] == kind {
						return value;
					}
				}

				time::sleep(Duration::from_millis(10)).await;
			}
		})
		.await
		.unwrap_or_else(|error| panic!("native media event {kind}: {error}"))
	}

	async fn energy(stream: &mut NativeAudioStream) -> f64 {
		let mut sum = 0.0;

		for index in 0..50 {
			let frame =
				time::timeout(Duration::from_secs(5), stream.next()).await.unwrap().unwrap();

			if index >= 30 {
				sum += frame.data.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>();
			}
		}

		sum
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
	async fn native_call_exchanges_audio_captions_mutes_and_releases_buffers() {
		call_lifecycle(false).await;
	}

	#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
	async fn remote_channel_loss_reports_failure_and_releases_buffers() {
		call_lifecycle(true).await;
	}

	#[tokio::test]
	async fn connecting_does_not_open_the_selected_audio_device() {
		// An invalid device must not be touched until a peer answer connects.
		let transport = Transport::start_device(u32::MAX).unwrap();
		event(&transport, "offer").await;
		assert_eq!(transport.level(), 0.);
		drop(transport);
	}

	#[tokio::test]
	async fn cancellation_releases_audio_before_and_after_the_offer_without_an_answer() {
		for wait_for_offer in [false, true] {
			let (input, captured) = rtrb::RingBuffer::new(4_800);
			let (playback, output) = rtrb::RingBuffer::new(4_800);
			let transport = Transport::start(Pcm { captured, playback }).unwrap();

			if wait_for_offer {
				event(&transport, "offer").await;
			}

			drop(transport);

			time::timeout(Duration::from_secs(3), async {
				while !input.is_abandoned() || !output.is_abandoned() {
					time::sleep(Duration::from_millis(10)).await;
				}
			})
			.await
			.expect("cancellation releases both audio endpoints without an answer");
		}
	}

	#[tokio::test]
	async fn cancelled_offer_continuation_cannot_resume_or_block_the_next_call() {
		let (input, captured) = rtrb::RingBuffer::new(4_800);
		let (playback, output) = rtrb::RingBuffer::new(4_800);
		let (entered, paused) = oneshot::channel();
		let (resume, continued) = oneshot::channel();
		let transport =
			Transport::start_inner(Pcm { captured, playback }, Some((entered, continued))).unwrap();

		time::timeout(Duration::from_secs(15), paused).await.unwrap().unwrap();

		assert!(transport.poll().is_none(), "paused offer must not reach signaling");

		drop(transport);

		time::timeout(Duration::from_secs(3), async {
			while !input.is_abandoned() || !output.is_abandoned() {
				time::sleep(Duration::from_millis(10)).await;
			}
		})
		.await
		.expect("cancelling an unapplied offer releases both audio endpoints");

		assert!(resume.send(()).is_err(), "cancelled offer continuation must be dropped");

		let (input, captured) = rtrb::RingBuffer::new(4_800);
		let (playback, output) = rtrb::RingBuffer::new(4_800);
		let next = Transport::start(Pcm { captured, playback }).unwrap();
		let offer = event(&next, "offer").await;

		assert!(offer["sdp"].as_str().is_some_and(|sdp| !sdp.is_empty()));
		assert!(!input.is_abandoned() && !output.is_abandoned());

		drop(next);

		time::timeout(Duration::from_secs(3), async {
			while !input.is_abandoned() || !output.is_abandoned() {
				time::sleep(Duration::from_millis(10)).await;
			}
		})
		.await
		.expect("the replacement call also releases both audio endpoints");
	}

	async fn connect_fixture_peer(peer: &Peer, transport: &Transport) {
		let offer = event(transport, "offer").await;
		let sdp = offer["sdp"].as_str().unwrap();
		// Both peers are on this host. Use one advertised interface so virtual
		// adapters do not add unrelated ICE paths to this media lifecycle test.
		let address = sdp
			.lines()
			.filter_map(|line| {
				let fields: Vec<_> = line.split_whitespace().collect();
				(line.starts_with("a=candidate:") && fields.get(2) == Some(&"udp"))
					.then(|| fields[4].to_owned())
			})
			.next()
			.expect("local UDP candidate");
		let offer = fixture_sdp_on_interface(sdp, &address);

		peer.0
			.set_remote_description(SessionDescription::parse(&offer, SdpType::Offer).unwrap())
			.await
			.unwrap();
		peer.0
			.set_local_description(peer.0.create_answer(AnswerOptions::default()).await.unwrap())
			.await
			.unwrap();

		while peer.0.ice_gathering_state() != IceGatheringState::Complete {
			time::sleep(Duration::from_millis(10)).await;
		}

		assert!(transport.command(Command::Answer(fixture_sdp_on_interface(
			&peer.0.current_local_description().unwrap().to_string(),
			&address
		))));

		event(transport, "connected").await;
	}

	fn fixture_sdp_on_interface(sdp: &str, address: &str) -> String {
		let mut candidates = 0;
		let lines: Vec<_> = sdp
			.lines()
			.filter(|line| {
				if !line.starts_with("a=candidate:") {
					return true;
				}
				let fields: Vec<_> = line.split_whitespace().collect();
				let selected = fields.get(2) == Some(&"udp") && fields.get(4) == Some(&address);
				candidates += usize::from(selected);
				selected
			})
			.collect();
		assert!(candidates > 0, "fixture peers must share an advertised UDP interface");
		lines.join("\r\n") + "\r\n"
	}

	async fn feed_fixture_tone(mut input: rtrb::Producer<f32>, source: NativeAudioSource) {
		let mut sample_index = 0;
		let mut frame = AudioFrame::new(48_000, 1, 480);

		loop {
			for sample in frame.data.to_mut() {
				let value =
					((sample_index as f32) * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.2;

				sample_index += 1;

				let _ = input.push(value);

				*sample = (value * 32_767.0) as i16;
			}

			source.capture_frame(&frame).await.unwrap();

			time::sleep(Duration::from_millis(10)).await;
		}
	}

	async fn call_lifecycle(close_remote_channel: bool) {
		time::timeout(Duration::from_secs(45), async {
			let (input, captured) = rtrb::RingBuffer::new(4_800);
			let (playback, mut output) = rtrb::RingBuffer::new(4_800);
			let transport = Transport::start(Pcm { captured, playback }).unwrap();

			assert!(transport.command(Command::Mute(true)), "mute can precede negotiation");

			let factory = PeerConnectionFactory::default();

			factory.set_adm_recording_enabled(false);
			factory.set_adm_playout_enabled(false);

			let mut config = RtcConfiguration::default();

			config.continual_gathering_policy = ContinualGatheringPolicy::GatherOnce;

			let peer = Peer(factory.create_peer_connection(config).unwrap());
			let source = NativeAudioSource::new(AudioSourceOptions::default(), 48_000, 1, 40);

			peer.0
				.add_track(factory.create_audio_track("remote", source.clone()).into(), &["remote"])
				.unwrap();

			let (tracks, mut track_events) = mpsc::channel(1);

			peer.0.on_track(Some(Box::new(move |event| {
				if let MediaStreamTrack::Audio(track) = event.track {
					let _ = tracks.try_send(track);
				}
			})));

			let (channels, mut channel_events) = mpsc::channel(1);

			peer.0.on_data_channel(Some(Box::new(move |data| {
				let _ = channels.try_send(data);
			})));

			connect_fixture_peer(&peer, &transport).await;

			let data = channel_events.recv().await.unwrap();

			data.send(br#"{"type":"turn.delta","test":"caption"}"#, false).unwrap();

			assert_eq!(event(&transport, "caption").await["event"]["test"], "caption");

			let mut stream = NativeAudioStream::new(track_events.recv().await.unwrap(), 48_000, 1);
			let feed = tokio::spawn(feed_fixture_tone(input, source));
			let initially_muted = energy(&mut stream).await;

			while output.pop().is_ok() {}

			assert!(transport.command(Command::Mute(false)));

			let audible = energy(&mut stream).await;

			assert!(initially_muted < audible * 0.1);
			assert!(audible > 1_000_000.0);

			let mut returned = 0.0;

			while let Ok(sample) = output.pop() {
				returned += sample.abs();
			}

			assert!(returned > 0.1, "decoded return tone reaches the playback queue");
			assert!(transport.command(Command::Mute(true)));

			let muted = energy(&mut stream).await;

			assert!(muted < audible * 0.1, "muted microphone must not transmit the tone");
			assert!(transport.command(Command::Mute(false)));
			assert!(energy(&mut stream).await > audible * 0.3);

			if close_remote_channel {
				data.close();

				let failure = time::timeout(Duration::from_secs(3), async {
					loop {
						if let Some(value) = transport.poll()
							&& value["type"] == "error"
						{
							break value;
						}

						time::sleep(Duration::from_millis(10)).await;
					}
				})
				.await
				.expect("remote channel loss reports failure");

				assert_eq!(failure["message"], "The audio connection was lost.");
			} else {
				drop(transport);
			}

			time::timeout(Duration::from_secs(3), async {
				while !output.is_abandoned() {
					time::sleep(Duration::from_millis(10)).await;
				}
			})
			.await
			.expect("closing releases the native audio producer");

			feed.abort();

			let _ = feed.await;

			stream.close();
		})
		.await
		.expect("native call lifecycle");
	}
}
