//! Narrow supported-OS process identity, session setup, signaling, and exit observation.
//!
//! Absence and identity mismatch are diagnostic observations only. Only an attached kernel
//! witness can return positive exit evidence.

use std::{
	fmt::{Display, Formatter},
	io,
	os::{
		fd::{AsRawFd as _, FromRawFd as _, OwnedFd},
		unix::process::CommandExt as _,
	},
	process::Command,
	sync::atomic::{AtomicBool, Ordering},
};

#[cfg(target_os = "linux")] use std::fs;
#[cfg(target_os = "macos")] use std::mem::{self, MaybeUninit};

use decodex_core::{
	ProcessBootIdentity, ProcessDeathEvidenceKind, ProcessIdentity, ProcessStartIdentity,
};

/// A supported-OS observation that never interprets absence as death.
#[derive(Debug)]
pub(crate) enum ExactProcessObservation {
	/// A kernel exit source is attached to the exact durable process identity.
	Attached(KernelExitWitness),
	/// No process could be inspected at the requested PID. This is not death evidence.
	NotObserved,
	/// The PID currently names different boot, start, group, or session facts.
	IdentityMismatch {
		/// Current facts, when the operating system exposed a complete identity.
		observed: ProcessIdentity,
	},
}

/// An OS-owned exit source attached only after exact identity validation.
#[derive(Debug)]
pub(crate) struct KernelExitWitness {
	descriptor: OwnedFd,
	identity: ProcessIdentity,
	kind: ProcessDeathEvidenceKind,
	positive_exit: AtomicBool,
}
impl KernelExitWitness {
	/// Return the exact durable identity to which this witness was attached.
	pub(crate) fn identity(&self) -> &ProcessIdentity {
		&self.identity
	}

	/// Return the pending evidence kind after this exact witness observed a positive event.
	///
	/// Process-group quiescence is still required before the kind can become durable evidence.
	pub(crate) fn positive_exit_kind(&self) -> Option<ProcessDeathEvidenceKind> {
		self.positive_exit.load(Ordering::Acquire).then_some(self.kind)
	}

	/// Poll without blocking. `None` means no positive exit event is available.
	pub(crate) fn try_positive_exit(
		&self,
	) -> Result<Option<ProcessDeathEvidenceKind>, ProcessPlatformError> {
		if self.positive_exit.load(Ordering::Acquire) {
			return Ok(Some(self.kind));
		}
		#[cfg(target_os = "linux")]
		{
			let mut descriptor =
				libc::pollfd { fd: self.descriptor.as_raw_fd(), events: libc::POLLIN, revents: 0 };
			// SAFETY: `descriptor` is valid for one element and timeout zero does not block.
			let result = unsafe { libc::poll(&mut descriptor, 1, 0) };
			if result == -1 {
				return Err(ProcessPlatformError::Observation(io::Error::last_os_error()));
			}
			if result == 1
				&& descriptor.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0
			{
				self.positive_exit.store(true, Ordering::Release);
				return Ok(Some(self.kind));
			}
			return Ok(None);
		}

		#[cfg(target_os = "macos")]
		{
			let mut event = MaybeUninit::<libc::kevent>::uninit();
			let timeout = libc::timespec { tv_sec: 0, tv_nsec: 0 };
			// SAFETY: the kqueue descriptor is owned, the output has capacity one, and the
			// zero timeout makes this a nonblocking observation.
			let result = unsafe {
				libc::kevent(
					self.descriptor.as_raw_fd(),
					std::ptr::null(),
					0,
					event.as_mut_ptr(),
					1,
					&timeout,
				)
			};
			if result == -1 {
				return Err(ProcessPlatformError::Observation(io::Error::last_os_error()));
			}
			if result == 1 {
				// SAFETY: `kevent` initialized one output event.
				let event = unsafe { event.assume_init() };
				if event.flags & libc::EV_ERROR != 0 {
					return Err(ProcessPlatformError::Observation(io::Error::other(
						"macOS process witness returned an error event",
					)));
				}
				if event.ident == self.identity.process_id as libc::uintptr_t
					&& event.filter == libc::EVFILT_PROC
					&& event.fflags & libc::NOTE_EXIT != 0
				{
					self.positive_exit.store(true, Ordering::Release);
					return Ok(Some(self.kind));
				}
			}
			Ok(None)
		}

		#[cfg(not(any(target_os = "linux", target_os = "macos")))]
		Err(ProcessPlatformError::Unsupported)
	}
}

/// Closed supported-OS adapter failure.
#[derive(Debug)]
pub(crate) enum ProcessPlatformError {
	/// This build target has no accepted ProcessGeneration adapter.
	#[cfg(not(any(target_os = "linux", target_os = "macos")))]
	Unsupported,
	/// Boot identity could not be read.
	BootIdentity(io::Error),
	/// Process identity could not be read safely.
	ProcessIdentity(io::Error),
	/// A kernel exit witness could not be attached or polled.
	Observation(io::Error),
	/// Exact owned-process signaling failed.
	Signal(io::Error),
}
impl std::error::Error for ProcessPlatformError {}
impl Display for ProcessPlatformError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			#[cfg(not(any(target_os = "linux", target_os = "macos")))]
			Self::Unsupported => formatter.write_str("the host has no accepted ProcessGeneration adapter"),
			Self::BootIdentity(error) => write!(formatter, "boot identity failed: {error}"),
			Self::ProcessIdentity(error) => write!(formatter, "process identity failed: {error}"),
			Self::Observation(error) =>
				write!(formatter, "process exit observation failed: {error}"),
			Self::Signal(error) => write!(formatter, "exact owned-process signal failed: {error}"),
		}
	}
}

/// Return the exact current supported-OS boot identity.
pub(crate) fn current_boot_identity() -> Result<ProcessBootIdentity, ProcessPlatformError> {
	#[cfg(target_os = "linux")]
	{
		let value = fs::read_to_string("/proc/sys/kernel/random/boot_id")
			.map_err(ProcessPlatformError::BootIdentity)?;
		return ProcessBootIdentity::new(format!("linux:{}", value.trim()))
			.map_err(|_| ProcessPlatformError::BootIdentity(invalid_identity()));
	}

	#[cfg(target_os = "macos")]
	{
		let mut name = [libc::CTL_KERN, libc::KERN_BOOTTIME];
		let mut boot = MaybeUninit::<libc::timeval>::uninit();
		let mut length = mem::size_of::<libc::timeval>();
		// SAFETY: the MIB requests one fixed `timeval` and `length` describes its storage.
		let result = unsafe {
			libc::sysctl(
				name.as_mut_ptr(),
				name.len() as libc::c_uint,
				boot.as_mut_ptr().cast(),
				&mut length,
				std::ptr::null_mut(),
				0,
			)
		};
		if result == -1 || length != mem::size_of::<libc::timeval>() {
			return Err(ProcessPlatformError::BootIdentity(io::Error::last_os_error()));
		}
		// SAFETY: successful fixed-size `sysctl` initialized the value.
		let boot = unsafe { boot.assume_init() };
		ProcessBootIdentity::new(format!("macos:{}:{}", boot.tv_sec, boot.tv_usec))
			.map_err(|_| ProcessPlatformError::BootIdentity(invalid_identity()))
	}

	#[cfg(not(any(target_os = "linux", target_os = "macos")))]
	Err(ProcessPlatformError::Unsupported)
}

/// Read one complete exact process identity. Missing processes return `None` without proof.
pub(crate) fn inspect_process_identity(
	process_id: u32,
	boot_id: &ProcessBootIdentity,
) -> Result<Option<ProcessIdentity>, ProcessPlatformError> {
	#[cfg(target_os = "linux")]
	{
		let stat = match fs::read_to_string(format!("/proc/{process_id}/stat")) {
			Ok(stat) => stat,
			Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
			Err(error) => return Err(ProcessPlatformError::ProcessIdentity(error)),
		};
		let close = stat.rfind(')').ok_or_else(|| {
			ProcessPlatformError::ProcessIdentity(io::Error::new(
				io::ErrorKind::InvalidData,
				"Linux process stat has no command terminator",
			))
		})?;
		let open = stat.find('(').ok_or_else(|| {
			ProcessPlatformError::ProcessIdentity(io::Error::new(
				io::ErrorKind::InvalidData,
				"Linux process stat has no command prefix",
			))
		})?;
		if parse_u32(stat[..open].trim())? != process_id {
			return Err(ProcessPlatformError::ProcessIdentity(invalid_identity()));
		}
		let fields = stat[close + 1..].split_ascii_whitespace().collect::<Vec<_>>();
		if fields.len() <= 19 {
			return Err(ProcessPlatformError::ProcessIdentity(io::Error::new(
				io::ErrorKind::InvalidData,
				"Linux process stat is incomplete",
			)));
		}
		let process_group_id = parse_u32(fields[2])?;
		let session_id = parse_u32(fields[3])?;
		let start_ticks = fields[19].parse::<u64>().map_err(|_| {
			ProcessPlatformError::ProcessIdentity(io::Error::new(
				io::ErrorKind::InvalidData,
				"Linux process start identity is invalid",
			))
		})?;
		ProcessIdentity::new(
			boot_id.clone(),
			process_id,
			ProcessStartIdentity::new(format!("linux-ticks:{start_ticks}"))
				.map_err(|_| ProcessPlatformError::ProcessIdentity(invalid_identity()))?,
			process_group_id,
			session_id,
		)
		.map(Some)
		.map_err(|_| ProcessPlatformError::ProcessIdentity(invalid_identity()))
	}

	#[cfg(target_os = "macos")]
	{
		let pid = i32::try_from(process_id)
			.map_err(|_| ProcessPlatformError::ProcessIdentity(invalid_identity()))?;
		let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
		let size = i32::try_from(mem::size_of::<libc::proc_bsdinfo>())
			.map_err(|_| ProcessPlatformError::ProcessIdentity(invalid_identity()))?;
		// SAFETY: the flavor and fixed output size match `proc_bsdinfo`.
		let result = unsafe {
			libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, info.as_mut_ptr().cast(), size)
		};
		if result == 0 {
			return Ok(None);
		}
		if result != size {
			return Err(ProcessPlatformError::ProcessIdentity(io::Error::last_os_error()));
		}
		// SAFETY: `proc_pidinfo` returned the complete fixed-size structure.
		let info = unsafe { info.assume_init() };
		// SAFETY: `getsid` performs a read-only process lookup.
		let session_id = unsafe { libc::getsid(pid) };
		if session_id == -1 {
			let error = io::Error::last_os_error();
			if error.raw_os_error() == Some(libc::ESRCH) {
				return Ok(None);
			}
			return Err(ProcessPlatformError::ProcessIdentity(error));
		}
		ProcessIdentity::new(
			boot_id.clone(),
			process_id,
			ProcessStartIdentity::new(format!(
				"macos-time:{}:{}",
				info.pbi_start_tvsec, info.pbi_start_tvusec
			))
			.map_err(|_| ProcessPlatformError::ProcessIdentity(invalid_identity()))?,
			info.pbi_pgid,
			u32::try_from(session_id)
				.map_err(|_| ProcessPlatformError::ProcessIdentity(invalid_identity()))?,
		)
		.map(Some)
		.map_err(|_| ProcessPlatformError::ProcessIdentity(invalid_identity()))
	}

	#[cfg(not(any(target_os = "linux", target_os = "macos")))]
	{
		let _ = (process_id, boot_id);
		Err(ProcessPlatformError::Unsupported)
	}
}

/// Attach a kernel witness only when the live process matches every persisted identity field.
pub(crate) fn attach_exit_witness(
	expected: &ProcessIdentity,
) -> Result<ExactProcessObservation, ProcessPlatformError> {
	let boot = current_boot_identity()?;
	if boot != expected.boot_id {
		return Ok(ExactProcessObservation::NotObserved);
	}
	match inspect_process_identity(expected.process_id, &boot)? {
		Some(observed) if observed == *expected => {},
		Some(observed) => return Ok(ExactProcessObservation::IdentityMismatch { observed }),
		None => return Ok(ExactProcessObservation::NotObserved),
	}

	#[cfg(target_os = "linux")]
	let (descriptor, kind) = {
		let pid = i32::try_from(expected.process_id)
			.map_err(|_| ProcessPlatformError::Observation(invalid_identity()))?;
		// SAFETY: `pidfd_open` receives one positive persisted PID and zero flags.
		let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
		if raw == -1 {
			let error = io::Error::last_os_error();
			if error.raw_os_error() == Some(libc::ESRCH) {
				return Ok(ExactProcessObservation::NotObserved);
			}
			return Err(ProcessPlatformError::Observation(error));
		}
		// SAFETY: successful `pidfd_open` returns a new owned descriptor.
		let descriptor = unsafe { OwnedFd::from_raw_fd(raw as i32) };
		(descriptor, ProcessDeathEvidenceKind::LinuxPidfdExit)
	};

	#[cfg(target_os = "macos")]
	let (descriptor, kind) = {
		// SAFETY: `kqueue` returns a new descriptor on success.
		let raw = unsafe { libc::kqueue() };
		if raw == -1 {
			return Err(ProcessPlatformError::Observation(io::Error::last_os_error()));
		}
		// SAFETY: the successful result is a new owned descriptor.
		let descriptor = unsafe { OwnedFd::from_raw_fd(raw) };
		let change = libc::kevent {
			ident: expected.process_id as libc::uintptr_t,
			filter: libc::EVFILT_PROC,
			flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT,
			fflags: libc::NOTE_EXIT,
			data: 0,
			udata: std::ptr::null_mut(),
		};
		// SAFETY: the change list contains one initialized process filter.
		let result = unsafe {
			libc::kevent(
				descriptor.as_raw_fd(),
				&change,
				1,
				std::ptr::null_mut(),
				0,
				std::ptr::null(),
			)
		};
		if result == -1 {
			let error = io::Error::last_os_error();
			if error.raw_os_error() == Some(libc::ESRCH) {
				return Ok(ExactProcessObservation::NotObserved);
			}
			return Err(ProcessPlatformError::Observation(error));
		}
		(descriptor, ProcessDeathEvidenceKind::MacosKqueueExitAndGroupQuiescence)
	};

	#[cfg(not(any(target_os = "linux", target_os = "macos")))]
	return Err(ProcessPlatformError::Unsupported);

	match inspect_process_identity(expected.process_id, &boot)? {
		Some(observed) if observed == *expected =>
			Ok(ExactProcessObservation::Attached(KernelExitWitness {
				descriptor,
				identity: expected.clone(),
				kind,
				positive_exit: AtomicBool::new(false),
			})),
		Some(observed) => Ok(ExactProcessObservation::IdentityMismatch { observed }),
		None => Ok(ExactProcessObservation::NotObserved),
	}
}

/// Configure generic session, file-size, and inherited-descriptor mechanics.
///
/// This function grants no ProcessGeneration lifetime capability. In particular, it does not
/// install Linux `PR_SET_PDEATHSIG`. That primitive requires a future accepted exact Linux
/// lifetime profile before source can add a capability-gated call.
pub(crate) fn configure_session_command(command: &mut Command, max_file_bytes: Option<u64>) {
	let descriptor_limit = unsafe { libc::getdtablesize() }.max(3);

	// SAFETY: this pre-exec closure uses only async-signal-safe syscalls and stack-owned values.
	unsafe {
		command.pre_exec(move || {
			if libc::setsid() == -1 {
				return Err(io::Error::last_os_error());
			}

			if let Some(limit) = max_file_bytes {
				let limit = libc::rlimit { rlim_cur: limit, rlim_max: limit };
				if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) == -1 {
					return Err(io::Error::last_os_error());
				}
			}

			for descriptor in 3..descriptor_limit {
				mark_descriptor_close_on_exec(descriptor)?;
			}
			Ok(())
		});
	}
}

/// Signal a process group only while its exact leader remains an owned, unreaped child.
pub(crate) fn signal_owned_process_group(
	identity: &ProcessIdentity,
	signal: i32,
) -> Result<(), ProcessPlatformError> {
	let boot_id = current_boot_identity().map_err(|error| match error {
		ProcessPlatformError::BootIdentity(error) => ProcessPlatformError::Signal(error),
		_ => ProcessPlatformError::Signal(invalid_identity()),
	})?;
	match inspect_process_identity(identity.process_id, &boot_id).map_err(|error| match error {
		ProcessPlatformError::ProcessIdentity(error) => ProcessPlatformError::Signal(error),
		_ => ProcessPlatformError::Signal(invalid_identity()),
	})? {
		Some(observed) if observed == *identity => {},
		Some(_) | None => return Err(ProcessPlatformError::Signal(invalid_identity())),
	}
	signal_owned_process_group_id(identity.process_group_id, signal)
}

/// Signal a newly spawned session before exact start identity capture while `Child` is unreaped.
pub(crate) fn signal_owned_process_group_id(
	process_group_id: u32,
	signal: i32,
) -> Result<(), ProcessPlatformError> {
	let process_group_id = i32::try_from(process_group_id)
		.map_err(|_| ProcessPlatformError::Signal(invalid_identity()))?;
	// SAFETY: ProcessSupervisor calls this only while it retains the exact unreaped `Child`.
	let result = unsafe { libc::kill(-process_group_id, signal) };
	if result == 0 { Ok(()) } else { Err(ProcessPlatformError::Signal(io::Error::last_os_error())) }
}

/// Check group quiescence only as corroboration after positive exact-leader death.
pub(crate) fn process_group_is_quiescent(
	identity: &ProcessIdentity,
) -> Result<bool, ProcessPlatformError> {
	process_group_id_is_quiescent(identity.process_group_id)
}

/// Check a newly spawned group only as corroboration after an owned-child wait.
pub(crate) fn process_group_id_is_quiescent(
	process_group_id: u32,
) -> Result<bool, ProcessPlatformError> {
	let process_group_id = i32::try_from(process_group_id)
		.map_err(|_| ProcessPlatformError::Observation(invalid_identity()))?;
	// SAFETY: signal zero does not mutate the target.
	let result = unsafe { libc::kill(-process_group_id, 0) };
	if result == 0 {
		return Ok(false);
	}
	match io::Error::last_os_error().raw_os_error() {
		Some(libc::ESRCH) => Ok(true),
		Some(libc::EPERM) => Ok(false),
		_ => Err(ProcessPlatformError::Observation(io::Error::last_os_error())),
	}
}

unsafe fn mark_descriptor_close_on_exec(descriptor: i32) -> io::Result<()> {
	// SAFETY: callers provide an integer descriptor and `fcntl` is async-signal-safe.
	let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFD) };
	if flags == -1 {
		let error = io::Error::last_os_error();
		if error.raw_os_error() != Some(libc::EBADF) {
			return Err(error);
		}
	} else if flags & libc::FD_CLOEXEC == 0
		&& unsafe { libc::fcntl(descriptor, libc::F_SETFD, flags | libc::FD_CLOEXEC) } == -1
	{
		return Err(io::Error::last_os_error());
	}
	Ok(())
}

#[cfg(target_os = "linux")]
fn parse_u32(value: &str) -> Result<u32, ProcessPlatformError> {
	value.parse().map_err(|_| {
		ProcessPlatformError::ProcessIdentity(io::Error::new(
			io::ErrorKind::InvalidData,
			"Linux process identity field is invalid",
		))
	})
}

fn invalid_identity() -> io::Error {
	io::Error::new(io::ErrorKind::InvalidData, "operating-system identity is invalid")
}

#[cfg(test)]
mod tests {
	use std::os::fd::AsRawFd as _;

	use super::mark_descriptor_close_on_exec;

	#[test]
	fn owned_descriptor_without_close_on_exec_gains_close_on_exec() {
		let file = tempfile::tempfile().unwrap();
		let descriptor = file.as_raw_fd();

		// SAFETY: the owned test descriptor remains open for the complete assertion.
		unsafe {
			assert_ne!(libc::fcntl(descriptor, libc::F_SETFD, 0), -1);
			assert_eq!(libc::fcntl(descriptor, libc::F_GETFD) & libc::FD_CLOEXEC, 0);

			mark_descriptor_close_on_exec(descriptor).unwrap();

			let flags = libc::fcntl(descriptor, libc::F_GETFD);
			assert_ne!(flags, -1);
			assert_ne!(flags & libc::FD_CLOEXEC, 0);
		}
	}
}
