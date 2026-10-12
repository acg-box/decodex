//! Dictation uses the same Apple voice-processing sink as Live, on one audio owner thread.
#![cfg_attr(test, allow(dead_code))]
use super::audio::owner::{self, AudioError, Mode, Session};
use decodex_protocol::DictationAudio;
use serde_json::{Value, json};
use std::{
	cell::Cell,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
		mpsc::{self, Receiver, SyncSender},
	},
	thread,
	time::{Duration, Instant},
};

pub(super) struct Capture {
	terminal: Cell<bool>,
	events: Receiver<Value>,
	stopped: Arc<AtomicBool>,
	finished: Arc<AtomicBool>,
}
impl Capture {
	pub(super) fn prepare(device: u32) {
		owner::warm(device);
	}

	pub(super) fn start(device: u32) -> Result<Self, AudioError> {
		let (tx, events) = mpsc::sync_channel(128);
		let stopped = Arc::new(AtomicBool::new(false));
		let finished = Arc::new(AtomicBool::new(false));
		let stop = stopped.clone();
		let finish = finished.clone();
		let requested = Instant::now();
		thread::Builder::new().name("dictation-capture".into()).spawn(move || {
            let mut audio = match Session::start(device, Mode::Dictation, stop.clone()) {
                Ok(audio) => audio,
                Err(error) => { let _ = tx.try_send(json!({"type":"error","message":error.message()})); return; }
            };
            if !run(&mut audio, &tx, &stop, &finish, requested) {
                let _ = tx.try_send(json!({"type":"error","message":"Microphone capture stopped. Check the input device and try again."}));
            }
        }).map_err(|_| AudioError::Worker)?;
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

fn run(
	audio: &mut Session,
	events: &SyncSender<Value>,
	stopped: &AtomicBool,
	finished: &AtomicBool,
	requested: Instant,
) -> bool {
	let mut first = true;
	let mut bytes = Vec::with_capacity(9_600);
	let mut healthy = true;
	let mut last_audio = Instant::now();
	loop {
		if stopped.load(Ordering::Acquire) {
			break;
		}
		let final_buffer = finished.load(Ordering::Acquire);
		let drained = if final_buffer {
			match audio.pause() {
				Ok(drained) => drained,
				Err(_) => return false,
			}
		} else {
			false
		};
		let (engine, pcm) = audio.parts();
		let before = bytes.len();
		drain_pcm(&mut pcm.captured, &mut bytes);
		if bytes.len() > before {
			last_audio = Instant::now();
		} else if !final_buffer && last_audio.elapsed() >= Duration::from_secs(3) {
			let message = if first {
				"No audio arrived from the microphone. Check its connection or choose another input, then try again."
			} else {
				"The microphone stopped delivering audio. Your received text remains in the draft. Check its connection and try again."
			};
			healthy = events.try_send(json!({"type":"error","message":message})).is_ok();
			break;
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
			let Ok(packet) = DictationAudio::from_pcm(std::mem::take(&mut bytes)) else {
				return false;
			};
			let event = json!({"type":"pcm","audio":packet,"level":(engine.level() * 5.).min(1.)});
			if events.try_send(event).is_err() {
				healthy = false;
				break;
			}
		}
		if final_buffer && drained && pcm.captured.is_empty() {
			healthy = events.try_send(json!({"type":"ended"})).is_ok();
			break;
		}
		if !final_buffer && !engine.running() {
			healthy = false;
			break;
		}
		thread::sleep(Duration::from_millis(5));
	}
	healthy
}

fn drain_pcm(input: &mut rtrb::Consumer<f32>, bytes: &mut Vec<u8>) {
	let available = (decodex_protocol::MAX_DICTATION_PCM_BYTES - bytes.len()) / 2;
	for _ in 0..available {
		let Ok(sample) = input.pop() else {
			break;
		};
		bytes.extend_from_slice(&pcm16(sample).to_le_bytes());
	}
}

fn pcm16(sample: f32) -> i16 {
	let sample = if sample.is_finite() { sample.clamp(-1., 1.) } else { 0. };
	(sample * if sample < 0. { 32_768. } else { 32_767. }) as i16
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	#[ignore = "Requires authorized microphone and DECODEX_AUDIO_TEST_DEVICE_ID"]
	fn physical_capture_becomes_ready_and_finishes_without_sending_a_turn() {
		let device = std::env::var("DECODEX_AUDIO_TEST_DEVICE_ID").unwrap().parse().unwrap();
		let capture = Capture::start(device).unwrap();
		let started = Instant::now();
		let mut ready = false;
		loop {
			assert!(
				started.elapsed() < Duration::from_secs(8),
				"capture must not wait indefinitely"
			);
			if let Some(event) = capture.poll() {
				assert_ne!(event["type"], "error", "{event}");
				if event["type"] == "dictation_ready" {
					ready = true;
					capture.finish();
				}
				if event["type"] == "ended" {
					assert!(ready);
					break;
				}
			}
			thread::sleep(Duration::from_millis(5));
		}
	}
	#[test]
	fn capture_burst_keeps_every_sample_within_the_wire_limit() {
		let (mut input, mut captured) = rtrb::RingBuffer::new(19_200);
		for index in 0..19_200 {
			input.push(index as f32 / 19_200.).unwrap();
		}
		let mut result = Vec::new();
		while !captured.is_empty() {
			let mut bytes = Vec::new();
			drain_pcm(&mut captured, &mut bytes);
			let packet = DictationAudio::from_pcm(bytes).unwrap();
			result.extend_from_slice(packet.pcm());
		}
		let expected: Vec<u8> =
			(0..19_200).flat_map(|index| pcm16(index as f32 / 19_200.).to_le_bytes()).collect();
		assert_eq!(result, expected);
	}

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
