//! Sole owner of the application's Apple audio graph. Network work never runs here.
//!
//! A session leases PCM queues, not the thread-bound engine. Cancellation pauses hardware
//! independently of transport teardown. Returned queues permit warm reuse; a new session
//! that overtakes teardown gets a fresh graph. Identity checks discard late old releases.
//! Standard channels and atomics provide the existing process-local lifecycle; no async
//! runtime, automatic retry or alternate capture backend is added.
use super::{Device, Pcm};
use std::{
	sync::{
		Arc, OnceLock,
		atomic::{AtomicBool, AtomicU32, Ordering},
		mpsc::{self, Receiver, Sender, SyncSender},
	},
	thread,
	time::Duration,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::shell::agent_surface::voice) enum Mode {
	Dictation,
	Live,
}
impl Mode {
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
}
impl AudioError {
	pub(in crate::shell::agent_surface::voice) fn message(self) -> &'static str {
		match self {
			Self::Busy => "The microphone is in use by another conversation.",
			Self::Worker => "The audio worker is unavailable. Restart Decodex.",
			Self::Prepare => "The selected microphone could not be configured.",
			Self::Start => "The microphone could not start recording.",
			Self::Cancelled => "Recording was cancelled.",
		}
	}
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
	device: u32,
	mode: Mode,
}
enum Command {
	Prepare(Key),
	Start {
		key: Key,
		state: Arc<State>,
		commands: Sender<Self>,
		reply: SyncSender<Result<Session, AudioError>>,
	},
	Pause {
		state: Arc<State>,
		reply: SyncSender<()>,
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
	pub(in crate::shell::agent_surface::voice) fn pause(&self) -> Result<(), AudioError> {
		let (reply, done) = mpsc::sync_channel(1);
		self.owner
			.send(Command::Pause { state: self.state.clone(), reply })
			.map_err(|_| AudioError::Worker)?;
		done.recv().map_err(|_| AudioError::Worker)
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
	engine: Device,
	pcm: Option<Pcm>,
	state: Option<Arc<State>>,
}
impl Prepared {
	fn matches(&self, state: &Arc<State>) -> bool {
		self.state.as_ref().is_some_and(|active| Arc::ptr_eq(active, state))
	}

	fn pause(&self) {
		self.engine.pause();
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
		let (engine, pcm) =
			Device::prepare(key.device, key.mode.rate()).map_err(|()| AudioError::Prepare)?;
		*cached = Some(Prepared { key, engine, pcm: Some(pcm), state: None });
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
	if cached
		.as_ref()
		.and_then(|graph| graph.state.as_ref())
		.is_some_and(|active| !active.cancelled.load(Ordering::Acquire))
	{
		return Err(AudioError::Busy);
	}
	let graph = prepare(cached, key)?;
	if state.cancelled.load(Ordering::Acquire) {
		return Err(AudioError::Cancelled);
	}
	let pcm = graph.pcm.as_mut().expect("available PCM");
	while pcm.captured.pop().is_ok() {}
	graph.engine.clear_playback().map_err(|()| AudioError::Start)?;
	graph.engine.resume().map_err(|()| AudioError::Start)?;
	state.running.store(true, Ordering::Release);
	graph.state = Some(state.clone());
	Ok(Session { state, pcm: graph.pcm.take(), owner: commands.clone() })
}
fn worker(inbox: Receiver<Command>) {
	let mut cached: Option<Prepared> = None;
	loop {
		let active = cached.as_ref().and_then(|graph| graph.state.as_ref()).is_some();
		let command = if active {
			inbox.recv_timeout(Duration::from_millis(5))
		} else {
			inbox.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
		};
		match command {
			Ok(Command::Prepare(key)) =>
				if cached.as_ref().is_none_or(|graph| graph.state.is_none()) {
					let _ = prepare(&mut cached, key);
				},
			Ok(Command::Start { key, state, commands, reply }) => {
				let result = start(&mut cached, key, state, &commands);
				if matches!(result, Err(AudioError::Prepare | AudioError::Start)) {
					cached = None;
				}
				let _ = reply.send(result);
			},
			Ok(Command::Pause { state, reply }) => {
				state.cancelled.store(true, Ordering::Release);
				if let Some(graph) = &cached
					&& graph.matches(&state)
				{
					graph.pause();
				}
				let _ = reply.send(());
			},
			Ok(Command::Release { state, pcm }) => {
				if let Some(graph) = &mut cached
					&& graph.matches(&state)
				{
					graph.pause();
					graph.pcm = Some(pcm);
					graph.state = None;
				}
			},
			Err(mpsc::RecvTimeoutError::Disconnected) => break,
			Err(mpsc::RecvTimeoutError::Timeout) => {},
		}
		if let Some(graph) = &cached
			&& let Some(state) = &graph.state
		{
			if state.cancelled.load(Ordering::Acquire) {
				if graph.engine.running() {
					graph.pause();
				}
			} else if !graph.engine.running() {
				state.running.store(false, Ordering::Release);
				// A failed or externally reconfigured graph must not enter the warm cache.
				cached = None;
			} else {
				state.level.store(graph.engine.level().to_bits(), Ordering::Relaxed);
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
		let _ = owner.commands.send(Command::Prepare(Key { device, mode: Mode::Dictation }));
	}
}
#[cfg(test)]
#[path = "native_audio_owner_tests.rs"]
mod tests;
