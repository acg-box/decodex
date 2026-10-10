//! Apple voice processing with a direct, bounded PCM path to the native transport.
#![cfg_attr(test, allow(dead_code))]
#[path = "native_audio_owner.rs"] pub(super) mod owner;
use std::{
	ptr::NonNull,
	rc::Rc,
	slice,
	sync::{
		Arc, Mutex,
		atomic::{AtomicU32, Ordering},
	},
};

use block2::RcBlock;
use objc2::{AnyThread, rc::Retained, runtime::Bool};
use objc2_audio_toolbox::AUAudioUnit;
use objc2_avf_audio::{
	AVAudioEngine, AVAudioFormat, AVAudioSinkNode, AVAudioSourceNode,
	AVAudioVoiceProcessingOtherAudioDuckingConfiguration,
	AVAudioVoiceProcessingOtherAudioDuckingLevel,
};
use objc2_core_audio_types::{AudioBufferList, AudioTimeStamp};
use rtrb::{Consumer, Producer, RingBuffer};

pub(super) const LIVE_SAMPLE_RATE: u32 = 48_000;
pub(super) const LIVE_FRAME_SAMPLES: u32 = LIVE_SAMPLE_RATE / 100;

pub(super) struct Pcm {
	pub(super) captured: Consumer<f32>,
	pub(super) playback: Producer<f32>,
}

pub(super) struct Device {
	engine: Retained<AVAudioEngine>,
	sink: Retained<AVAudioSinkNode>,
	source: Retained<AVAudioSourceNode>,
	output: Arc<Mutex<Consumer<f32>>>,
	level: Arc<AtomicU32>,
	_thread: std::marker::PhantomData<Rc<()>>,
}
impl Device {
	fn resume(&self) -> Result<(), ()> {
		unsafe {
			self.engine
				.startAndReturnError()
				.map_err(|error| eprintln!("DecodexAudio engine_start_failed: {error}"))
		}
	}

	pub(super) fn pause(&self) {
		unsafe {
			self.engine.pause();
		}
	}

	fn prepare(device: u32, sample_rate: f64) -> Result<(Self, Pcm), ()> {
		let (capture, captured) = RingBuffer::new(4_800);
		let (playback, output) = RingBuffer::new(4_800);
		let capture = Mutex::new(capture);
		let output = Arc::new(Mutex::new(output));
		let rendered_output = output.clone();
		let level = Arc::new(AtomicU32::new(0));
		let captured_level = level.clone();
		let receive = RcBlock::new(
			move |_: NonNull<AudioTimeStamp>, count: u32, buffers: NonNull<AudioBufferList>| unsafe {
				capture_audio(count, buffers, &capture, &captured_level)
			},
		);
		let render = RcBlock::new(
			move |mut silence: NonNull<Bool>,
			      _: NonNull<AudioTimeStamp>,
			      count: u32,
			      mut buffers: NonNull<AudioBufferList>| {
				let buffers = unsafe { buffers.as_mut() };

				if buffers.mNumberBuffers != 1 {
					return -50;
				}

				let buffer = &mut buffers.mBuffers[0];

				if buffer.mData.is_null()
					|| (buffer.mDataByteSize as usize) < count as usize * size_of::<f32>()
				{
					return -50;
				}

				let samples = unsafe {
					slice::from_raw_parts_mut(buffer.mData.cast::<f32>(), count as usize)
				};

				samples.fill(0.0);

				if let Ok(mut output) = rendered_output.try_lock() {
					for sample in samples.iter_mut() {
						*sample = output.pop().unwrap_or(0.0);
					}
				}

				unsafe {
					*silence.as_mut() = Bool::new(samples.iter().all(|sample| *sample == 0.0));
				}

				0
			},
		);

		// Engine configuration and lifetime operations remain on the creating thread.
		// Blocks only access bounded queues; they do not allocate, wait or touch UI state.
		unsafe {
			let engine = AVAudioEngine::new();
			let input = engine.inputNode();

			// Select the input before voice processing combines the input and output units.
			// Changing CurrentDevice on bus 0 of the shared voice unit selects the output.
			let unit: Retained<AUAudioUnit> = input.AUAudioUnit();
			if device != 0 && unit.deviceID() != device {
				unit.setDeviceID_error(device).map_err(|_| ())?;
			}
			input.setVoiceProcessingEnabled_error(true).map_err(|_| ())?;
			input.setVoiceProcessingOtherAudioDuckingConfiguration(
				AVAudioVoiceProcessingOtherAudioDuckingConfiguration {
					enableAdvancedDucking: Bool::NO,
					duckingLevel: AVAudioVoiceProcessingOtherAudioDuckingLevel::Min,
				},
			);

			let format = AVAudioFormat::initStandardFormatWithSampleRate_channels(
				AVAudioFormat::alloc(),
				sample_rate,
				1,
			)
			.ok_or(())?;
			let sink = AVAudioSinkNode::initWithReceiverBlock(
				AVAudioSinkNode::alloc(),
				RcBlock::as_ptr(&receive),
			);
			let source = AVAudioSourceNode::initWithFormat_renderBlock(
				AVAudioSourceNode::alloc(),
				&format,
				RcBlock::as_ptr(&render),
			);

			engine.attachNode(&sink);
			engine.attachNode(&source);
			engine.connect_to_format(&input, &sink, Some(&format));
			engine.connect_to_format(&source, &engine.mainMixerNode(), Some(&format));
			// Voice processing requires matching client input and output sample rates.
			engine.connect_to_format(&engine.mainMixerNode(), &engine.outputNode(), Some(&format));

			let device =
				Self { engine, sink, source, output, level, _thread: std::marker::PhantomData };

			device.engine.prepare();

			Ok((device, Pcm { captured, playback }))
		}
	}

	fn clear_playback(&self) -> Result<(), ()> {
		let mut output = self.output.lock().map_err(|_| ())?;
		while output.pop().is_ok() {}
		Ok(())
	}

	pub(super) fn running(&self) -> bool {
		unsafe { self.engine.isRunning() }
	}

	pub(super) fn level(&self) -> f32 {
		f32::from_bits(self.level.load(Ordering::Relaxed))
	}
}
impl Drop for Device {
	fn drop(&mut self) {
		unsafe {
			self.engine.stop();
			self.engine.disconnectNodeInput(&self.sink);
			self.engine.disconnectNodeOutput(&self.source);
			self.engine.detachNode(&self.sink);
			self.engine.detachNode(&self.source);
		}
	}
}

/// # Safety
/// The audio graph must supply a valid buffer list and Float32 sample storage for this call.
unsafe fn capture_audio(
	count: u32,
	buffers: NonNull<AudioBufferList>,
	capture: &Mutex<Producer<f32>>,
	captured_level: &AtomicU32,
) -> i32 {
	// The graph supplies one non-interleaved Float32 mono buffer at 48 kHz.
	let buffers = unsafe { buffers.as_ref() };

	if buffers.mNumberBuffers != 1 {
		return -50;
	}

	let buffer = &buffers.mBuffers[0];

	if buffer.mData.is_null() || (buffer.mDataByteSize as usize) < count as usize * size_of::<f32>()
	{
		return -50;
	}

	let samples = unsafe { slice::from_raw_parts(buffer.mData.cast::<f32>(), count as usize) };

	if let Ok(mut capture) = capture.try_lock() {
		let mut energy = 0.0;

		for &sample in samples {
			let sample = if sample.is_finite() { sample.clamp(-1.0, 1.0) } else { 0.0 };

			energy += sample * sample;

			let _ = capture.push(sample);
		}

		captured_level
			.store((energy / count.max(1) as f32).sqrt().min(1.0).to_bits(), Ordering::Relaxed);
	}

	0
}
