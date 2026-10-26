//! WebRTC prepares software DSP without creating Apple's duplex aggregate device.
use std::cell::Cell;

use libwebrtc::peer_connection_factory::{
	PeerConnectionFactory, native::PeerConnectionFactoryExt as _,
};

pub(super) struct Live {
	factory: PeerConnectionFactory,
	device: u32,
	running: Cell<bool>,
}
impl Live {
	pub(super) fn new(device: u32) -> Result<Self, ()> {
		let factory = PeerConnectionFactory::default();

		factory.set_adm_recording_enabled(false);
		factory.set_adm_playout_enabled(false);

		if !factory.acquire_platform_adm() {
			return Err(());
		}

		// Factory construction initializes WebRTC's software AEC, NS and AGC.
		// Acquiring the platform device enumerates CoreAudio; it does not start I/O.
		let live = Self { factory, device, running: Cell::new(false) };

		live.select()?;

		Ok(live)
	}

	pub(super) fn factory(&self) -> PeerConnectionFactory {
		self.factory.clone()
	}

	fn select(&self) -> Result<(), ()> {
		// WebRTC's macOS index zero follows the system default. Explicit GUIDs in
		// this backend are CoreAudio IDs; resolve an index without a silent fallback.
		let input = if self.device == 0 {
			0
		} else {
			let id = self.device.to_string();

			(0..self.factory.recording_devices().max(0) as u16)
				.find(|&index| self.factory.recording_device_guid(index) == id)
				.ok_or(())?
		};

		if !self.factory.set_recording_device(input) {
			eprintln!(
				"DecodexAudio live_select_input_failed initialized={}",
				self.factory.recording_is_initialized()
			);

			return Err(());
		}
		if !self.factory.set_playout_device(0) {
			eprintln!(
				"DecodexAudio live_select_output_failed initialized={}",
				self.factory.playout_is_initialized()
			);

			return Err(());
		}

		Ok(())
	}

	pub(super) fn start(&self) -> Result<(), ()> {
		self.factory.set_adm_recording_enabled(true);
		self.factory.set_adm_playout_enabled(true);

		for (stage, action) in [
			(
				"initialize_output",
				PeerConnectionFactory::init_playout as fn(&PeerConnectionFactory) -> bool,
			),
			("initialize_input", PeerConnectionFactory::init_recording),
			("start_output", PeerConnectionFactory::start_playout),
			("start_input", PeerConnectionFactory::start_recording),
		] {
			if !action(&self.factory) {
				eprintln!("DecodexAudio live_start_failed stage={stage}");

				self.stop();

				return Err(());
			}
		}

		self.running.set(true);

		Ok(())
	}

	pub(super) fn stop(&self) {
		self.factory.stop_recording();
		self.factory.stop_playout();
		self.factory.set_adm_recording_enabled(false);
		self.factory.set_adm_playout_enabled(false);
		self.running.set(false);
	}

	pub(super) fn running(&self) -> bool {
		self.running.get()
	}
}
impl Drop for Live {
	fn drop(&mut self) {
		self.stop();
		self.factory.release_platform_adm();
	}
}
