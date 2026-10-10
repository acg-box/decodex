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
	for mode in [Mode::Dictation, Mode::Dictation, Mode::Live, Mode::Dictation] {
		let mut session =
			owner.start(Key { device, mode }, Arc::new(AtomicBool::new(false))).unwrap();
		let (state, pcm) = session.parts();
		let until = std::time::Instant::now() + Duration::from_secs(2);
		let mut captured = 0;
		let mut rendered = 0;
		while std::time::Instant::now() < until {
			assert!(state.running());
			while pcm.captured.pop().is_ok() {
				captured += 1;
			}
			while pcm.playback.push(0.).is_ok() {
				rendered += 1;
			}
			thread::sleep(Duration::from_millis(10));
		}
		assert!(captured > mode.rate() as usize);
		assert!(rendered - (4_800 - pcm.playback.slots()) > mode.rate() as usize);
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
