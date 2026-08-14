//! Architecture-level validation-supervision acceptance coverage.

use base64 as _;
use reqwest as _;

use std::{
	ffi::OsString,
	fs,
	path::PathBuf,
	sync::{Arc, Mutex},
	thread,
	time::{Duration, Instant},
};

#[cfg(target_os = "macos")] use core_foundation as _;
use decodex_codex as _;
use decodex_database as _;
use decodex_protocol as _;
use futures_util as _;
#[cfg(target_os = "macos")] use security_framework as _;
use serde as _;
use serde_json as _;
use sha2 as _;
use tokio as _;
use tokio_tungstenite as _;
use zeroize as _;

use decodex_core::RepositoryContentRevision;
use decodex_runtime::{
	ProtectedWorktreeFingerprint, ProtectedWorktreeStateProbe, SupervisedValidationEvidence,
	ValidationAcceptance, ValidationCancellation, ValidationCommandAuthority, ValidationRejection,
	ValidationSupervisionError, ValidationTermination, supervise_validation,
};
use tempfile::TempDir;

const REVISION: &str = "1111111111111111111111111111111111111111";

struct Probe {
	observations: Vec<ProtectedWorktreeFingerprint>,
	next: usize,
}

impl Probe {
	fn stable() -> Self {
		Self { observations: vec![fingerprint(1), fingerprint(1)], next: 0 }
	}
}

impl ProtectedWorktreeStateProbe for Probe {
	fn observe(&mut self) -> Result<ProtectedWorktreeFingerprint, ValidationSupervisionError> {
		let value = self
			.observations
			.get(self.next)
			.or_else(|| self.observations.last())
			.ok_or_else(|| {
				ValidationSupervisionError::StateObservation("fixture exhausted".into())
			})?
			.clone();
		self.next = self.next.saturating_add(1);
		Ok(value)
	}
}

fn fingerprint(marker: u8) -> ProtectedWorktreeFingerprint {
	ProtectedWorktreeFingerprint {
		source_revision: RepositoryContentRevision::new(REVISION)
			.expect("fixture revision is canonical"),
		worktree_state: [marker; 32],
	}
}

fn authority(
	root: &TempDir,
	script: &str,
	deadline: Duration,
	stdout_limit: usize,
	stderr_limit: usize,
) -> ValidationCommandAuthority {
	ValidationCommandAuthority::new(
		PathBuf::from("/bin/sh"),
		vec![OsString::from("-c"), OsString::from(script)],
		root.path().to_owned(),
		vec![(OsString::from("LC_ALL"), OsString::from("C"))],
		Instant::now() + deadline,
		stdout_limit,
		stderr_limit,
		RepositoryContentRevision::new(REVISION).expect("fixture revision is canonical"),
	)
	.expect("validation authority is canonical")
}

fn run(script: &str) -> SupervisedValidationEvidence {
	let root = TempDir::new().expect("temporary validation root is available");
	let authority = authority(&root, script, Duration::from_secs(2), 4_096, 4_096);
	supervise_validation(&authority, &ValidationCancellation::default(), &mut Probe::stable())
		.expect("supervision returns complete evidence")
}

#[test]
fn success_nonzero_and_signal_have_exact_bounded_evidence() {
	let success = run("printf accepted; printf diagnostic >&2");
	assert_eq!(success.before, fingerprint(1));
	assert_eq!(success.after, Some(fingerprint(1)));
	assert_eq!(success.termination, ValidationTermination::Exited(0));
	assert_eq!(success.acceptance, ValidationAcceptance::Accepted);
	assert_eq!(success.stdout, b"accepted");
	assert_eq!(success.stderr, b"diagnostic");

	let nonzero = run("exit 23");
	assert_eq!(nonzero.termination, ValidationTermination::Exited(23));
	assert_eq!(
		nonzero.acceptance,
		ValidationAcceptance::Rejected(ValidationRejection::ProcessFailed)
	);

	let signaled = run("kill -TERM $$");
	assert_eq!(signaled.termination, ValidationTermination::Signaled(libc::SIGTERM));
	assert_eq!(
		signaled.acceptance,
		ValidationAcceptance::Rejected(ValidationRejection::ProcessFailed)
	);
}

#[test]
fn timeout_and_cancellation_fail_closed() {
	let root = TempDir::new().expect("temporary validation root is available");
	let timeout = authority(&root, "sleep 30", Duration::from_millis(120), 4_096, 4_096);
	let evidence =
		supervise_validation(&timeout, &ValidationCancellation::default(), &mut Probe::stable())
			.expect("timeout returns evidence");
	assert_eq!(evidence.termination, ValidationTermination::TimedOut);
	assert_eq!(evidence.acceptance, ValidationAcceptance::Rejected(ValidationRejection::TimedOut));

	let cancellation = ValidationCancellation::default();
	cancellation.cancel();
	let cancelled = authority(&root, "exit 0", Duration::from_secs(1), 4_096, 4_096);
	let evidence = supervise_validation(&cancelled, &cancellation, &mut Probe::stable())
		.expect("pre-spawn cancellation returns evidence");
	assert_eq!(evidence.termination, ValidationTermination::Cancelled);
	assert_eq!(evidence.after, None);
	assert_eq!(evidence.acceptance, ValidationAcceptance::Rejected(ValidationRejection::Cancelled));
}

#[test]
fn output_limits_and_concurrent_protected_state_mutation_override_process_success() {
	let root = TempDir::new().expect("temporary validation root is available");
	let output = authority(&root, "printf 123456789", Duration::from_secs(1), 4, 4_096);
	let evidence =
		supervise_validation(&output, &ValidationCancellation::default(), &mut Probe::stable())
			.expect("output-limit supervision returns evidence");
	assert_eq!(evidence.stdout, b"1234");
	assert_eq!(evidence.termination, ValidationTermination::OutputLimitExceeded);
	assert_eq!(
		evidence.acceptance,
		ValidationAcceptance::Rejected(ValidationRejection::OutputLimitExceeded)
	);

	let mutation = authority(&root, "exit 0", Duration::from_secs(1), 4_096, 4_096);
	let mut probe = Probe { observations: vec![fingerprint(1), fingerprint(2)], next: 0 };
	let evidence = supervise_validation(&mutation, &ValidationCancellation::default(), &mut probe)
		.expect("mutation supervision returns evidence");
	assert_eq!(evidence.termination, ValidationTermination::Exited(0));
	assert_eq!(
		evidence.acceptance,
		ValidationAcceptance::Rejected(ValidationRejection::ProtectedStateMutated)
	);
}

#[test]
fn timeout_tears_down_descendants_and_spawn_failure_returns_no_false_evidence() {
	let root = TempDir::new().expect("temporary validation root is available");
	let pid_path = root.path().join("descendant.pid");
	let script = format!("sleep 30 & child=$!; printf %s $child > {}; wait", pid_path.display());
	let timeout = authority(&root, &script, Duration::from_millis(250), 4_096, 4_096);
	let evidence =
		supervise_validation(&timeout, &ValidationCancellation::default(), &mut Probe::stable())
			.expect("descendant timeout returns evidence");
	assert_eq!(evidence.termination, ValidationTermination::TimedOut);
	let pid = fs::read_to_string(&pid_path)
		.expect("descendant PID was recorded")
		.parse::<i32>()
		.expect("descendant PID is numeric");
	// SAFETY: signal zero observes only the recorded test child identity.
	let result = unsafe { libc::kill(pid, 0) };
	assert_eq!(result, -1);
	assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));

	let missing = ValidationCommandAuthority::new(
		root.path().join("missing-validator"),
		Vec::new(),
		root.path().to_owned(),
		Vec::new(),
		Instant::now() + Duration::from_secs(1),
		4_096,
		4_096,
		RepositoryContentRevision::new(REVISION).expect("fixture revision is canonical"),
	)
	.expect("missing executable is still explicit authority");
	assert!(matches!(
		supervise_validation(&missing, &ValidationCancellation::default(), &mut Probe::stable()),
		Err(ValidationSupervisionError::Spawn(_))
	));
}

#[test]
fn cancellation_during_execution_terminates_the_child_group() {
	let root = TempDir::new().expect("temporary validation root is available");
	let authority = authority(&root, "sleep 30", Duration::from_secs(2), 4_096, 4_096);
	let cancellation = ValidationCancellation::default();
	let trigger = cancellation.clone();
	let evidence = Arc::new(Mutex::new(None));
	let slot = Arc::clone(&evidence);
	let worker = thread::spawn(move || {
		let observed = supervise_validation(&authority, &cancellation, &mut Probe::stable())
			.expect("cancellation returns evidence");
		*slot.lock().expect("evidence lock is available") = Some(observed);
	});
	thread::sleep(Duration::from_millis(80));
	trigger.cancel();
	worker.join().expect("supervisor thread exits");
	let evidence =
		evidence.lock().expect("evidence lock is available").take().expect("evidence was recorded");
	assert_eq!(evidence.termination, ValidationTermination::Cancelled);
	assert_eq!(evidence.acceptance, ValidationAcceptance::Rejected(ValidationRejection::Cancelled));
}
