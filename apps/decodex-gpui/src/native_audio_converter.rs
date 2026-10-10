//! Apple sample-rate conversion on the audio owner, never in the device callback.
use block2::RcBlock;
use objc2::{AnyThread, rc::Retained};
use objc2_avf_audio::{
	AVAudioBuffer, AVAudioConverter, AVAudioConverterInputStatus, AVAudioConverterOutputStatus,
	AVAudioFormat, AVAudioPCMBuffer,
};
use rtrb::{Consumer, Producer};
use std::{
	cell::Cell,
	ptr::NonNull,
	slice,
	sync::{Arc, Mutex},
};

pub(super) struct CaptureConverter {
	input: Consumer<f32>,
	output: Arc<Mutex<Producer<f32>>>,
	converter: Retained<AVAudioConverter>,
	buffer: Retained<AVAudioPCMBuffer>,
	converted: Retained<AVAudioPCMBuffer>,
	finished: bool,
	input_frames: usize,
}
impl CaptureConverter {
	pub(super) fn new(
		input: Consumer<f32>,
		output: Arc<Mutex<Producer<f32>>>,
		rate: f64,
		target: f64,
	) -> Result<Self, ()> {
		unsafe {
			let from = AVAudioFormat::initStandardFormatWithSampleRate_channels(
				AVAudioFormat::alloc(),
				rate,
				1,
			)
			.ok_or(())?;
			let to = AVAudioFormat::initStandardFormatWithSampleRate_channels(
				AVAudioFormat::alloc(),
				target,
				1,
			)
			.ok_or(())?;
			let converter =
				AVAudioConverter::initFromFormat_toFormat(AVAudioConverter::alloc(), &from, &to)
					.ok_or(())?;
			let buffer = AVAudioPCMBuffer::initWithPCMFormat_frameCapacity(
				AVAudioPCMBuffer::alloc(),
				&from,
				4096,
			)
			.ok_or(())?;
			let converted = AVAudioPCMBuffer::initWithPCMFormat_frameCapacity(
				AVAudioPCMBuffer::alloc(),
				&to,
				4096,
			)
			.ok_or(())?;
			Ok(Self {
				input,
				output,
				converter,
				buffer,
				converted,
				finished: false,
				input_frames: (rate / 100.).ceil().clamp(1., 4096.) as usize,
			})
		}
	}

	pub(super) fn reset(&mut self) {
		self.finished = false;
		while self.input.pop().is_ok() {}
		unsafe {
			self.converter.reset();
		}
	}

	pub(super) fn pump(&mut self) -> Result<(), ()> {
		while !self.input.is_empty() {
			if self.output.lock().map_err(|_| ())?.slots() < 4096 {
				break;
			}
			unsafe {
				let mut samples = [0.0; 4096];
				let mut count = 0;
				for sample in samples.iter_mut().take(self.input_frames) {
					let Ok(value) = self.input.pop() else { break };
					*sample = value;
					count += 1;
				}

				let supplied = Cell::new(0);
				let input = self.buffer.clone();
				// Respect the converter's requested frame count, retaining the remaining input.
				let block = RcBlock::new(
					move |requested: u32,
					      mut status: NonNull<AVAudioConverterInputStatus>|
					      -> *mut AVAudioBuffer {
						if supplied.get() == count {
							*status.as_mut() = AVAudioConverterInputStatus::NoDataNow;
							return std::ptr::null_mut();
						}
						let offset = supplied.get();
						let frames = requested.min(count - offset);
						std::ptr::copy_nonoverlapping(
							samples.as_ptr().add(offset as usize),
							(*input.floatChannelData()).as_ptr(),
							frames as usize,
						);
						input.setFrameLength(frames);
						supplied.set(offset + frames);
						*status.as_mut() = AVAudioConverterInputStatus::HaveData;
						Retained::as_ptr(&input).cast_mut().cast()
					},
				);
				loop {
					let status = self.converter.convertToBuffer_error_withInputFromBlock(
						&self.converted,
						None,
						RcBlock::as_ptr(&block),
					);
					if status == AVAudioConverterOutputStatus::Error {
						return Err(());
					}
					self.write_output()?;
					if status != AVAudioConverterOutputStatus::HaveData {
						break;
					}
				}
			}
		}
		Ok(())
	}

	fn write_output(&self) -> Result<(), ()> {
		unsafe {
			let values = slice::from_raw_parts(
				(*self.converted.floatChannelData()).as_ptr(),
				self.converted.frameLength() as usize,
			);
			let mut output = self.output.lock().map_err(|_| ())?;
			for &sample in values {
				output.push(sample).map_err(|_| ())?;
			}
		}
		Ok(())
	}

	pub(super) fn finish(&mut self) -> Result<bool, ()> {
		if self.finished {
			return Ok(true);
		}
		self.pump()?;
		if !self.input.is_empty() || self.output.lock().map_err(|_| ())?.slots() < 4096 {
			return Ok(false);
		}
		let end = RcBlock::new(
			|_: u32, mut status: NonNull<AVAudioConverterInputStatus>| -> *mut AVAudioBuffer {
				unsafe {
					*status.as_mut() = AVAudioConverterInputStatus::EndOfStream;
				}
				std::ptr::null_mut()
			},
		);
		let status = unsafe {
			self.converter.convertToBuffer_error_withInputFromBlock(
				&self.converted,
				None,
				RcBlock::as_ptr(&end),
			)
		};
		if status == AVAudioConverterOutputStatus::Error {
			return Err(());
		}
		self.write_output()?;
		self.finished = status == AVAudioConverterOutputStatus::EndOfStream;
		Ok(self.finished)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rtrb::RingBuffer;

	#[test]
	fn finishing_keeps_audio_when_the_consumer_is_temporarily_full() {
		let (mut microphone, input) = RingBuffer::new(4800);
		let (mut output, mut captured) = RingBuffer::new(4096);
		for _ in 0..4096 {
			output.push(0.75).unwrap();
		}
		for _ in 0..4800 {
			microphone.push(0.25).unwrap();
		}
		let mut converter =
			CaptureConverter::new(input, Arc::new(Mutex::new(output)), 24_000., 24_000.).unwrap();
		assert!(!converter.finish().unwrap());
		let mut result = Vec::new();
		loop {
			while let Ok(sample) = captured.pop() {
				result.push(sample);
			}
			if converter.finish().unwrap() {
				break;
			}
		}
		while let Ok(sample) = captured.pop() {
			result.push(sample);
		}
		assert_eq!(result.len(), 4096 + 4800);
		assert!(result[..4096].iter().all(|sample| *sample == 0.75));
		assert!(result[4096..].iter().all(|sample| *sample == 0.25));
	}

	#[test]
	fn hardware_rates_keep_duration_and_amplitude() {
		for rate in [8_000, 16_000, 24_000, 44_100, 48_000, 96_000] {
			let (mut microphone, input) = RingBuffer::new(4096);
			let (output, mut captured) = RingBuffer::new(8192);
			let mut converter = CaptureConverter::new(
				input,
				Arc::new(Mutex::new(output)),
				f64::from(rate),
				24_000.,
			)
			.unwrap();
			let mut result = Vec::new();
			for frame in 0..rate {
				microphone.push(0.25).unwrap();
				if frame % 441 == 440 {
					converter.pump().unwrap();
					while let Ok(sample) = captured.pop() {
						result.push(sample);
					}
				}
			}
			converter.pump().unwrap();
			while let Ok(sample) = captured.pop() {
				result.push(sample);
			}
			while !converter.finish().unwrap() {
				while let Ok(sample) = captured.pop() {
					result.push(sample);
				}
			}
			while let Ok(sample) = captured.pop() {
				result.push(sample);
			}
			assert!(
				(23_999..=24_001).contains(&result.len()),
				"rate={rate}, samples={}",
				result.len()
			);
			assert!(
				result[100..result.len() - 100].iter().all(|sample| (sample - 0.25).abs() < 0.002)
			);
			converter.reset();
			for _ in 0..2048 {
				microphone.push(0.).unwrap();
			}
			converter.pump().unwrap();
			while let Ok(sample) = captured.pop() {
				assert!(sample.abs() < 0.001);
			}
		}
	}
}
