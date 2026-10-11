//! Apple voice processing with a direct, bounded PCM path to the native transport.
#![cfg_attr(test, allow(dead_code))]
#[path = "native_audio_converter.rs"] mod converter;
#[path = "native_audio_owner.rs"] pub(super) mod owner;
#[path = "native_audio_route.rs"] mod route;
use converter::CaptureConverter;
use route::Route;
use std::{
	ptr::NonNull,
	rc::Rc,
	slice,
	sync::{
		Arc, Mutex,
		atomic::{AtomicBool, AtomicU32, Ordering},
	},
};

use block2::RcBlock;
use objc2::{
	AnyThread,
	rc::Retained,
	runtime::{Bool, ProtocolObject},
};
use objc2_audio_toolbox::AUAudioUnit;
use objc2_avf_audio::{
	AVAudioEngine, AVAudioEngineConfigurationChangeNotification, AVAudioFormat, AVAudioSinkNode,
	AVAudioSourceNode, AVAudioVoiceProcessingOtherAudioDuckingConfiguration,
	AVAudioVoiceProcessingOtherAudioDuckingLevel,
};
use objc2_core_audio_types::{AudioBufferList, AudioTimeStamp};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};
use rtrb::{Consumer, Producer, RingBuffer};

pub(super) const LIVE_SAMPLE_RATE: u32 = 48_000;
pub(super) const LIVE_FRAME_SAMPLES: u32 = LIVE_SAMPLE_RATE / 100;

pub(super) struct Pcm {
	pub(super) captured: Consumer<f32>,
	pub(super) playback: Producer<f32>,
}

#[derive(Clone)]
struct Ports {
	captured: Arc<Mutex<Producer<f32>>>,
	output: Arc<Mutex<Consumer<f32>>>,
	level: Arc<AtomicU32>,
}
pub(super) struct Device {
	engine: Retained<AVAudioEngine>,
	sink: Retained<AVAudioSinkNode>,
	source: Option<Retained<AVAudioSourceNode>>,
	configuration_changed: Arc<AtomicBool>,
	observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
	converter: Option<CaptureConverter>,
	sample_rate: f64,
	route: Route,
	ports: Ports,
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

	fn stop_for_route(&self) {
		self.ports.level.store(0, Ordering::Relaxed);
		unsafe {
			self.engine.stop();
		}
	}

	pub(super) fn pause(&self) {
		unsafe {
			self.engine.pause();
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
		let started = std::time::Instant::now();
		eprintln!("DecodexAudio route_rebuild_begin");
		let ports = old.ports.clone();
		ports.level.store(0, Ordering::Relaxed);
		let device = old.route.requested();
		let rate = old.sample_rate;
		let duplex = old.source.is_some();
		drop(old);
		eprintln!("DecodexAudio route_release_ms={:.3}", started.elapsed().as_secs_f64() * 1000.);
		let rebuilt = Self::build(device, rate, duplex, ports);
		eprintln!("DecodexAudio route_build_ms={:.3}", started.elapsed().as_secs_f64() * 1000.);
		rebuilt
	}

	fn build(device: u32, sample_rate: f64, duplex: bool, ports: Ports) -> Result<Self, ()> {
		let (capture, raw) = if duplex {
			(ports.captured.clone(), None)
		} else {
			let (send, receive) = RingBuffer::new(19_200);
			(Arc::new(Mutex::new(send)), Some(receive))
		};
		let rendered_output = ports.output.clone();
		let captured_level = ports.level.clone();
		let receive = RcBlock::new(
			move |_: NonNull<AudioTimeStamp>, count: u32, buffers: NonNull<AudioBufferList>| unsafe {
				capture_audio(count, buffers, &capture, &captured_level)
			},
		);
		let render = RcBlock::new(
			move |silence: NonNull<Bool>,
			      _: NonNull<AudioTimeStamp>,
			      count: u32,
			      buffers: NonNull<AudioBufferList>| {
				unsafe { render_audio(silence, count, buffers, &rendered_output) }
			},
		);

		// Engine configuration and lifetime operations remain on the creating thread.
		// Blocks only access bounded queues; they do not allocate, wait or touch UI state.
		unsafe {
			let route = Route::new(device)?;
			let engine = AVAudioEngine::new();
			let input = engine.inputNode();

			// Select the input before voice processing combines the input and output units.
			// Changing CurrentDevice on bus 0 of the shared voice unit selects the output.
			let unit: Retained<AUAudioUnit> = input.AUAudioUnit();
			// Input-only capture must not depend on an aggregate containing the output
			// device. Route observes system-default changes and selects the physical input.
			let selected = if duplex { device } else { route.selected };
			if selected != 0 && unit.deviceID() != selected {
				unit.setDeviceID_error(selected).map_err(|_| ())?;
			}
			if duplex {
				input.setVoiceProcessingEnabled_error(true).map_err(|_| ())?;
				input.setVoiceProcessingOtherAudioDuckingConfiguration(
					AVAudioVoiceProcessingOtherAudioDuckingConfiguration {
						enableAdvancedDucking: Bool::NO,
						duckingLevel: AVAudioVoiceProcessingOtherAudioDuckingLevel::Min,
					},
				);
			}

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
			engine.attachNode(&sink);
			let converter = if let Some(raw) = raw {
				// The client/output format can still describe the previous device after a
				// Bluetooth route change. The hardware/input scope owns the new rate.
				engine.connect_to_format(&input, &sink, None);
				let hardware = sink.inputFormatForBus(0);
				Some(CaptureConverter::new(
					raw,
					ports.captured.clone(),
					hardware.sampleRate(),
					sample_rate,
				)?)
			} else {
				engine.connect_to_format(&input, &sink, Some(&format));
				None
			};
			// Dictation has no playback or echo to cancel. Do not activate an output unit.
			let source = if duplex {
				let source = AVAudioSourceNode::initWithFormat_renderBlock(
					AVAudioSourceNode::alloc(),
					&format,
					RcBlock::as_ptr(&render),
				);
				engine.attachNode(&source);
				engine.connect_to_format(&source, &engine.mainMixerNode(), Some(&format));
				// Voice processing requires matching client input/output rates.
				engine.connect_to_format(
					&engine.mainMixerNode(),
					&engine.outputNode(),
					Some(&format),
				);
				Some(source)
			} else {
				None
			};
			let configuration_changed = Arc::new(AtomicBool::new(false));
			let changed = configuration_changed.clone();
			let callback = RcBlock::new(move |_: NonNull<NSNotification>| {
				changed.store(true, Ordering::Release);
			});
			// Never operate on the engine in Apple's notification queue. The owner handles it.
			let observer = NSNotificationCenter::defaultCenter()
				.addObserverForName_object_queue_usingBlock(
					Some(AVAudioEngineConfigurationChangeNotification),
					Some(&engine),
					None,
					&callback,
				);
			let device = Self {
				engine,
				sink,
				source,
				ports,
				configuration_changed,
				observer,
				converter,
				sample_rate,
				route,
				_thread: std::marker::PhantomData,
			};

			device.engine.prepare();

			Ok(device)
		}
	}

	fn pump(&mut self) -> Result<(), ()> {
		if let Some(converter) = &mut self.converter {
			converter.pump()?;
		}
		Ok(())
	}

	fn finish_capture(&mut self) -> Result<bool, ()> {
		self.converter.as_mut().map_or(Ok(true), CaptureConverter::finish)
	}

	fn reset_capture(&mut self) {
		if let Some(converter) = &mut self.converter {
			converter.reset();
		}
	}

	fn configuration_changed(&self) -> bool {
		self.configuration_changed.swap(false, Ordering::AcqRel)
	}

	fn clear_playback(&self) -> Result<(), ()> {
		let mut output = self.ports.output.lock().map_err(|_| ())?;
		while output.pop().is_ok() {}
		Ok(())
	}

	pub(super) fn running(&self) -> bool {
		unsafe { self.engine.isRunning() }
	}

	pub(super) fn level(&self) -> f32 {
		f32::from_bits(self.ports.level.load(Ordering::Relaxed))
	}
}
impl Drop for Device {
	fn drop(&mut self) {
		unsafe {
			NSNotificationCenter::defaultCenter().removeObserver((*self.observer).as_ref());
			self.engine.stop();
			self.engine.disconnectNodeInput(&self.sink);
			if let Some(source) = &self.source {
				self.engine.disconnectNodeOutput(source);
			}
			self.engine.detachNode(&self.sink);
			if let Some(source) = &self.source {
				self.engine.detachNode(source);
			}
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

/// # Safety
/// The output unit supplies valid writable Float32 mono storage for this callback.
unsafe fn render_audio(
	mut silence: NonNull<Bool>,
	count: u32,
	mut buffers: NonNull<AudioBufferList>,
	output: &Mutex<Consumer<f32>>,
) -> i32 {
	let buffers = unsafe { buffers.as_mut() };
	if buffers.mNumberBuffers != 1 {
		return -50;
	}
	let buffer = &mut buffers.mBuffers[0];
	if buffer.mData.is_null() || (buffer.mDataByteSize as usize) < count as usize * size_of::<f32>()
	{
		return -50;
	}
	let samples = unsafe { slice::from_raw_parts_mut(buffer.mData.cast::<f32>(), count as usize) };
	samples.fill(0.);
	if let Ok(mut output) = output.try_lock() {
		for sample in samples.iter_mut() {
			*sample = output.pop().unwrap_or(0.);
		}
	}
	unsafe {
		*silence.as_mut() = Bool::new(samples.iter().all(|sample| *sample == 0.));
	}
	0
}
