//! Sole owner of native audio I/O. Network work never runs here.
//!
//! Sessions reserve dictation queues or one prewarmed WebRTC factory. Cancellation stops
//! capture independently of transport teardown. Dictation returns its reusable queue;
//! Live prepares the next factory after release because WebRTC retires its native device
//! with the last peer. Identity checks discard late releases from an old session.
use std::{
	sync::{
		Arc, OnceLock,
		atomic::{AtomicBool, AtomicU32, Ordering},
		mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender},
	},
	thread::Builder,
	time::{Duration, Instant},
};

use libwebrtc::peer_connection_factory::PeerConnectionFactory;

use crate::shell::agent_surface::voice::audio::{Device, LIVE_SAMPLE_RATE, Pcm};
use decodex_protocol::DICTATION_SAMPLE_RATE;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::shell::agent_surface::voice) enum Mode {
	Dictation,
	Live,
}
impl Mode {
	fn slot(self) -> usize {
		match self {
			Self::Dictation => 0,
			Self::Live => 1,
		}
	}

	fn rate(self) -> f64 {
		match self {
			Self::Dictation => f64::from(DICTATION_SAMPLE_RATE),
			Self::Live => f64::from(LIVE_SAMPLE_RATE),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::shell::agent_surface::voice) enum AudioError {
	Busy,
	Worker,
	Prepare,
	Start,
	Cancelled,
	Capture,
}
impl AudioError {
	pub(in crate::shell::agent_surface::voice) fn message(self) -> &'static str {
		match self {
			Self::Busy => "The microphone is in use by another conversation.",
			Self::Worker => "The audio worker is unavailable. Restart Decodex.",
			Self::Prepare => "The selected microphone could not be configured.",
			Self::Start => "The microphone could not start recording.",
			Self::Cancelled => "Recording was cancelled.",
			Self::Capture => "Microphone audio conversion failed.",
		}
	}
}

enum Command {
	#[cfg(test)]
	CheckPrepared(Mode, SyncSender<bool>),
	#[cfg(test)]
	CheckCaptureChannels(SyncSender<Option<u32>>),
	Prepare(u32),
	Start {
		key: Key,
		record: bool,
		state: Arc<State>,
		commands: Sender<Self>,
		reply: SyncSender<Result<Session, AudioError>>,
	},
	Resume {
		state: Arc<State>,
		reply: SyncSender<Result<(), AudioError>>,
	},
	Pause {
		state: Arc<State>,
		reply: SyncSender<Result<bool, AudioError>>,
	},
	Release {
		state: Arc<State>,
		pcm: Pcm,
	},
}

pub(in crate::shell::agent_surface::voice) struct State {
	cancelled: Arc<AtomicBool>,
	running: AtomicBool,
	level: AtomicU32,
}
impl State {
	pub(in crate::shell::agent_surface::voice) fn running(&self) -> bool {
		self.running.load(Ordering::Acquire)
	}

	pub(in crate::shell::agent_surface::voice) fn level(&self) -> f32 {
		f32::from_bits(self.level.load(Ordering::Relaxed))
	}
}

pub(in crate::shell::agent_surface::voice) struct Session {
	state: Arc<State>,
	pcm: Option<Pcm>,
	owner: Sender<Command>,
	factory: Option<PeerConnectionFactory>,
}
impl Session {
	/// Called on a capture/transport worker, never on the UI thread.
	pub(in crate::shell::agent_surface::voice) fn start(
		device: u32,
		mode: Mode,
		cancelled: Arc<AtomicBool>,
	) -> Result<Self, AudioError> {
		owner()?.start(Key { device, mode }, cancelled, true)
	}

	/// Reserve Live's prepared factory before signaling, without enabling I/O.
	pub(in crate::shell::agent_surface::voice) fn prepare_live(
		device: u32,
		cancelled: Arc<AtomicBool>,
	) -> Result<Self, AudioError> {
		owner()?.start(Key { device, mode: Mode::Live }, cancelled, false)
	}

	pub(in crate::shell::agent_surface::voice) fn live_factory(&self) -> PeerConnectionFactory {
		self.factory.as_ref().expect("Live session factory").clone()
	}

	pub(in crate::shell::agent_surface::voice) fn resume(&self) -> Result<(), AudioError> {
		let (reply, done) = mpsc::sync_channel(1);

		self.owner
			.send(Command::Resume { state: self.state.clone(), reply })
			.map_err(|_| AudioError::Worker)?;

		done.recv().map_err(|_| AudioError::Worker)?
	}

	pub(in crate::shell::agent_surface::voice) fn parts(&mut self) -> (Arc<State>, &mut Pcm) {
		(self.state.clone(), self.pcm.as_mut().expect("leased PCM"))
	}

	/// Stop hardware before draining the final dictation samples.
	pub(in crate::shell::agent_surface::voice) fn pause(&self) -> Result<bool, AudioError> {
		let (reply, done) = mpsc::sync_channel(1);

		self.owner
			.send(Command::Pause { state: self.state.clone(), reply })
			.map_err(|_| AudioError::Worker)?;

		done.recv().map_err(|_| AudioError::Worker)?
	}
}

impl Drop for Session {
	fn drop(&mut self) {
		self.state.cancelled.store(true, Ordering::Release);

		if let Some(pcm) = self.pcm.take() {
			let _ = self.owner.send(Command::Release { state: self.state.clone(), pcm });
		}
	}
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct Key {
	device: u32,
	mode: Mode,
}

struct Owner {
	commands: Sender<Command>,
}
impl Owner {
	fn new() -> Result<Self, AudioError> {
		let (commands, inbox) = mpsc::channel();

		Builder::new()
			.name("native-audio".into())
			.spawn(move || worker(inbox))
			.map_err(|_| AudioError::Worker)?;

		Ok(Self { commands })
	}

	fn start(
		&self,
		key: Key,
		cancelled: Arc<AtomicBool>,
		record: bool,
	) -> Result<Session, AudioError> {
		let state = Arc::new(State {
			cancelled,
			running: AtomicBool::new(false),
			level: AtomicU32::new(0),
		});
		let (reply, done) = mpsc::sync_channel(1);

		self.commands
			.send(Command::Start {
				key,
				record,
				state: state.clone(),
				commands: self.commands.clone(),
				reply,
			})
			.map_err(|_| AudioError::Worker)?;

		let result = done.recv().map_err(|_| AudioError::Worker)?;

		if result.is_err() {
			state.cancelled.store(true, Ordering::Release);
		}

		result
	}
}

struct Prepared {
	key: Key,
	engine: Option<Device>,
	pcm: Option<Pcm>,
	state: Option<Arc<State>>,
	paused: bool,
	recovery: Option<(Instant, bool, Instant)>,
}
impl Prepared {
	fn engine(&self) -> &Device {
		self.engine.as_ref().expect("prepared device")
	}

	fn follow_route(&mut self) -> Result<bool, ()> {
		let (input, output) = self.engine.as_mut().ok_or(())?.route.update()?;

		if input && self.key.mode == Mode::Dictation {
			self.rebuild()?;
		}
		// The native WebRTC device follows its selected route and owns Live recovery.
		if input || output {
			eprintln!("DecodexAudio route_change input={input} output={output}");
		}

		Ok(input || (output && self.key.mode == Mode::Live))
	}

	fn rebuild(&mut self) -> Result<(), ()> {
		self.engine = Some(Device::rebuild(self.engine.take().ok_or(())?)?);
		self.recovery = None;

		Ok(())
	}

	fn matches(&self, state: &Arc<State>) -> bool {
		self.state.as_ref().is_some_and(|active| Arc::ptr_eq(active, state))
	}

	/// Keep session queues and network state alive across Apple's hardware reconfiguration.
	fn maintain_route(&mut self) -> bool {
		let Ok(route_changed) = self.follow_route() else {
			return false;
		};
		let changed = route_changed;
		let Some(engine) = self.engine.as_mut() else {
			return false;
		};

		if self.state.as_ref().is_some_and(|state| state.cancelled.load(Ordering::Acquire)) {
			engine.pause();

			return true;
		}
		if engine.running() {
			self.recovery = None;

			return true;
		}

		let now = Instant::now();
		let (since, notified, next_attempt) = self.recovery.get_or_insert((now, changed, now));

		*notified |= changed;

		// isRunning can turn false before the asynchronous configuration notification arrives.
		let deadline = if *notified { Duration::from_secs(2) } else { Duration::from_millis(250) };

		if now.duration_since(*since) >= deadline {
			return false;
		}
		if *notified && now >= *next_attempt {
			*next_attempt = now + Duration::from_millis(50);

			if engine.resume().is_ok() {
				eprintln!(
					"DecodexAudio route_resumed_ms={:.3}",
					since.elapsed().as_secs_f64() * 1_000.
				);
			}
		}

		true
	}

	fn pause(&self) {
		if let Some(engine) = &self.engine {
			if self.state.as_ref().is_some_and(|state| state.cancelled.load(Ordering::Acquire)) {
				engine.cancel();
			} else {
				engine.pause();
			}
		}
		if let Some(state) = &self.state {
			state.running.store(false, Ordering::Release);
			state.level.store(0, Ordering::Relaxed);
		}
	}
}

impl Drop for Prepared {
	fn drop(&mut self) {
		self.pause();
	}
}

pub(in crate::shell::agent_surface::voice) fn warm(device: u32) {
	if let Ok(owner) = owner() {
		let _ = owner.commands.send(Command::Prepare(device));
	}
}

fn prepare(cached: &mut Option<Prepared>, key: Key) -> Result<&mut Prepared, AudioError> {
	if cached.as_ref().is_none_or(|graph| graph.key != key || graph.pcm.is_none()) {
		*cached = None;

		let (engine, pcm) = Device::prepare(key.device, key.mode.rate(), key.mode == Mode::Live)
			.map_err(|()| AudioError::Prepare)?;

		*cached = Some(Prepared {
			key,
			engine: Some(engine),
			pcm: Some(pcm),
			state: None,
			paused: true,
			recovery: None,
		});
	}

	Ok(cached.as_mut().expect("prepared graph"))
}

fn start(
	cached: &mut Option<Prepared>,
	key: Key,
	state: Arc<State>,
	commands: &Sender<Command>,
	record: bool,
) -> Result<Session, AudioError> {
	if state.cancelled.load(Ordering::Acquire) {
		return Err(AudioError::Cancelled);
	}

	let graph = prepare(cached, key)?;

	if state.cancelled.load(Ordering::Acquire) {
		return Err(AudioError::Cancelled);
	}
	if record {
		resume(graph, &state)?;
	}

	graph.paused = !record;
	graph.state = Some(state.clone());

	Ok(Session {
		state,
		pcm: graph.pcm.take(),
		owner: commands.clone(),
		factory: graph.engine().live_factory(),
	})
}

fn resume(graph: &mut Prepared, state: &Arc<State>) -> Result<(), AudioError> {
	if state.cancelled.load(Ordering::Acquire) {
		return Err(AudioError::Cancelled);
	}

	graph.follow_route().map_err(|()| AudioError::Prepare)?;

	if state.cancelled.load(Ordering::Acquire) {
		return Err(AudioError::Cancelled);
	}

	graph.engine().reset_capture().map_err(|()| AudioError::Start)?;

	if let Some(pcm) = graph.pcm.as_mut() {
		while pcm.captured.pop().is_ok() {}
	}

	graph.engine().clear_playback().map_err(|()| AudioError::Start)?;

	graph.recovery = None;
	graph.paused = false;

	graph.engine().resume().map_err(|()| AudioError::Start)?;
	state.running.store(true, Ordering::Release);

	Ok(())
}

fn worker(inbox: Receiver<Command>) {
	// Keep each prepared resource, but lease only one session across both modes.
	let mut cached: [Option<Prepared>; 2] = [None, None];

	loop {
		let active = cached.iter().flatten().any(|graph| graph.state.is_some());
		let command = if active {
			inbox.recv_timeout(Duration::from_millis(5))
		} else {
			inbox.recv().map_err(|_| RecvTimeoutError::Disconnected)
		};

		match command {
			#[cfg(test)]
			Ok(Command::CheckCaptureChannels(reply)) => {
				let channels = cached[Mode::Dictation.slot()]
					.as_ref()
					.filter(|graph| graph.key.mode == Mode::Dictation)
					.map(|graph| graph.engine().capture_channels().expect("input format"));
				let _ = reply.send(channels);
			},
			#[cfg(test)]
			Ok(Command::CheckPrepared(mode, reply)) => {
				let quiet = cached[mode.slot()]
					.as_ref()
					.is_some_and(|graph| !graph.engine().running() && graph.pcm.is_some());
				let _ = reply.send(quiet);
			},
			Ok(Command::Prepare(device)) =>
				if !cached.iter().flatten().any(|graph| graph.state.is_some()) {
					for mode in [Mode::Dictation, Mode::Live] {
						let _ = prepare(&mut cached[mode.slot()], Key { device, mode });
					}
				},
			Ok(Command::Start { key, record, state, commands, reply }) => {
				let busy = cached.iter().flatten().any(|graph| {
					graph
						.state
						.as_ref()
						.is_some_and(|active| !active.cancelled.load(Ordering::Acquire))
				});
				let result = if busy {
					Err(AudioError::Busy)
				} else {
					for graph in cached.iter().flatten() {
						if graph.engine().running() {
							graph.pause();
						}
					}

					let slot = &mut cached[key.mode.slot()];
					let result = start(slot, key, state, &commands, record);

					if matches!(result, Err(AudioError::Prepare | AudioError::Start)) {
						*slot = None;
					}

					result
				};
				let _ = reply.send(result);
			},
			Ok(Command::Resume { state, reply }) => {
				let result = resume_cached_session(&mut cached, &state);
				let _ = reply.send(result);
			},
			Ok(Command::Pause { state, reply }) => {
				let drained = pause_cached_session(&mut cached, &state);
				let _ = reply.send(drained);
			},
			Ok(Command::Release { state, pcm }) => {
				if let Some(slot) = cached
					.iter_mut()
					.find(|slot| slot.as_ref().is_some_and(|graph| graph.matches(&state)))
				{
					let graph = slot.as_mut().expect("matching session");

					graph.pause();

					if graph.key.mode == Mode::Live {
						// WebRTC's last peer terminates its platform ADM. Consume each
						// prepared factory once; prime the next while transport closes.
						let key = graph.key;

						*slot = None;

						let _ = prepare(slot, key);
					} else {
						graph.pcm = Some(pcm);
						graph.state = None;
					}
				}
			},
			Err(RecvTimeoutError::Disconnected) => break,
			Err(RecvTimeoutError::Timeout) => {},
		}

		for slot in &mut cached {
			if let Some(graph) = slot
				&& let Some(state) = graph.state.clone()
			{
				if graph.paused || state.cancelled.load(Ordering::Acquire) {
					if graph.engine().running() {
						graph.pause();
					}
				} else if !graph.maintain_route() {
					state.running.store(false, Ordering::Release);

					*slot = None;
				} else {
					state.level.store(graph.engine().level().to_bits(), Ordering::Relaxed);
				}
			}
		}
	}
}

fn resume_cached_session(
	cached: &mut [Option<Prepared>; 2],
	state: &Arc<State>,
) -> Result<(), AudioError> {
	cached
		.iter_mut()
		.flatten()
		.find(|graph| graph.matches(state))
		.ok_or(AudioError::Cancelled)
		.and_then(|graph| resume(graph, state))
}

fn pause_cached_session(
	cached: &mut [Option<Prepared>; 2],
	state: &Arc<State>,
) -> Result<bool, AudioError> {
	let drained =
		if let Some(graph) = cached.iter_mut().flatten().find(|graph| graph.matches(state)) {
			graph.paused = true;

			graph.pause();

			graph
				.engine
				.as_mut()
				.expect("prepared device")
				.finish_capture()
				.map_err(|()| AudioError::Capture)
		} else {
			Ok(true)
		};

	if !matches!(drained, Ok(false)) {
		state.cancelled.store(true, Ordering::Release);
	}

	drained
}

fn owner() -> Result<&'static Owner, AudioError> {
	static OWNER: OnceLock<Result<Owner, AudioError>> = OnceLock::new();

	OWNER.get_or_init(Owner::new).as_ref().map_err(|error| *error)
}
#[cfg(test)]
#[path = "native_audio_owner_tests.rs"]
mod tests;
