//! Sole owner of the application's Apple audio graph. Network work never runs here.
//!
//! A session leases PCM queues, not the thread-bound engine. Cancellation pauses hardware
//! independently of transport teardown. Returned queues permit warm reuse; a new session
//! that overtakes teardown gets a fresh graph. Identity checks discard late old releases.
//! Standard channels and atomics provide the existing process-local lifecycle; no async
//! runtime or alternate capture backend is added. Hardware route changes stay in this owner.
use super::{Device, Pcm};
use std::{
	sync::{
		Arc, OnceLock,
		atomic::{AtomicBool, AtomicU32, Ordering},
		mpsc::{self, Receiver, Sender, SyncSender},
	},
	thread,
	time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::shell::agent_surface::voice) enum Mode {
	Dictation,
	Live,
}
impl Mode {
	fn index(self) -> usize {
		match self {
			Self::Dictation => 0,
			Self::Live => 1,
		}
	}

	fn rate(self) -> f64 {
		match self {
			Self::Dictation => f64::from(decodex_protocol::DICTATION_SAMPLE_RATE),
			Self::Live => f64::from(super::LIVE_SAMPLE_RATE),
		}
	}
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
	device: u32,
	mode: Mode,
}
enum Command {
	#[cfg(test)]
	CheckPrepared(SyncSender<bool>),
	Prepare(Key),
	Start {
		key: Key,
		state: Arc<State>,
		commands: Sender<Self>,
		reply: SyncSender<Result<Session, AudioError>>,
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
}
impl Session {
	/// Called on a capture/transport worker, never on the UI thread.
	pub(in crate::shell::agent_surface::voice) fn start(
		device: u32,
		mode: Mode,
		cancelled: Arc<AtomicBool>,
	) -> Result<Self, AudioError> {
		owner()?.start(Key { device, mode }, cancelled)
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
struct Owner {
	commands: Sender<Command>,
}
impl Owner {
	fn new() -> Result<Self, AudioError> {
		let (commands, inbox) = mpsc::channel();
		thread::Builder::new()
			.name("native-audio".into())
			.spawn(move || worker(inbox))
			.map_err(|_| AudioError::Worker)?;
		Ok(Self { commands })
	}

	fn start(&self, key: Key, cancelled: Arc<AtomicBool>) -> Result<Session, AudioError> {
		let state = Arc::new(State {
			cancelled,
			running: AtomicBool::new(false),
			level: AtomicU32::new(0),
		});
		let (reply, done) = mpsc::sync_channel(1);
		self.commands
			.send(Command::Start {
				key,
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
		} else if (input || output) && self.key.mode == Mode::Live {
			// VoiceProcessingIO converts hardware formats to the fixed client format.
			// Keep its DSP graph when system input or output changes.
			self.engine().stop_for_route();
			self.recovery = None;
		}
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
		let configuration_changed = self.engine().configuration_changed();
		if !route_changed
			&& configuration_changed
			&& !self.engine().running()
			&& self.key.mode == Mode::Dictation
			&& self.rebuild().is_err()
		{
			return false;
		}
		let changed = configuration_changed || route_changed;
		let Some(engine) = self.engine.as_mut() else {
			return false;
		};
		if self.state.as_ref().is_some_and(|state| state.cancelled.load(Ordering::Acquire)) {
			engine.pause();
			return true;
		}
		if engine.pump().is_err() {
			return false;
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
					since.elapsed().as_secs_f64() * 1000.
				);
			}
		}
		true
	}

	fn pause(&self) {
		if let Some(engine) = &self.engine {
			engine.pause();
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
) -> Result<Session, AudioError> {
	if state.cancelled.load(Ordering::Acquire) {
		return Err(AudioError::Cancelled);
	}
	let graph = prepare(cached, key)?;
	if state.cancelled.load(Ordering::Acquire) {
		return Err(AudioError::Cancelled);
	}
	while graph.pcm.as_mut().expect("available PCM").captured.pop().is_ok() {}
	graph.engine().clear_playback().map_err(|()| AudioError::Start)?;
	graph.follow_route().map_err(|()| AudioError::Prepare)?;
	if graph.engine().configuration_changed() && key.mode == Mode::Dictation {
		graph.rebuild().map_err(|()| AudioError::Prepare)?;
	}
	if state.cancelled.load(Ordering::Acquire) {
		return Err(AudioError::Cancelled);
	}
	graph.engine.as_mut().expect("prepared device").reset_capture();
	graph.recovery = None;
	graph.paused = false;
	graph.engine().resume().map_err(|()| AudioError::Start)?;
	state.running.store(true, Ordering::Release);
	graph.state = Some(state.clone());
	Ok(Session { state, pcm: graph.pcm.take(), owner: commands.clone() })
}
fn worker(inbox: Receiver<Command>) {
	// Two fixed modes retain preparation work, but admission and hardware activation
	// remain global. Cached graphs never record in parallel.
	let mut cached: [Option<Prepared>; 2] = [None, None];
	loop {
		let active = cached.iter().flatten().any(|graph| graph.state.is_some());
		let command = if active {
			inbox.recv_timeout(Duration::from_millis(5))
		} else {
			inbox.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
		};
		match command {
			#[cfg(test)]
			Ok(Command::CheckPrepared(reply)) => {
				let quiet = cached.iter().all(|slot| {
					slot.as_ref().is_some_and(|graph| {
						!graph.engine().running()
							&& graph.pcm.as_ref().is_some_and(|pcm| pcm.captured.is_empty())
					})
				});
				let _ = reply.send(quiet);
			},
			Ok(Command::Prepare(key)) => {
				if !cached.iter().flatten().any(|graph| graph.state.is_some()) {
					let _ = prepare(&mut cached[key.mode.index()], key);
				}
			},
			Ok(Command::Start { key, state, commands, reply }) => {
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
					let slot = &mut cached[key.mode.index()];
					let result = start(slot, key, state, &commands);
					if matches!(result, Err(AudioError::Prepare | AudioError::Start)) {
						*slot = None;
					}
					result
				};
				let _ = reply.send(result);
			},
			Ok(Command::Pause { state, reply }) => {
				let drained = if let Some(graph) =
					cached.iter_mut().flatten().find(|graph| graph.matches(&state))
				{
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
				let _ = reply.send(drained);
			},
			Ok(Command::Release { state, pcm }) => {
				if let Some(graph) = cached.iter_mut().flatten().find(|graph| graph.matches(&state))
				{
					graph.pause();
					graph.pcm = Some(pcm);
					graph.state = None;
				}
			},
			Err(mpsc::RecvTimeoutError::Disconnected) => break,
			Err(mpsc::RecvTimeoutError::Timeout) => {},
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
fn owner() -> Result<&'static Owner, AudioError> {
	static OWNER: OnceLock<Result<Owner, AudioError>> = OnceLock::new();
	OWNER.get_or_init(Owner::new).as_ref().map_err(|error| *error)
}
pub(in crate::shell::agent_surface::voice) fn warm(device: u32) {
	if let Ok(owner) = owner() {
		for mode in [Mode::Dictation, Mode::Live] {
			let _ = owner.commands.send(Command::Prepare(Key { device, mode }));
		}
	}
}
#[cfg(test)]
#[path = "native_audio_owner_tests.rs"]
mod tests;
