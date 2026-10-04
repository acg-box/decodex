//! Dictation uses the same Apple voice-processing sink as Live, on one audio owner thread.
use super::audio::{Device, Pcm};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
	cell::Cell,
	sync::{
		Arc, OnceLock,
		atomic::{AtomicBool, Ordering},
		mpsc::{self, Receiver, Sender, SyncSender},
	},
	thread,
	time::{Duration, Instant},
};

enum Command {
	Prepare(u32),
	Start {
		device: u32,
		requested: Instant,
		events: SyncSender<Value>,
		stopped: Arc<AtomicBool>,
		finished: Arc<AtomicBool>,
	},
}

pub(super) struct Capture {
	terminal: Cell<bool>,
	events: Receiver<Value>,
	stopped: Arc<AtomicBool>,
	finished: Arc<AtomicBool>,
}
impl Capture {
	pub(super) fn prepare(device: u32) {
		let _ = owner().and_then(|tx| tx.send(Command::Prepare(device)).map_err(|_| ()));
	}

	pub(super) fn start(device: u32) -> Result<Self, ()> {
		let (tx, events) = mpsc::sync_channel(128);
		let stopped = Arc::new(AtomicBool::new(false));
		let finished = Arc::new(AtomicBool::new(false));
		owner()?
			.send(Command::Start {
				device,
				requested: Instant::now(),
				events: tx,
				stopped: stopped.clone(),
				finished: finished.clone(),
			})
			.map_err(|_| ())?;
		Ok(Self { terminal: Cell::new(false), events, stopped, finished })
	}

	pub(super) fn poll(&self) -> Option<Value> {
		if self.terminal.get() {
			return None;
		}
		let event = match self.events.try_recv() {
			Ok(event) => event,
			Err(mpsc::TryRecvError::Empty) => return None,
			Err(mpsc::TryRecvError::Disconnected) =>
				json!({"type":"error","message":"Microphone audio delivery stopped."}),
		};
		if matches!(event["type"].as_str(), Some("ended" | "error")) {
			self.terminal.set(true);
		}
		Some(event)
	}

	pub(super) fn finish(&self) {
		self.finished.store(true, Ordering::Release);
	}
}
impl Drop for Capture {
	fn drop(&mut self) {
		self.stopped.store(true, Ordering::Release);
	}
}

fn owner() -> Result<&'static Sender<Command>, ()> {
	static OWNER: OnceLock<Result<Sender<Command>, ()>> = OnceLock::new();
	OWNER.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new().name("dictation-audio".into()).spawn(move || {
            let mut prepared: Option<(u32, Device, Pcm)> = None;
            while let Ok(command) = rx.recv() {
                let device = match &command { Command::Prepare(id) | Command::Start { device: id, .. } => *id };
                if let Command::Start { stopped, .. } = &command {
                    if stopped.load(Ordering::Acquire) { continue; }
                }
                if prepared.as_ref().is_none_or(|(id, _, _)| *id != device) {
                    prepared = Device::prepare(device, 24_000.0).ok().map(|(engine, pcm)| (device, engine, pcm));
                }
                if let Command::Start { events, stopped, finished, requested, .. } = command {
                    let healthy = if let Some((_, engine, pcm)) = &mut prepared {
                        run(engine, pcm, &events, &stopped, &finished, requested)
                    } else { false };
                    if !healthy {
                        prepared = None;
                        let _ = events.try_send(json!({"type":"error","message":"Microphone capture stopped. Check the input device and try again."}));
                    }
                }
            }
        }).map_err(|_| ())?;
        Ok(tx)
    }).as_ref().map_err(|_| ())
}

fn run(
	engine: &Device,
	pcm: &mut Pcm,
	events: &SyncSender<Value>,
	stopped: &AtomicBool,
	finished: &AtomicBool,
	requested: Instant,
) -> bool {
	// No previous session audio crosses the capture boundary.
	while pcm.captured.pop().is_ok() {}
	if stopped.load(Ordering::Acquire) {
		return true;
	}
	if finished.load(Ordering::Acquire) {
		return events.try_send(json!({"type":"ended"})).is_ok();
	}
	if engine.resume().is_err() {
		return false;
	}
	let mut first = true;
	let mut bytes = Vec::with_capacity(9_600);
	let mut healthy = true;
	loop {
		if stopped.load(Ordering::Acquire) {
			break;
		}
		let final_buffer = finished.load(Ordering::Acquire);
		if final_buffer {
			engine.pause();
		}
		while let Ok(sample) = pcm.captured.pop() {
			bytes.extend_from_slice(&pcm16(sample).to_le_bytes());
		}
		if first && !bytes.is_empty() {
			first = false;
			let elapsed = requested.elapsed().as_secs_f64() * 1_000.;
			eprintln!("DecodexAudio dictation_sink_first_pcm_ms={elapsed:.3}");
			if events.try_send(json!({"type":"dictation_ready","capture_ms":elapsed})).is_err() {
				healthy = false;
				break;
			}
		}
		if bytes.len() >= 960 || (final_buffer && !bytes.is_empty()) {
			let event = json!({"type":"pcm","audio":STANDARD.encode(&bytes),"level":(engine.level() * 5.).min(1.)});
			bytes.clear();
			if events.try_send(event).is_err() {
				healthy = false;
				break;
			}
		}
		if final_buffer {
			healthy = events.try_send(json!({"type":"ended"})).is_ok();
			break;
		}
		if !engine.running() {
			healthy = false;
			break;
		}
		thread::sleep(Duration::from_millis(5));
	}
	engine.pause();
	healthy
}

fn pcm16(sample: f32) -> i16 {
	let sample = if sample.is_finite() { sample.clamp(-1., 1.) } else { 0. };
	(sample * if sample < 0. { 32_768. } else { 32_767. }) as i16
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn capture_reports_worker_exit_but_not_after_normal_completion() {
		for normal in [false, true] {
			let (tx, events) = mpsc::sync_channel(1);
			if normal {
				tx.send(json!({"type":"ended"})).unwrap();
			}
			drop(tx);
			let capture = Capture {
				events,
				terminal: Cell::new(false),
				stopped: Arc::new(AtomicBool::new(false)),
				finished: Arc::new(AtomicBool::new(false)),
			};
			assert_eq!(capture.poll().unwrap()["type"], if normal { "ended" } else { "error" });
			assert!(capture.poll().is_none());
		}
	}

	#[test]
	fn pcm_preserves_microphone_amplitude_and_handles_nonfinite_samples() {
		assert_eq!(pcm16(0.25), 8191);
		assert_eq!(pcm16(-1.), i16::MIN);
		assert_eq!(pcm16(1.), i16::MAX);
		assert_eq!(pcm16(f32::NAN), 0);
	}
}
