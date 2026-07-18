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
		mpsc::{Receiver, SyncSender, TryRecvError, sync_channel},
	},
	thread,
	time::{Duration, Instant},
};

use decodex_core::RepositoryContentRevision;

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const TERMINATION_GRACE: Duration = Duration::from_millis(100);
const MAX_CAPTURE_BYTES: usize = 4 * 1_024 * 1_024;
const MAX_DRAIN_EVENTS: usize = 32;
const MAX_DRAIN_BYTES: usize = 256 * 1_024;
const MAX_DRAIN_TIME: Duration = Duration::from_millis(2);

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
	let stdout = match child.stdout.take() {
		Some(stdout) => stdout,
		None => return Err(missing_capture(&mut child, authority.deadline, "stdout")),
	};
	let stderr = match child.stderr.take() {
		Some(stderr) => stderr,
		None => return Err(missing_capture(&mut child, authority.deadline, "stderr")),
	};

	let (capture_sender, capture_receiver) = sync_channel(16);
	let _stdout_reader = spawn_capture(stdout, CaptureStream::Stdout, capture_sender.clone());
	let _stderr_reader = spawn_capture(stderr, CaptureStream::Stderr, capture_sender);
	let mut capture = CaptureState::new(
		capture_receiver,
		authority.stdout_limit,
		authority.stderr_limit,
	);
	let mut forced = None;
	let recorded_status = loop {
		// Observe leader completion first. Once recorded, no later supervisor event may replace it.
		match child.try_wait() {
			Ok(Some(status)) => break Some(status),
			Ok(None) => {},
			Err(_) => {
				forced = Some(ValidationTermination::SupervisionLost);
				break None;
			},
		}
		capture.drain_bounded(authority.deadline);
		let now = Instant::now();
		let supervisor_event = if capture.output_exceeded {
			Some(ValidationTermination::OutputLimitExceeded)
		} else if now >= authority.deadline {
			Some(ValidationTermination::TimedOut)
		} else if cancellation.is_cancelled() {
			Some(ValidationTermination::Cancelled)
		} else {
			None
		};
		if let Some(supervisor_event) = supervisor_event {
			// Close the observation gap between the iteration's first poll and committing a
			// forced outcome. Any exit observed here has already happened and remains authoritative.
			match child.try_wait() {
				Ok(Some(status)) => break Some(status),
				Ok(None) => forced = Some(supervisor_event),
				Err(_) => forced = Some(ValidationTermination::SupervisionLost),
			}
			break None;
		}
		sleep_bounded(authority.deadline);
	};
	let teardown = teardown_process_group(&mut child, authority.deadline, recorded_status);
	while teardown.confirmed
		&& !capture.output_exceeded
		&& !capture.settled()
		&& Instant::now() < authority.deadline
	{
		capture.drain_bounded(authority.deadline);
		if !capture.settled() {
			sleep_bounded(authority.deadline);
		}
	}
	let termination = if teardown.confirmed {
		if teardown.observed_before_signal || forced.is_none() {
			classify_status(teardown.status)
		} else {
			forced.expect("forced termination checked")
		}
	} else {
		ValidationTermination::SupervisionLost
	};
	let after = probe.observe().ok();
	let acceptance = classify_acceptance(
		termination,
		capture.complete(),
		capture.output_exceeded,
		&before,
		after.as_ref(),
	);

	Ok(SupervisedValidationEvidence {
		source_revision: authority.expected_source_revision.clone(),
		before,
		after,
		termination,
		acceptance,
		stdout: capture.stdout,
		stderr: capture.stderr,
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

#[derive(Clone, Copy)]
enum CaptureStream {
	Stdout,
	Stderr,
}

enum CaptureEvent {
	Chunk(CaptureStream, Vec<u8>),
	Eof(CaptureStream),
	Failed(CaptureStream),
}

struct CaptureState {
	receiver: Receiver<CaptureEvent>,
	stdout: Vec<u8>,
	stderr: Vec<u8>,
	stdout_limit: usize,
	stderr_limit: usize,
	stdout_done: bool,
	stderr_done: bool,
	failed: bool,
	output_exceeded: bool,
}

impl CaptureState {
	fn new(receiver: Receiver<CaptureEvent>, stdout_limit: usize, stderr_limit: usize) -> Self {
		Self {
			receiver,
			stdout: Vec::with_capacity(stdout_limit.min(64 * 1_024)),
			stderr: Vec::with_capacity(stderr_limit.min(64 * 1_024)),
			stdout_limit,
			stderr_limit,
			stdout_done: false,
			stderr_done: false,
			failed: false,
			output_exceeded: false,
		}
	}

	fn drain_bounded(&mut self, deadline: Instant) {
		let drain_deadline = deadline.min(Instant::now() + MAX_DRAIN_TIME);
		let mut events = 0;
		let mut bytes = 0;
		while events < MAX_DRAIN_EVENTS
			&& bytes < MAX_DRAIN_BYTES
			&& Instant::now() < drain_deadline
			&& !self.output_exceeded
		{
			match self.receiver.try_recv() {
				Ok(CaptureEvent::Chunk(stream, chunk)) => {
					events += 1;
					bytes = bytes.saturating_add(chunk.len());
					self.append(stream, &chunk);
				},
				Ok(CaptureEvent::Eof(stream)) => {
					events += 1;
					self.mark_done(stream);
				},
				Ok(CaptureEvent::Failed(stream)) => {
					events += 1;
					self.failed = true;
					self.mark_done(stream);
				},
				Err(TryRecvError::Empty) => break,
				Err(TryRecvError::Disconnected) => {
					if !self.stdout_done || !self.stderr_done {
						self.failed = true;
					}
					break;
				},
			}
		}
	}

	fn append(&mut self, stream: CaptureStream, bytes: &[u8]) {
		let (output, limit) = match stream {
			CaptureStream::Stdout => (&mut self.stdout, self.stdout_limit),
			CaptureStream::Stderr => (&mut self.stderr, self.stderr_limit),
		};
		let remaining = limit.saturating_sub(output.len());
		output.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
		self.output_exceeded |= bytes.len() > remaining;
	}

	fn mark_done(&mut self, stream: CaptureStream) {
		match stream {
			CaptureStream::Stdout => self.stdout_done = true,
			CaptureStream::Stderr => self.stderr_done = true,
		}
	}

	fn complete(&self) -> bool {
		self.stdout_done && self.stderr_done && !self.failed
	}

	fn settled(&self) -> bool {
		self.stdout_done && self.stderr_done
	}
}

fn spawn_capture<R: Read + Send + 'static>(
	mut reader: R,
	stream: CaptureStream,
	sender: SyncSender<CaptureEvent>,
) -> thread::JoinHandle<()> {
	thread::spawn(move || {
		let mut buffer = [0_u8; 8 * 1_024];
		loop {
			let count = match reader.read(&mut buffer) {
				Ok(0) => {
					let _ = sender.send(CaptureEvent::Eof(stream));
					return;
				},
				Ok(count) => count,
				Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
				Err(_) => {
					let _ = sender.send(CaptureEvent::Failed(stream));
					return;
				},
			};
			if sender.send(CaptureEvent::Chunk(stream, buffer[..count].to_vec())).is_err() {
				return;
			}
		}
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

struct TeardownOutcome {
	status: Option<ExitStatus>,
	confirmed: bool,
	observed_before_signal: bool,
}

fn missing_capture(
	child: &mut Child,
	deadline: Instant,
	stream: &'static str,
) -> ValidationSupervisionError {
	let teardown = teardown_process_group(child, deadline, None);
	let suffix = if teardown.confirmed { "" } else { "; teardown unconfirmed" };
	ValidationSupervisionError::Spawn(io::Error::other(format!(
		"{stream} supervision unavailable{suffix}"
	)))
}

fn teardown_process_group(
	child: &mut Child,
	deadline: Instant,
	recorded_status: Option<ExitStatus>,
) -> TeardownOutcome {
	let pid = child.id();
	let mut status = recorded_status;
	poll_status(child, &mut status);
	let observed_before_signal = status.is_some();
	let grace_deadline = deadline.min(Instant::now() + TERMINATION_GRACE);
	if Instant::now() < deadline {
		let _ = signal_group_until(pid, libc::SIGTERM, deadline);
	}
	loop {
		poll_status(child, &mut status);
		if group_gone(pid, deadline) && status.is_some() {
			return TeardownOutcome { status, confirmed: true, observed_before_signal };
		}
		let now = Instant::now();
		if now >= grace_deadline {
			break;
		}
		sleep_bounded(grace_deadline);
	}

	// Always attempt both group and leader SIGKILL paths. Neither call is allowed to extend the
	// command authority's single absolute deadline.
	let _ = signal_group_until(pid, libc::SIGKILL, deadline);
	let _ = child.kill();
	loop {
		poll_status(child, &mut status);
		if group_gone(pid, deadline) && status.is_some() {
			return TeardownOutcome { status, confirmed: true, observed_before_signal };
		}
		let now = Instant::now();
		if now >= deadline {
			break;
		}
		let _ = signal_group_until(pid, libc::SIGKILL, deadline);
		sleep_bounded(deadline);
	}
	// One final nonblocking observation at the deadline; never call blocking `wait` or `join`.
	poll_status(child, &mut status);
	let confirmed = group_gone(pid, deadline) && status.is_some();
	TeardownOutcome { status, confirmed, observed_before_signal }
}

fn poll_status(child: &mut Child, status: &mut Option<ExitStatus>) {
	if status.is_none() {
		if let Ok(Some(observed)) = child.try_wait() {
			*status = Some(observed);
		}
	}
}

fn signal_group_until(pid: u32, signal: i32, deadline: Instant) -> io::Result<()> {
	let pid = i32::try_from(pid).map_err(|_| io::Error::other("invalid child process identity"))?;
	loop {
		// SAFETY: the negative PID addresses only the group created in `pre_exec`.
		if unsafe { libc::kill(-pid, signal) } == 0 {
			return Ok(());
		}
		let error = io::Error::last_os_error();
		match error.raw_os_error() {
			Some(libc::ESRCH) => return Ok(()),
			Some(libc::EINTR) if Instant::now() < deadline => continue,
			_ => return Err(error),
		}
	}
}

fn group_gone(pid: u32, deadline: Instant) -> bool {
	let Ok(pid) = i32::try_from(pid) else { return false };
	loop {
		// SAFETY: signal zero observes only the child-created process group.
		if unsafe { libc::kill(-pid, 0) } == 0 {
			return false;
		}
		match io::Error::last_os_error().raw_os_error() {
			Some(libc::ESRCH) => return true,
			Some(libc::EINTR) if Instant::now() < deadline => continue,
			_ => return false,
		}
	}
}

fn sleep_bounded(deadline: Instant) {
	if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
		thread::sleep(POLL_INTERVAL.min(remaining));
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

#[cfg(test)]
#[path = "supervised_validation/tests.rs"]
mod tests;
