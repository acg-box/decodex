//! Fail-closed supervision for explicitly authorized project validation commands.
//!
//! This is lifecycle and evidence supervision inside the trusted same-UID V1 boundary, not a
//! sandbox. Callers supply the executable, arguments, working directory, complete environment,
//! absolute deadline, expected source revision, and an exact protected-state probe.

use std::{
	collections::BTreeSet,
	ffi::OsString,
	fmt::{self, Display},
	io::{self, Read},
	os::unix::process::{CommandExt as _, ExitStatusExt as _},
	path::PathBuf,
	process::{Child, Command, ExitStatus, Stdio},
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
	},
	thread,
	time::{Duration, Instant},
};

use decodex_core::RepositoryContentRevision;

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const TERMINATION_GRACE: Duration = Duration::from_millis(100);
const MAX_CAPTURE_BYTES: usize = 4 * 1_024 * 1_024;

/// Explicit, immutable command authority for one supervised validation invocation.
pub struct ValidationCommandAuthority {
	executable: PathBuf,
	argv: Vec<OsString>,
	cwd: PathBuf,
	environment: Vec<(OsString, OsString)>,
	deadline: Instant,
	stdout_limit: usize,
	stderr_limit: usize,
	expected_source_revision: RepositoryContentRevision,
}

impl ValidationCommandAuthority {
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		executable: PathBuf,
		argv: Vec<OsString>,
		cwd: PathBuf,
		environment: Vec<(OsString, OsString)>,
		deadline: Instant,
		stdout_limit: usize,
		stderr_limit: usize,
		expected_source_revision: RepositoryContentRevision,
	) -> Result<Self, ValidationSupervisionError> {
		if !executable.is_absolute() {
			return Err(ValidationSupervisionError::InvalidAuthority(
				"validation executable must be absolute",
			));
		}
		if !cwd.is_absolute() {
			return Err(ValidationSupervisionError::InvalidAuthority(
				"validation working directory must be absolute",
			));
		}
		if deadline <= Instant::now() {
			return Err(ValidationSupervisionError::InvalidAuthority(
				"validation deadline must be in the future",
			));
		}
		if stdout_limit == 0
			|| stderr_limit == 0
			|| stdout_limit > MAX_CAPTURE_BYTES
			|| stderr_limit > MAX_CAPTURE_BYTES
		{
			return Err(ValidationSupervisionError::InvalidAuthority(
				"validation capture limits must be 1..=4194304 bytes",
			));
		}
		let mut names = BTreeSet::new();
		if environment.iter().any(|(name, _)| name.is_empty() || !names.insert(name.clone())) {
			return Err(ValidationSupervisionError::InvalidAuthority(
				"validation environment names must be non-empty and unique",
			));
		}
		Ok(Self {
			executable,
			argv,
			cwd,
			environment,
			deadline,
			stdout_limit,
			stderr_limit,
			expected_source_revision,
		})
	}

	pub fn executable(&self) -> &PathBuf {
		&self.executable
	}

	pub fn argv(&self) -> &[OsString] {
		&self.argv
	}

	pub fn cwd(&self) -> &PathBuf {
		&self.cwd
	}

	pub fn environment(&self) -> &[(OsString, OsString)] {
		&self.environment
	}

	pub fn deadline(&self) -> Instant {
		self.deadline
	}

	pub fn expected_source_revision(&self) -> &RepositoryContentRevision {
		&self.expected_source_revision
	}
}

/// Exact protected source/worktree state observed around a validation command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtectedWorktreeFingerprint {
	pub source_revision: RepositoryContentRevision,
	pub worktree_state: [u8; 32],
}

/// Authority-specific protected-state observer. It must not derive identity from ambient CWD.
pub trait ProtectedWorktreeStateProbe {
	fn observe(&mut self) -> Result<ProtectedWorktreeFingerprint, ValidationSupervisionError>;
}

/// Cooperative cancellation handle. Cancellation terminates the complete child process group.
#[derive(Clone, Default)]
pub struct ValidationCancellation {
	cancelled: Arc<AtomicBool>,
}

impl ValidationCancellation {
	pub fn cancel(&self) {
		self.cancelled.store(true, Ordering::Release);
	}

	pub fn is_cancelled(&self) -> bool {
		self.cancelled.load(Ordering::Acquire)
	}
}

/// Deterministic leader-process classification, or the supervisor reason that preempted it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationTermination {
	Exited(i32),
	Signaled(i32),
	TimedOut,
	Cancelled,
	OutputLimitExceeded,
	SupervisionLost,
}

/// Why validation evidence was not accepted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationRejection {
	ProcessFailed,
	TimedOut,
	Cancelled,
	OutputLimitExceeded,
	ProtectedStateMutated,
	IncompleteEvidence,
}

/// Acceptance classification. Success is evidence for a caller; it grants no mutation authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationAcceptance {
	Accepted,
	Rejected(ValidationRejection),
}

/// Complete bounded observation of one supervised command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupervisedValidationEvidence {
	pub source_revision: RepositoryContentRevision,
	pub before: ProtectedWorktreeFingerprint,
	pub after: Option<ProtectedWorktreeFingerprint>,
	pub termination: ValidationTermination,
	pub acceptance: ValidationAcceptance,
	pub stdout: Vec<u8>,
	pub stderr: Vec<u8>,
}

/// Boundary/configuration error before trustworthy complete evidence can be returned.
#[derive(Debug)]
pub enum ValidationSupervisionError {
	InvalidAuthority(&'static str),
	StateObservation(String),
	Spawn(io::Error),
}

impl Display for ValidationSupervisionError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::InvalidAuthority(reason) => write!(formatter, "invalid validation authority: {reason}"),
			Self::StateObservation(reason) =>
				write!(formatter, "protected-state observation failed: {reason}"),
			Self::Spawn(error) => write!(formatter, "validation process spawn failed: {error}"),
		}
	}
}

impl std::error::Error for ValidationSupervisionError {}

/// Run one explicitly authorized validation command under bounded fail-closed supervision.
pub fn supervise_validation<P: ProtectedWorktreeStateProbe>(
	authority: &ValidationCommandAuthority,
	cancellation: &ValidationCancellation,
	probe: &mut P,
) -> Result<SupervisedValidationEvidence, ValidationSupervisionError> {
	let before = probe.observe()?;
	if before.source_revision != authority.expected_source_revision {
		return Err(ValidationSupervisionError::StateObservation(
			"protected source revision differs from command authority".to_owned(),
		));
	}
	if cancellation.is_cancelled() {
		return Ok(pre_spawn_rejection(
			authority,
			before,
			ValidationTermination::Cancelled,
			ValidationRejection::Cancelled,
		));
	}
	if Instant::now() >= authority.deadline {
		return Ok(pre_spawn_rejection(
			authority,
			before,
			ValidationTermination::TimedOut,
			ValidationRejection::TimedOut,
		));
	}

	let mut command = Command::new(&authority.executable);
	command
		.args(&authority.argv)
		.current_dir(&authority.cwd)
		.env_clear()
		.envs(authority.environment.iter().cloned())
		.stdin(Stdio::null())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped());
	configure_child(&mut command);
	let mut child = command.spawn().map_err(ValidationSupervisionError::Spawn)?;
	let stdout = child.stdout.take().ok_or_else(|| {
		let _ = terminate_process_group(&mut child, authority.deadline);
		ValidationSupervisionError::Spawn(io::Error::other("stdout supervision unavailable"))
	})?;
	let stderr = child.stderr.take().ok_or_else(|| {
		let _ = terminate_process_group(&mut child, authority.deadline);
		ValidationSupervisionError::Spawn(io::Error::other("stderr supervision unavailable"))
	})?;

	let output_exceeded = Arc::new(AtomicBool::new(false));
	let stdout_reader = spawn_capture(stdout, authority.stdout_limit, output_exceeded.clone());
	let stderr_reader = spawn_capture(stderr, authority.stderr_limit, output_exceeded.clone());
	let mut forced = None;
	let status = loop {
		let now = Instant::now();
		let reason = if now >= authority.deadline {
			Some(ValidationTermination::TimedOut)
		} else if cancellation.is_cancelled() {
			Some(ValidationTermination::Cancelled)
		} else if output_exceeded.load(Ordering::Acquire) {
			Some(ValidationTermination::OutputLimitExceeded)
		} else {
			None
		};
		if let Some(reason) = reason {
			forced = Some(reason);
			let status = terminate_process_group(&mut child, authority.deadline).ok();
			break status;
		}
		match child.try_wait() {
			Ok(Some(status)) => {
				// A successful leader may not detach descendants or keep capture pipes alive.
				let _ = signal_group(child.id(), libc::SIGKILL);
				break Some(status);
			},
			Ok(None) => thread::sleep(POLL_INTERVAL.min(authority.deadline - now)),
			Err(_) => {
				forced = Some(ValidationTermination::SupervisionLost);
				let status = terminate_process_group(&mut child, authority.deadline).ok();
				break status;
			},
		}
	};

	let stdout = stdout_reader.join().ok().and_then(Result::ok);
	let stderr = stderr_reader.join().ok().and_then(Result::ok);
	let capture_complete = stdout.is_some() && stderr.is_some();
	let stdout = stdout.unwrap_or_default();
	let stderr = stderr.unwrap_or_default();
	let termination = forced.unwrap_or_else(|| classify_status(status));
	let after = probe.observe().ok();
	let acceptance = classify_acceptance(
		termination,
		capture_complete,
		output_exceeded.load(Ordering::Acquire),
		&before,
		after.as_ref(),
	);

	Ok(SupervisedValidationEvidence {
		source_revision: authority.expected_source_revision.clone(),
		before,
		after,
		termination,
		acceptance,
		stdout,
		stderr,
	})
}

fn pre_spawn_rejection(
	authority: &ValidationCommandAuthority,
	before: ProtectedWorktreeFingerprint,
	termination: ValidationTermination,
	rejection: ValidationRejection,
) -> SupervisedValidationEvidence {
	SupervisedValidationEvidence {
		source_revision: authority.expected_source_revision.clone(),
		before,
		after: None,
		termination,
		acceptance: ValidationAcceptance::Rejected(rejection),
		stdout: Vec::new(),
		stderr: Vec::new(),
	}
}

fn spawn_capture<R: Read + Send + 'static>(
	mut reader: R,
	limit: usize,
	exceeded: Arc<AtomicBool>,
) -> thread::JoinHandle<io::Result<Vec<u8>>> {
	thread::spawn(move || {
		let mut captured = Vec::with_capacity(limit.min(64 * 1_024));
		let mut buffer = [0_u8; 8 * 1_024];
		loop {
			let count = match reader.read(&mut buffer) {
				Ok(0) => break,
				Ok(count) => count,
				Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
				Err(error) => return Err(error),
			};
			let remaining = limit.saturating_sub(captured.len());
			captured.extend_from_slice(&buffer[..count.min(remaining)]);
			if count > remaining {
				exceeded.store(true, Ordering::Release);
			}
		}
		Ok(captured)
	})
}

fn configure_child(command: &mut Command) {
	// SAFETY: the closure runs after fork and before exec and calls only async-signal-safe libc.
	unsafe {
		command.pre_exec(|| {
			libc::umask(0o077);
			if libc::setpgid(0, 0) == -1 {
				return Err(io::Error::last_os_error());
			}
			Ok(())
		});
	}
}

fn terminate_process_group(child: &mut Child, deadline: Instant) -> io::Result<ExitStatus> {
	let _ = signal_group(child.id(), libc::SIGTERM);
	let grace_deadline = deadline.min(Instant::now() + TERMINATION_GRACE);
	loop {
		match child.try_wait()? {
			Some(status) => {
				let _ = signal_group(child.id(), libc::SIGKILL);
				return Ok(status);
			},
			None if Instant::now() < grace_deadline => thread::sleep(POLL_INTERVAL),
			None => break,
		}
	}
	let _ = signal_group(child.id(), libc::SIGKILL);
	child.wait()
}

fn signal_group(pid: u32, signal: i32) -> io::Result<()> {
	let pid = i32::try_from(pid).map_err(|_| io::Error::other("invalid child process identity"))?;
	// SAFETY: the negative PID addresses only the group created in `pre_exec`.
	let result = unsafe { libc::kill(-pid, signal) };
	if result == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
		Ok(())
	} else {
		Err(io::Error::last_os_error())
	}
}

fn classify_status(status: Option<ExitStatus>) -> ValidationTermination {
	match status {
		Some(status) if status.code().is_some() =>
			ValidationTermination::Exited(status.code().expect("exit code checked")),
		Some(status) if status.signal().is_some() =>
			ValidationTermination::Signaled(status.signal().expect("signal checked")),
		Some(_) | None => ValidationTermination::SupervisionLost,
	}
}

fn classify_acceptance(
	termination: ValidationTermination,
	capture_complete: bool,
	output_exceeded: bool,
	before: &ProtectedWorktreeFingerprint,
	after: Option<&ProtectedWorktreeFingerprint>,
) -> ValidationAcceptance {
	if after.is_some_and(|after| after != before) {
		return ValidationAcceptance::Rejected(ValidationRejection::ProtectedStateMutated);
	}
	if after.is_none() || !capture_complete {
		return ValidationAcceptance::Rejected(ValidationRejection::IncompleteEvidence);
	}
	if output_exceeded || termination == ValidationTermination::OutputLimitExceeded {
		return ValidationAcceptance::Rejected(ValidationRejection::OutputLimitExceeded);
	}
	match termination {
		ValidationTermination::Exited(0) => ValidationAcceptance::Accepted,
		ValidationTermination::TimedOut =>
			ValidationAcceptance::Rejected(ValidationRejection::TimedOut),
		ValidationTermination::Cancelled =>
			ValidationAcceptance::Rejected(ValidationRejection::Cancelled),
		ValidationTermination::SupervisionLost =>
			ValidationAcceptance::Rejected(ValidationRejection::IncompleteEvidence),
		ValidationTermination::OutputLimitExceeded =>
			ValidationAcceptance::Rejected(ValidationRejection::OutputLimitExceeded),
		ValidationTermination::Exited(_) | ValidationTermination::Signaled(_) =>
			ValidationAcceptance::Rejected(ValidationRejection::ProcessFailed),
	}
}
