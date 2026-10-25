use std::{env, thread, time::Instant};

use libwebrtc::peer_connection_factory::native::PeerConnectionFactoryExt as _;

use crate::shell::agent_surface::voice::audio::owner::{
	Arc, AtomicBool, AudioError, Command, Duration, Key, Mode, Ordering, Owner, mpsc,
};

#[test]
#[ignore = "Requires authorized microphone; verifies Live preparation and deferred capture"]
fn prepared_live_reserves_its_factory_without_starting_io() {
	let owner = Owner::new().unwrap();

	owner.commands.send(Command::Prepare(0)).unwrap();

	for mode in [Mode::Dictation, Mode::Live] {
		let (reply, ready) = mpsc::sync_channel(1);

		owner.commands.send(Command::CheckPrepared(mode, reply)).unwrap();

		assert!(ready.recv_timeout(Duration::from_secs(5)).unwrap());
	}

	let cancelled = Arc::new(AtomicBool::new(false));
	let mut session =
		owner.start(Key { device: 0, mode: Mode::Live }, cancelled.clone(), false).unwrap();
	let (state, _) = session.parts();
	let factory = session.live_factory();

	thread::sleep(Duration::from_millis(200));

	assert!(!state.running());
	assert!(!factory.adm_recording_enabled());
	assert!(!factory.adm_playout_enabled());
	assert!(!factory.recording_is_initialized());
	assert!(!factory.playout_is_initialized());
	assert!(matches!(
		owner.start(
			Key { device: 0, mode: Mode::Dictation },
			Arc::new(AtomicBool::new(false)),
			true
		),
		Err(AudioError::Busy)
	));

	session.resume().unwrap();

	assert!(state.running());
	assert!(factory.recording_is_initialized());

	cancelled.store(true, Ordering::Release);

	let deadline = Instant::now() + Duration::from_secs(1);

	while state.running() && Instant::now() < deadline {
		thread::sleep(Duration::from_millis(5));
	}

	assert!(!state.running());
	assert!(!factory.adm_recording_enabled());
	assert!(!factory.adm_playout_enabled());
	assert!(matches!(session.resume(), Err(AudioError::Cancelled)));
}

#[test]
fn cancelled_queued_start_does_not_open_hardware() {
	let owner = Owner::new().unwrap();

	assert!(matches!(
		owner.start(
			Key { device: u32::MAX, mode: Mode::Live },
			Arc::new(AtomicBool::new(true)),
			false,
		),
		Err(AudioError::Cancelled)
	));

	let (reply, ready) = mpsc::sync_channel(1);

	owner.commands.send(Command::CheckPrepared(Mode::Live, reply)).unwrap();

	assert!(!ready.recv_timeout(Duration::from_secs(2)).unwrap());
	assert!(matches!(
		owner.start(
			Key { device: u32::MAX, mode: Mode::Live },
			Arc::new(AtomicBool::new(true)),
			true
		),
		Err(AudioError::Cancelled)
	));
}

#[test]
#[ignore = "Requires authorized microphone and DECODEX_AUDIO_TEST_DEVICE_ID"]
fn production_owner_captures_across_modes_and_stops_before_transport_teardown() {
	let device = env::var("DECODEX_AUDIO_TEST_DEVICE_ID")
		.expect("physical input device ID")
		.parse::<u32>()
		.unwrap();

	assert_ne!(device, 0);

	let owner = Owner::new().unwrap();

	for mode in
		[Mode::Dictation, Mode::Dictation, Mode::Live, Mode::Live, Mode::Dictation, Mode::Live]
	{
		let requested = Instant::now();

		if mode == Mode::Live {
			owner.commands.send(Command::Prepare(device)).unwrap();

			let (reply, ready) = mpsc::sync_channel(1);

			owner.commands.send(Command::CheckPrepared(Mode::Live, reply)).unwrap();

			assert!(
				ready.recv_timeout(Duration::from_secs(15)).unwrap(),
				"call preparation must not record"
			);
		}

		let capture_requested = Instant::now();
		let mut first = true;
		let mut session =
			owner.start(Key { device, mode }, Arc::new(AtomicBool::new(false)), true).unwrap();
		let (state, pcm) = session.parts();
		let until = Instant::now() + Duration::from_secs(2);
		let mut captured = 0;

		while Instant::now() < until {
			assert!(state.running());

			while pcm.captured.pop().is_ok() {
				if first {
					eprintln!(
						"audio_first_pcm_ms={:.3} preparation_and_capture_ms={:.3} mode={}",
						capture_requested.elapsed().as_secs_f64() * 1_000.,
						requested.elapsed().as_secs_f64() * 1_000.,
						mode.rate()
					);

					first = false;
				}

				captured += 1;
			}

			thread::sleep(Duration::from_millis(10));
		}

		if mode == Mode::Live {
			assert!(session.live_factory().recording_is_initialized());
		} else {
			assert!(captured > mode.rate() as usize);
		}

		session.pause().unwrap();

		assert!(!state.running());
	}

	let cancelled = Arc::new(AtomicBool::new(false));
	let mut old = owner.start(Key { device, mode: Mode::Live }, cancelled.clone(), true).unwrap();
	let (old_state, _) = old.parts();

	assert!(matches!(
		owner.start(Key { device, mode: Mode::Dictation }, Arc::new(AtomicBool::new(false)), true),
		Err(AudioError::Busy)
	));

	cancelled.store(true, Ordering::Release);

	let deadline = Instant::now() + Duration::from_secs(1);

	while old_state.running() && Instant::now() < deadline {
		thread::sleep(Duration::from_millis(5));
	}

	assert!(
		!old_state.running(),
		"capture must stop while the old transport still retains its session"
	);

	let mut next = owner
		.start(Key { device, mode: Mode::Dictation }, Arc::new(AtomicBool::new(false)), true)
		.unwrap();
	let (next_state, _) = next.parts();

	drop(old); // A late old release must not stop the new session.

	thread::sleep(Duration::from_millis(50));

	assert!(next_state.running());
}

#[test]
#[ignore = "Requires authorized microphone; change system routes during each 25-second capture"]
fn production_owner_keeps_dictating_through_system_route_changes() {
	let owner = Owner::new().unwrap();

	for mode in [Mode::Dictation] {
		let mut session =
			owner.start(Key { device: 0, mode }, Arc::new(AtomicBool::new(false)), true).unwrap();
		let (state, pcm) = session.parts();
		let started = Instant::now();
		let mut last_pcm = started;
		let mut max_gap = Duration::ZERO;
		let mut samples = 0;

		eprintln!("route_test_ready rate={}", mode.rate());

		while started.elapsed() < Duration::from_secs(25) {
			assert!(state.running(), "route change terminated the session");

			let mut received = false;

			while pcm.captured.pop().is_ok() {
				samples += 1;
				received = true;
			}

			if received {
				let now = Instant::now();

				max_gap = max_gap.max(now.duration_since(last_pcm));
				last_pcm = now;
			}

			assert!(last_pcm.elapsed() < Duration::from_secs(8), "capture did not recover");

			thread::sleep(Duration::from_millis(5));
		}

		assert!(samples > mode.rate() as usize * 10);
		eprintln!(
			"route_test_complete rate={} samples={samples} max_gap_ms={:.3}",
			mode.rate(),
			max_gap.as_secs_f64() * 1_000.
		);

		session.pause().unwrap();
	}
}

#[test]
#[ignore = "Requires authorized microphone and DECODEX_AUDIO_TEST_DEVICE_ID"]
fn prepared_dictation_starts_without_recording_during_preparation() {
	let device = env::var("DECODEX_AUDIO_TEST_DEVICE_ID").unwrap().parse().unwrap();
	let owner = Owner::new().unwrap();

	for _ in 0..3 {
		let mode = Mode::Dictation;

		owner.commands.send(Command::Prepare(device)).unwrap();

		let (reply, ready) = mpsc::sync_channel(1);

		owner.commands.send(Command::CheckPrepared(Mode::Dictation, reply)).unwrap();

		assert!(
			ready.recv_timeout(Duration::from_secs(15)).unwrap(),
			"preparation must not record"
		);

		let requested = Instant::now();
		let mut session =
			owner.start(Key { device, mode }, Arc::new(AtomicBool::new(false)), true).unwrap();
		let (state, pcm) = session.parts();

		while pcm.captured.pop().is_err() {
			assert!(state.running());
			assert!(requested.elapsed() < Duration::from_secs(3));

			thread::sleep(Duration::from_millis(2));
		}

		eprintln!(
			"prepared_first_pcm_ms={:.3} mode={}",
			requested.elapsed().as_secs_f64() * 1_000.,
			mode.rate()
		);

		let finishing = Instant::now();

		while !session.pause().unwrap() {
			assert!(finishing.elapsed() < Duration::from_secs(1), "capture must finish promptly");

			while session.parts().1.captured.pop().is_ok() {}

			thread::sleep(Duration::from_millis(2));
		}

		while session.parts().1.captured.pop().is_ok() {}
	}
}

#[test]
#[ignore = "Requires authorized microphone; checks the current default without changing routes"]
fn default_dictation_keeps_warm_capture_and_does_not_pin_a_physical_device() {
	let owner = Owner::new().unwrap();

	owner.commands.send(Command::Prepare(0)).unwrap();

	let (reply, cold) = mpsc::sync_channel(1);

	owner.commands.send(Command::CheckCaptureChannels(reply)).unwrap();

	let expected_channels = cold.recv_timeout(Duration::from_secs(15)).unwrap().unwrap();
	let (reply, ready) = mpsc::sync_channel(1);

	owner.commands.send(Command::CheckPrepared(Mode::Dictation, reply)).unwrap();

	assert!(ready.recv_timeout(Duration::from_secs(15)).unwrap());

	thread::sleep(Duration::from_secs(1));

	for attempt in 0..5 {
		let requested = Instant::now();
		let (reply, channels) = mpsc::sync_channel(1);
		let mut session = owner
			.start(Key { device: 0, mode: Mode::Dictation }, Arc::new(AtomicBool::new(false)), true)
			.unwrap();

		owner.commands.send(Command::CheckCaptureChannels(reply)).unwrap();

		assert_eq!(
			channels.recv_timeout(Duration::from_secs(15)).unwrap(),
			Some(expected_channels),
			"dictation must retain its mono input format"
		);

		let (state, pcm) = session.parts();
		let until = Instant::now() + Duration::from_secs(4);
		let mut samples = 0;
		let mut first_pcm = None;

		while Instant::now() < until {
			assert!(state.running(), "capture stopped on attempt {attempt}");

			while pcm.captured.pop().is_ok() {
				first_pcm.get_or_insert_with(|| requested.elapsed());

				samples += 1;
			}

			if samples >= 24_000 {
				break;
			}

			thread::sleep(Duration::from_millis(5));
		}

		eprintln!(
			"default_capture attempt={attempt} samples={samples} first_pcm_ms={:.3}",
			first_pcm.expect("microphone input").as_secs_f64() * 1_000.
		);
		assert!(
			first_pcm.unwrap() < Duration::from_secs(1),
			"warm capture must not wait for seconds"
		);

		session.pause().unwrap();

		assert!(samples >= 24_000, "default capture did not deliver one second of audio");

		drop(session);

		thread::sleep(Duration::from_secs(2));
	}
}
