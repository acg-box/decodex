use super::*;

#[test]
fn cancelled_queued_start_does_not_open_hardware() {
	let owner = Owner::new().unwrap();
	assert!(matches!(
		owner.start(Key { device: u32::MAX, mode: Mode::Live }, Arc::new(AtomicBool::new(true))),
		Err(AudioError::Cancelled)
	));
}

#[test]
#[ignore = "Requires authorized microphone and DECODEX_AUDIO_TEST_DEVICE_ID"]
fn production_owner_captures_across_modes_and_stops_before_transport_teardown() {
	let device = std::env::var("DECODEX_AUDIO_TEST_DEVICE_ID")
		.expect("physical input device ID")
		.parse::<u32>()
		.unwrap();
	assert_ne!(device, 0);
	let owner = Owner::new().unwrap();
	for mode in [Mode::Dictation, Mode::Dictation, Mode::Live, Mode::Dictation, Mode::Live] {
		let requested = std::time::Instant::now();
		let mut first = true;
		let mut session =
			owner.start(Key { device, mode }, Arc::new(AtomicBool::new(false))).unwrap();
		let (state, pcm) = session.parts();
		let until = std::time::Instant::now() + Duration::from_secs(2);
		let mut captured = 0;
		let mut rendered = 0;
		while std::time::Instant::now() < until {
			assert!(state.running());
			while pcm.captured.pop().is_ok() {
				if first {
					eprintln!(
						"audio_first_pcm_ms={:.3} mode={}",
						requested.elapsed().as_secs_f64() * 1000.,
						mode.rate()
					);
					first = false;
				}
				captured += 1;
			}
			while pcm.playback.push(0.).is_ok() {
				rendered += 1;
			}
			thread::sleep(Duration::from_millis(10));
		}
		assert!(captured > mode.rate() as usize);
		if mode == Mode::Live {
			assert!(rendered - (4_800 - pcm.playback.slots()) > mode.rate() as usize);
		}
		session.pause().unwrap();
		assert!(!state.running());
	}
	let cancelled = Arc::new(AtomicBool::new(false));
	let mut old = owner.start(Key { device, mode: Mode::Live }, cancelled.clone()).unwrap();
	let (old_state, _) = old.parts();
	assert!(matches!(
		owner.start(Key { device, mode: Mode::Dictation }, Arc::new(AtomicBool::new(false))),
		Err(AudioError::Busy)
	));
	cancelled.store(true, Ordering::Release);
	let deadline = std::time::Instant::now() + Duration::from_secs(1);
	while old_state.running() && std::time::Instant::now() < deadline {
		thread::sleep(Duration::from_millis(5));
	}
	assert!(
		!old_state.running(),
		"capture must stop while the old transport still retains its session"
	);
	let mut next = owner
		.start(Key { device, mode: Mode::Dictation }, Arc::new(AtomicBool::new(false)))
		.unwrap();
	let (next_state, _) = next.parts();
	drop(old); // A late old release must not stop the new session.
	thread::sleep(Duration::from_millis(50));
	assert!(next_state.running());
}

#[test]
#[ignore = "Requires authorized microphone; change system routes during each 25-second capture"]
fn production_owner_keeps_capturing_through_system_route_changes() {
	let owner = Owner::new().unwrap();
	for mode in [Mode::Dictation, Mode::Live] {
		let mut session =
			owner.start(Key { device: 0, mode }, Arc::new(AtomicBool::new(false))).unwrap();
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
			max_gap.as_secs_f64() * 1000.
		);
		session.pause().unwrap();
	}
}

#[test]
#[ignore = "Requires authorized microphone and DECODEX_AUDIO_TEST_DEVICE_ID"]
fn prepared_modes_start_without_recording_during_preparation() {
	let device = std::env::var("DECODEX_AUDIO_TEST_DEVICE_ID").unwrap().parse().unwrap();
	let owner = Owner::new().unwrap();
	for mode in [Mode::Dictation, Mode::Live] {
		owner.commands.send(Command::Prepare(Key { device, mode })).unwrap();
	}
	let (reply, ready) = mpsc::sync_channel(1);
	owner.commands.send(Command::CheckPrepared(reply)).unwrap();
	assert!(ready.recv_timeout(Duration::from_secs(15)).unwrap(), "preparation must not record");
	for mode in [Mode::Dictation, Mode::Live, Mode::Dictation, Mode::Live] {
		let requested = Instant::now();
		let mut session =
			owner.start(Key { device, mode }, Arc::new(AtomicBool::new(false))).unwrap();
		let (state, pcm) = session.parts();
		while pcm.captured.pop().is_err() {
			assert!(state.running());
			assert!(requested.elapsed() < Duration::from_secs(3));
			thread::sleep(Duration::from_millis(2));
		}
		eprintln!(
			"prepared_first_pcm_ms={:.3} mode={}",
			requested.elapsed().as_secs_f64() * 1000.,
			mode.rate()
		);
		session.pause().unwrap();
	}
}

#[test]
#[ignore = "Requires authorized microphone; checks the current default without changing routes"]
fn default_dictation_delivers_pcm_after_prepared_engines_settle() {
	let owner = Owner::new().unwrap();
	owner.commands.send(Command::Prepare(Key { device: 0, mode: Mode::Dictation })).unwrap();
	let (reply, cold) = mpsc::sync_channel(1);
	owner.commands.send(Command::CheckCaptureChannels(reply)).unwrap();
	let expected_channels = cold.recv_timeout(Duration::from_secs(15)).unwrap().unwrap();
	for mode in [Mode::Dictation, Mode::Live] {
		owner.commands.send(Command::Prepare(Key { device: 0, mode })).unwrap();
	}
	let (reply, ready) = mpsc::sync_channel(1);
	owner.commands.send(Command::CheckPrepared(reply)).unwrap();
	assert!(ready.recv_timeout(Duration::from_secs(15)).unwrap());
	thread::sleep(Duration::from_secs(8));
	for attempt in 0..5 {
		let requested = Instant::now();
		let mut session = owner
			.start(Key { device: 0, mode: Mode::Dictation }, Arc::new(AtomicBool::new(false)))
			.unwrap();
		let (reply, channels) = mpsc::sync_channel(1);
		owner.commands.send(Command::CheckCaptureChannels(reply)).unwrap();
		assert_eq!(
			channels.recv_timeout(Duration::from_secs(15)).unwrap(),
			Some(expected_channels),
			"live preparation must not add echo-reference channels to dictation"
		);
		let (state, pcm) = session.parts();
		let mut samples = 0;
		let until = Instant::now() + Duration::from_secs(4);
		while Instant::now() < until {
			assert!(state.running(), "capture stopped on attempt {attempt}");
			while pcm.captured.pop().is_ok() {
				samples += 1;
			}
			if samples >= 24_000 {
				break;
			}
			thread::sleep(Duration::from_millis(5));
		}
		eprintln!(
			"default_capture attempt={attempt} samples={samples} elapsed_ms={}",
			requested.elapsed().as_millis()
		);
		session.pause().unwrap();
		assert!(samples >= 24_000, "default capture did not deliver one second of audio");
		drop(session);
		thread::sleep(Duration::from_secs(2));
	}
}
