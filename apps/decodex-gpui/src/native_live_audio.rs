//! Duplex voice processing remains separate from input-only dictation preparation.
use super::{Ports, capture_audio, render_audio};
use block2::RcBlock;
use objc2::{
	AnyThread,
	rc::Retained,
	runtime::{Bool, ProtocolObject},
};
use objc2_avf_audio::{
	AVAudioEngine, AVAudioEngineConfigurationChangeNotification, AVAudioFormat, AVAudioSinkNode,
	AVAudioSourceNode, AVAudioVoiceProcessingOtherAudioDuckingConfiguration,
	AVAudioVoiceProcessingOtherAudioDuckingLevel,
};
use objc2_core_audio_types::{AudioBufferList, AudioTimeStamp};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};
use std::{
	ptr::NonNull,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

pub(super) struct Live {
	engine: Retained<AVAudioEngine>,
	sink: Retained<AVAudioSinkNode>,
	source: Retained<AVAudioSourceNode>,
	changed: Arc<AtomicBool>,
	observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}
impl Live {
	pub(super) fn new(device: u32, sample_rate: f64, ports: &Ports) -> Result<Self, ()> {
		let capture = ports.captured.clone();
		let level = ports.level.clone();
		let output = ports.output.clone();
		let receive = RcBlock::new(
			move |_: NonNull<AudioTimeStamp>, count: u32, buffers: NonNull<AudioBufferList>| unsafe {
				capture_audio(count, buffers, &capture, &level)
			},
		);
		let render = RcBlock::new(
			move |silence: NonNull<Bool>,
			      _: NonNull<AudioTimeStamp>,
			      count: u32,
			      buffers: NonNull<AudioBufferList>| unsafe {
				render_audio(silence, count, buffers, &output)
			},
		);
		unsafe {
			let engine = AVAudioEngine::new();
			let input = engine.inputNode();
			// Select before VoiceProcessingIO combines the input and output units.
			let unit = input.AUAudioUnit();
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
			engine.attachNode(&sink);
			engine.connect_to_format(&input, &sink, Some(&format));
			let source = AVAudioSourceNode::initWithFormat_renderBlock(
				AVAudioSourceNode::alloc(),
				&format,
				RcBlock::as_ptr(&render),
			);
			engine.attachNode(&source);
			engine.connect_to_format(&source, &engine.mainMixerNode(), Some(&format));
			engine.connect_to_format(&engine.mainMixerNode(), &engine.outputNode(), Some(&format));
			let changed = Arc::new(AtomicBool::new(false));
			let dirty = changed.clone();
			let callback = RcBlock::new(move |_: NonNull<NSNotification>| {
				dirty.store(true, Ordering::Release);
			});
			let observer = NSNotificationCenter::defaultCenter()
				.addObserverForName_object_queue_usingBlock(
					Some(AVAudioEngineConfigurationChangeNotification),
					Some(&engine),
					None,
					&callback,
				);
			engine.prepare();
			Ok(Self { engine, sink, source, changed, observer })
		}
	}

	pub(super) fn start(&self) -> Result<(), ()> {
		unsafe {
			self.engine
				.startAndReturnError()
				.map_err(|error| eprintln!("DecodexAudio engine_start_failed: {error}"))
		}
	}

	pub(super) fn stop(&self) {
		unsafe {
			self.engine.stop();
		}
	}

	pub(super) fn pause(&self) {
		unsafe {
			self.engine.pause();
		}
	}

	pub(super) fn running(&self) -> bool {
		unsafe { self.engine.isRunning() }
	}

	pub(super) fn changed(&self) -> bool {
		self.changed.swap(false, Ordering::AcqRel)
	}
}
impl Drop for Live {
	fn drop(&mut self) {
		unsafe {
			NSNotificationCenter::defaultCenter().removeObserver((*self.observer).as_ref());
			self.engine.stop();
			self.engine.disconnectNodeInput(&self.sink);
			self.engine.disconnectNodeOutput(&self.source);
			self.engine.detachNode(&self.sink);
			self.engine.detachNode(&self.source);
		}
	}
}
