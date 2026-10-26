//! Native audio resources: input-only dictation and WebRTC's duplex voice device.
#![cfg_attr(test, allow(dead_code))]
#[path = "native_audio_owner.rs"] pub(super) mod owner;

#[path = "native_input_queue.rs"] mod input;
#[path = "native_live_audio.rs"] mod live;
#[path = "native_audio_route.rs"] mod route;

use std::{
	ptr::NonNull,
	rc::Rc,
	slice,
	sync::{
		Arc, Mutex,
		atomic::{AtomicU32, Ordering},
	},
	time::Instant,
};

use libwebrtc::peer_connection_factory::PeerConnectionFactory;
use objc2_core_audio_types::AudioBufferList;
use rtrb::{Consumer, Producer, RingBuffer};

use route::Route;

pub(super) const LIVE_SAMPLE_RATE: u32 = 48_000;
pub(super) const LIVE_FRAME_SAMPLES: u32 = LIVE_SAMPLE_RATE / 100;

pub(super) struct Pcm {
	pub(super) captured: Consumer<f32>,
	pub(super) playback: Producer<f32>,
}

pub(super) struct Device {
	hardware: Hardware,
	sample_rate: f64,
	route: Route,
	ports: Ports,
	_thread: std::marker::PhantomData<Rc<()>>,
}
impl Device {
	fn resume(&self) -> Result<(), ()> {
		match &self.hardware {
			Hardware::Input(input) => input.start(),
			Hardware::Live(live) => live.start(),
		}
	}

	pub(super) fn pause(&self) {
		match &self.hardware {
			Hardware::Input(input) => input.stop(),
			Hardware::Live(live) => live.stop(),
		}
	}

	fn cancel(&self) {
		match &self.hardware {
			Hardware::Input(input) => input.cancel(),
			Hardware::Live(live) => live.stop(),
		}
	}

	fn prepare(device: u32, sample_rate: f64, duplex: bool) -> Result<(Self, Pcm), ()> {
		let (captured, input) = RingBuffer::new(if duplex { 4_800 } else { 19_200 });
		let (playback, output) = RingBuffer::new(4_800);
		let ports = Ports {
			captured: Arc::new(Mutex::new(captured)),
			output: Arc::new(Mutex::new(output)),
			level: Arc::new(AtomicU32::new(0)),
		};

		Ok((Self::build(device, sample_rate, duplex, ports)?, Pcm { captured: input, playback }))
	}

	fn rebuild(old: Self) -> Result<Self, ()> {
		let started = Instant::now();
		let ports = old.ports.clone();

		ports.level.store(0, Ordering::Relaxed);

		let device = old.route.requested();
		let rate = old.sample_rate;
		let duplex = matches!(old.hardware, Hardware::Live(_));

		drop(old);

		let rebuilt = Self::build(device, rate, duplex, ports);

		eprintln!("DecodexAudio route_build_ms={:.3}", started.elapsed().as_secs_f64() * 1_000.);

		rebuilt
	}

	fn build(device: u32, sample_rate: f64, duplex: bool, ports: Ports) -> Result<Self, ()> {
		let started = Instant::now();
		let route = Route::new(device)?;
		let hardware = if duplex {
			Hardware::Live(live::Live::new(device)?)
		} else {
			Hardware::Input(input::Input::new(&route, sample_rate, ports.clone())?)
		};

		eprintln!(
			"DecodexAudio device_open input={device} duplex={duplex} prepare_ms={:.3}",
			started.elapsed().as_secs_f64() * 1_000.
		);

		Ok(Self { hardware, sample_rate, route, ports, _thread: std::marker::PhantomData })
	}

	fn finish_capture(&self) -> Result<bool, ()> {
		match &self.hardware {
			Hardware::Input(input) => input.finished(),
			Hardware::Live(_) => Ok(true),
		}
	}

	fn reset_capture(&self) -> Result<(), ()> {
		match &self.hardware {
			Hardware::Input(input) => input.reset(),
			Hardware::Live(_) => Ok(()),
		}
	}

	fn clear_playback(&self) -> Result<(), ()> {
		let mut output = self.ports.output.lock().map_err(|_| ())?;

		while output.pop().is_ok() {}

		Ok(())
	}

	pub(super) fn running(&self) -> bool {
		match &self.hardware {
			Hardware::Input(input) => input.running().unwrap_or(false),
			Hardware::Live(live) => live.running(),
		}
	}

	pub(super) fn level(&self) -> f32 {
		f32::from_bits(self.ports.level.load(Ordering::Relaxed))
	}

	fn live_factory(&self) -> Option<PeerConnectionFactory> {
		match &self.hardware {
			Hardware::Live(live) => Some(live.factory()),
			Hardware::Input(_) => None,
		}
	}

	#[cfg(test)]
	fn capture_channels(&self) -> Option<u32> {
		match &self.hardware {
			Hardware::Input(input) => input.channels(),
			Hardware::Live(_) => None,
		}
	}
}

impl Drop for Device {
	fn drop(&mut self) {
		eprintln!(
			"DecodexAudio device_close input={} duplex={}",
			self.route.requested(),
			matches!(self.hardware, Hardware::Live(_))
		);
	}
}

#[derive(Clone)]
struct Ports {
	captured: Arc<Mutex<Producer<f32>>>,
	output: Arc<Mutex<Consumer<f32>>>,
	level: Arc<AtomicU32>,
}

enum Hardware {
	Input(input::Input),
	Live(live::Live),
}

/// # Safety
/// The audio graph must supply a valid buffer list and Float32 sample storage for this call.
unsafe fn capture_audio(
	count: u32,
	buffers: NonNull<AudioBufferList>,
	capture: &Mutex<Producer<f32>>,
	captured_level: &AtomicU32,
) -> i32 {
	// The graph supplies non-interleaved Float32 mono samples at the session rate.
	let buffers = unsafe { buffers.as_ref() };

	if buffers.mNumberBuffers == 0 {
		return -50;
	}

	let channels = unsafe {
		slice::from_raw_parts(buffers.mBuffers.as_ptr(), buffers.mNumberBuffers as usize)
	};

	if channels.iter().any(|buffer| {
		buffer.mData.is_null()
			|| (buffer.mDataByteSize as usize) < count as usize * size_of::<f32>()
			|| buffer.mNumberChannels != 1
	}) {
		return -50;
	}

	if let Ok(mut capture) = capture.try_lock() {
		let mut energy = 0.0;

		for frame in 0..count as usize {
			let sample = channels
				.iter()
				.map(|buffer| unsafe { *buffer.mData.cast::<f32>().add(frame) })
				.sum::<f32>()
				/ channels.len() as f32;
			let sample = if sample.is_finite() { sample.clamp(-1., 1.) } else { 0. };

			energy += sample * sample;

			let _ = capture.push(sample);
		}

		captured_level
			.store((energy / count.max(1) as f32).sqrt().min(1.).to_bits(), Ordering::Relaxed);
	}

	0
}
