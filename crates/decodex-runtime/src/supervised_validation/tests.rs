use std::{sync::mpsc::sync_channel, time::Instant};

use super::{
	CaptureEvent, CaptureState, CaptureStream, ProtectedWorktreeFingerprint, ValidationAcceptance,
	ValidationRejection, ValidationTermination, classify_acceptance,
};
use decodex_core::RepositoryContentRevision;

fn fingerprint() -> ProtectedWorktreeFingerprint {
	ProtectedWorktreeFingerprint {
		source_revision: RepositoryContentRevision::new(
			"1111111111111111111111111111111111111111",
		)
		.expect("fixture revision is canonical"),
		worktree_state: [1; 32],
	}
}

#[test]
fn capture_failure_is_incomplete_evidence_even_after_successful_exit() {
	let (sender, receiver) = sync_channel(4);
	sender.send(CaptureEvent::Failed(CaptureStream::Stdout)).expect("failure is queued");
	sender.send(CaptureEvent::Eof(CaptureStream::Stderr)).expect("EOF is queued");
	drop(sender);
	let mut capture = CaptureState::new(receiver, 128, 128);
	capture.drain_bounded(Instant::now() + std::time::Duration::from_secs(1));

	assert!(capture.settled());
	assert!(!capture.complete());
	assert_eq!(
		classify_acceptance(
			ValidationTermination::Exited(0),
			capture.complete(),
			capture.output_exceeded,
			&fingerprint(),
			Some(&fingerprint()),
		),
		ValidationAcceptance::Rejected(ValidationRejection::IncompleteEvidence)
	);
}

#[test]
fn capture_drain_processes_a_bounded_event_batch() {
	let (sender, receiver) = sync_channel(64);
	for _ in 0..40 {
		sender.send(CaptureEvent::Chunk(CaptureStream::Stdout, vec![b'x']))
			.expect("chunk is queued");
	}
	drop(sender);
	let mut capture = CaptureState::new(receiver, 128, 128);
	capture.drain_bounded(Instant::now() + std::time::Duration::from_secs(1));

	assert!(capture.stdout.len() <= 32);
	assert!(!capture.settled());
}
