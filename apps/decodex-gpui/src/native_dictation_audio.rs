//! Apple's Voice Processing I/O owns dictation noise suppression, gain and resampling.
use std::{
	ptr::NonNull,
	slice,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
};

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

use super::{Ports, capture_audio, route::Route};

pub(super) struct Input {
	engine: Retained<AVAudioEngine>,
	sink: Retained<AVAudioSinkNode>,
	source: Retained<AVAudioSourceNode>,
	configuration_changed: Arc<AtomicBool>,
	observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

impl Input {
	pub(super) fn new(route: &Route, sample_rate: f64, ports: Ports) -> Result<Self, ()> {
		let capture = ports.captured;
		let level = ports.level;
		let receive = RcBlock::new(
			move |_: NonNull<AudioTimeStamp>, count: u32, buffers: NonNull<AudioBufferList>| unsafe {
				capture_audio(count, buffers, &capture, &level)
			},
		);
		let render = RcBlock::new(
			|silence: NonNull<Bool>, _: NonNull<AudioTimeStamp>, count, buffers| unsafe {
				render_silence(silence, count, buffers)
			},
		);

		// All graph operations stay on the audio owner. Render callbacks only copy bounded PCM.
		unsafe {
			let engine = AVAudioEngine::new();
			let input = engine.inputNode();
			// Bind an explicit input before Voice Processing combines the I/O units.
			// Preserve Apple's aggregate for the default input, including Bluetooth routes.
			if route.requested() != 0 && !route.is_default_input()? {
				input.AUAudioUnit().setDeviceID_error(route.selected).map_err(|error| {
					eprintln!("DecodexAudio dictation_select_input_failed: {error}");
				})?;
			}

			input.setVoiceProcessingEnabled_error(true).map_err(|error| {
				eprintln!("DecodexAudio dictation_voice_processing_failed: {error}");
			})?;
			input.setVoiceProcessingBypassed(false);
			input.setVoiceProcessingAGCEnabled(true);
			if !input.isVoiceProcessingEnabled()
				|| input.isVoiceProcessingBypassed()
				|| !input.isVoiceProcessingAGCEnabled()
			{
				eprintln!("DecodexAudio dictation_voice_processing_unavailable");

				return Err(());
			}
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
			// Voice Processing I/O converts hardware audio to the 24 kHz mono client format.
			engine.connect_to_format(&input, &sink, Some(&format));

			// Voice Processing requires duplex I/O with matching client rates. Render silence;
			// captured speech never goes to the speakers or a second software processor.
			let source = AVAudioSourceNode::initWithFormat_renderBlock(
				AVAudioSourceNode::alloc(),
				&format,
				RcBlock::as_ptr(&render),
			);
			engine.attachNode(&source);
			engine.connect_to_format(&source, &engine.mainMixerNode(), Some(&format));
			engine.connect_to_format(&engine.mainMixerNode(), &engine.outputNode(), Some(&format));
			engine.prepare();

			let configuration_changed = Arc::new(AtomicBool::new(false));
			let changed = configuration_changed.clone();
			let callback = RcBlock::new(move |_: NonNull<NSNotification>| {
				changed.store(true, Ordering::Release);
			});
			// Apple's notification thread must not change the graph. The owner rebuilds it.
			let observer = NSNotificationCenter::defaultCenter()
				.addObserverForName_object_queue_usingBlock(
					Some(AVAudioEngineConfigurationChangeNotification),
					Some(&engine),
					None,
					&callback,
				);

			Ok(Self { engine, sink, source, configuration_changed, observer })
		}
	}

	pub(super) fn reset(&self) -> Result<(), ()> {
		unsafe { self.engine.reset() };

		Ok(())
	}

	pub(super) fn start(&self) -> Result<(), ()> {
		unsafe {
			self.engine.startAndReturnError().map_err(|error| {
				eprintln!("DecodexAudio dictation_start_failed: {error}");
			})
		}
	}

	pub(super) fn stop(&self) {
		// Pause stops hardware synchronously but preserves prepared Voice Processing resources.
		// The worker then drains PCM already delivered by the sink; the next start stays warm.
		unsafe { self.engine.pause() };
	}

	pub(super) fn cancel(&self) {
		self.stop();
	}

	pub(super) fn finished(&self) -> Result<bool, ()> {
		self.running().map(|running| !running)
	}

	pub(super) fn running(&self) -> Result<bool, ()> {
		Ok(unsafe { self.engine.isRunning() })
	}

	pub(super) fn configuration_changed(&self) -> bool {
		self.configuration_changed.swap(false, Ordering::AcqRel)
	}

	#[cfg(test)]
	pub(super) fn settings(&self) -> super::DictationInput {
		unsafe {
			let input = self.engine.inputNode();
			let format = self.sink.inputFormatForBus(0);

			super::DictationInput {
				sample_rate: format.sampleRate(),
				channels: format.channelCount(),
				voice_processing: input.isVoiceProcessingEnabled()
					&& !input.isVoiceProcessingBypassed(),
				agc: input.isVoiceProcessingAGCEnabled(),
			}
		}
	}
}

impl Drop for Input {
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

/// # Safety
/// The source node supplies valid writable Float32 buffers for the requested frame count.
unsafe fn render_silence(
	mut silence: NonNull<Bool>,
	count: u32,
	mut buffers: NonNull<AudioBufferList>,
) -> i32 {
	let buffers = unsafe { buffers.as_mut() };
	let channels = unsafe {
		slice::from_raw_parts_mut(buffers.mBuffers.as_mut_ptr(), buffers.mNumberBuffers as usize)
	};

	for buffer in channels {
		let samples = count as usize * buffer.mNumberChannels as usize;

		if buffer.mData.is_null() || (buffer.mDataByteSize as usize) < samples * size_of::<f32>() {
			return -50;
		}

		unsafe { slice::from_raw_parts_mut(buffer.mData.cast::<f32>(), samples) }.fill(0.);
	}

	unsafe { *silence.as_mut() = Bool::YES };

	0
}
