//! Prewarm an input-only Audio Queue without configuring a playback graph.
use std::{
	ffi::c_void,
	ptr::{self, NonNull},
	sync::atomic::{AtomicBool, Ordering},
};

#[cfg(test)] use objc2_audio_toolbox::kAudioQueueProperty_StreamDescription;
use objc2_audio_toolbox::{
	AudioQueueAllocateBuffer, AudioQueueBufferRef, AudioQueueDispose, AudioQueueEnqueueBuffer,
	AudioQueueGetProperty, AudioQueueNewInput, AudioQueueRef, AudioQueueReset,
	AudioQueueSetProperty, AudioQueueStart, AudioQueueStop, kAudioQueueProperty_CurrentDevice,
	kAudioQueueProperty_IsRunning,
};
use objc2_core_audio_types::{
	AudioBuffer, AudioBufferList, AudioStreamBasicDescription, AudioStreamPacketDescription,
	AudioTimeStamp, kAudioFormatFlagsNativeFloatPacked, kAudioFormatLinearPCM,
};
use objc2_core_foundation::CFString;

use crate::shell::agent_surface::voice::audio::{self, Ports, route::Route};

pub(super) struct Input {
	queue: AudioQueueRef,
	callback: Box<Callback>,
	buffers: [AudioQueueBufferRef; 3],
}
impl Input {
	pub(super) fn new(route: &Route, rate: f64, ports: Ports) -> Result<Self, ()> {
		let mut format = AudioStreamBasicDescription {
			mSampleRate: rate,
			mFormatID: kAudioFormatLinearPCM,
			mFormatFlags: kAudioFormatFlagsNativeFloatPacked,
			mBytesPerPacket: 4,
			mFramesPerPacket: 1,
			mBytesPerFrame: 4,
			mChannelsPerFrame: 1,
			mBitsPerChannel: 32,
			mReserved: 0,
		};
		let mut callback = Box::new(Callback {
			ports,
			recording: AtomicBool::new(false),
			failed: AtomicBool::new(false),
		});
		let mut queue = ptr::null_mut();

		// Audio Queue owns format conversion and an input-only hardware connection.
		// A null run loop requests its internal callback thread. The boxed state stays
		// valid until synchronous disposal has finished all callbacks.
		unsafe {
			check(AudioQueueNewInput(
				NonNull::from(&mut format),
				Some(receive),
				(&raw mut *callback).cast(),
				None,
				None,
				0,
				NonNull::from(&mut queue),
			))?;

			let mut input = Self { queue, callback, buffers: [ptr::null_mut(); 3] };

			if route.requested() != 0 {
				let uid = route.input_uid()?;
				let mut value = uid.as_ref() as *const CFString;

				check(AudioQueueSetProperty(
					queue,
					kAudioQueueProperty_CurrentDevice,
					NonNull::from(&mut value).cast(),
					size_of_val(&value) as u32,
				))?;
			}

			// Ten milliseconds per callback; memory is allocated before the user clicks.
			for buffer in &mut input.buffers {
				check(AudioQueueAllocateBuffer(
					queue,
					(rate / 100.) as u32 * 4,
					NonNull::from(buffer),
				))?;
			}

			Ok(input)
		}
	}

	pub(super) fn reset(&self) -> Result<(), ()> {
		unsafe {
			// Complete a previous graceful stop before reusing its prepared buffers.
			self.callback.recording.store(false, Ordering::Release);

			if self.running()? {
				check(AudioQueueStop(self.queue, true))?;
			}

			check(AudioQueueReset(self.queue))?;

			self.callback.failed.store(false, Ordering::Release);
		}

		Ok(())
	}

	pub(super) fn start(&self) -> Result<(), ()> {
		unsafe {
			for buffer in self.buffers {
				check(AudioQueueEnqueueBuffer(self.queue, buffer, 0, ptr::null()))?;
			}

			self.callback.recording.store(true, Ordering::Release);

			if let Err(error) = check(AudioQueueStart(self.queue, ptr::null())) {
				self.callback.recording.store(false, Ordering::Release);

				return Err(error);
			}
		}

		Ok(())
	}

	pub(super) fn stop(&self) {
		if self.callback.recording.swap(false, Ordering::AcqRel)
			&& check(unsafe { AudioQueueStop(self.queue, false) }).is_err()
		{
			self.callback.failed.store(true, Ordering::Release);
		}
	}

	pub(super) fn cancel(&self) {
		let recording = self.callback.recording.swap(false, Ordering::AcqRel);

		if recording || self.running().unwrap_or(false) {
			let _ = check(unsafe { AudioQueueStop(self.queue, true) });
		}
	}

	pub(super) fn finished(&self) -> Result<bool, ()> {
		if self.callback.failed.load(Ordering::Acquire) {
			return Err(());
		}

		self.running().map(|running| !running)
	}

	pub(super) fn running(&self) -> Result<bool, ()> {
		let mut value = 0_u32;
		let mut size = size_of_val(&value) as u32;

		check(unsafe {
			AudioQueueGetProperty(
				self.queue,
				kAudioQueueProperty_IsRunning,
				NonNull::from(&mut value).cast(),
				NonNull::from(&mut size),
			)
		})?;

		Ok(value != 0)
	}

	#[cfg(test)]
	pub(super) fn channels(&self) -> Option<u32> {
		// All fields are numeric; zero is a valid initial value for the out parameter.
		let mut format: AudioStreamBasicDescription = unsafe { std::mem::zeroed() };
		let mut size = size_of_val(&format) as u32;

		check(unsafe {
			AudioQueueGetProperty(
				self.queue,
				kAudioQueueProperty_StreamDescription,
				NonNull::from(&mut format).cast(),
				NonNull::from(&mut size),
			)
		})
		.ok()?;

		Some(format.mChannelsPerFrame)
	}
}

impl Drop for Input {
	fn drop(&mut self) {
		self.callback.recording.store(false, Ordering::Release);

		// Synchronous disposal stops capture and releases buffers before callback state.
		let _ = check(unsafe { AudioQueueDispose(self.queue, true) });
	}
}

struct Callback {
	ports: Ports,
	recording: AtomicBool,
	failed: AtomicBool,
}

fn check(status: i32) -> Result<(), ()> {
	if status == 0 {
		Ok(())
	} else {
		eprintln!("DecodexAudio input_queue_error={status}");

		Err(())
	}
}

unsafe extern "C-unwind" fn receive(
	context: *mut c_void,
	queue: AudioQueueRef,
	buffer: AudioQueueBufferRef,
	_: NonNull<AudioTimeStamp>,
	_: u32,
	_: *const AudioStreamPacketDescription,
) {
	// Apple supplies one mono Float32 buffer in the queue's requested format.
	let state = unsafe { &*context.cast::<Callback>() };
	let audio = unsafe { &*buffer };
	let mut list = AudioBufferList {
		mNumberBuffers: 1,
		mBuffers: [AudioBuffer {
			mNumberChannels: 1,
			mDataByteSize: audio.mAudioDataByteSize,
			mData: audio.mAudioData.as_ptr(),
		}],
	};
	let status = unsafe {
		audio::capture_audio(
			audio.mAudioDataByteSize / 4,
			NonNull::from(&mut list),
			&state.ports.captured,
			&state.ports.level,
		)
	};

	if status != 0 {
		state.failed.store(true, Ordering::Release);
	}
	if state.recording.load(Ordering::Acquire)
		&& unsafe { AudioQueueEnqueueBuffer(queue, buffer, 0, ptr::null()) } != 0
	{
		state.failed.store(true, Ordering::Release);
	}
}

#[cfg(test)]
mod tests {
	use std::{
		ptr,
		sync::{Arc, Mutex, atomic::AtomicU32},
	};

	use rtrb::RingBuffer;

	use crate::shell::agent_surface::voice::audio::input::{
		self, AtomicBool, Callback, NonNull, Ordering, Ports,
	};

	#[test]
	fn stopping_copies_the_final_buffer_without_requeueing() {
		let (captured, mut received) = RingBuffer::new(8);
		let (_, output) = RingBuffer::new(8);
		let mut state = Callback {
			ports: Ports {
				captured: Arc::new(Mutex::new(captured)),
				output: Arc::new(Mutex::new(output)),
				level: Arc::new(AtomicU32::new(0)),
			},
			recording: AtomicBool::new(false),
			failed: AtomicBool::new(false),
		};
		let mut samples = [0.5_f32, -0.25, 0.125];
		let mut buffer = objc2_audio_toolbox::AudioQueueBuffer {
			mAudioDataBytesCapacity: size_of_val(&samples) as u32,
			mAudioData: NonNull::from(&mut samples).cast(),
			mAudioDataByteSize: size_of_val(&samples) as u32,
			mUserData: ptr::null_mut(),
			mPacketDescriptionCapacity: 0,
			mPacketDescriptions: ptr::null(),
			mPacketDescriptionCount: 0,
		};

		// No queue call occurs after stopping. The callback does not read the timestamp.
		unsafe {
			input::receive(
				(&raw mut state).cast(),
				ptr::null_mut(),
				&raw mut buffer,
				NonNull::dangling(),
				0,
				ptr::null(),
			);
		}

		for sample in samples {
			assert_eq!(received.pop(), Ok(sample));
		}

		assert!(received.is_empty());
		assert!(!state.failed.load(Ordering::Acquire));
	}
}
