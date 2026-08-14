//! Suspended macOS process creation with exact code-identity verification.
//!
//! The caller first captures a statically validated identity from an immutable reference snapshot
//! and proves that a separate canonical execution path has the same identity. A spawn executes the
//! canonical path directly and stops it before user code runs. The caller can then repeat its
//! filesystem checks before this module compares the kernel's dynamic code object with an exact
//! CDHash requirement, the canonical path, and the captured `kSecCodeInfoUnique` value. The child
//! resumes only after all checks succeed.

use std::{
	ffi::{CString, OsStr, OsString, c_char, c_short, c_void},
	fmt::{Debug, Formatter},
	fs::{self, File},
	io::{self, ErrorKind},
	os::{
		fd::{AsRawFd as _, FromRawFd as _},
		unix::{
			ffi::OsStrExt as _,
			fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _},
			process::ExitStatusExt as _,
		},
	},
	path::{Path, PathBuf},
	process::ExitStatus,
	ptr, thread,
	time::Duration,
};

use core_foundation::{
	base::{CFGetTypeID, CFTypeRef, OSStatus, TCFType as _},
	data::{CFDataGetBytePtr, CFDataGetLength, CFDataGetTypeID, CFDataRef},
	dictionary::{CFDictionary, CFDictionaryGetValue, CFDictionaryRef},
	string::CFStringRef,
	url::CFURL,
};
use libc::{
	EINTR, ESRCH, F_GETFD, F_GETFL, F_SETFL, FD_CLOEXEC, O_CLOEXEC, O_NOFOLLOW, O_NONBLOCK,
	O_RDONLY, O_WRONLY, SIGCONT, SIGKILL, WNOHANG, posix_spawn_file_actions_t, posix_spawnattr_t,
};
use security_framework::os::macos::code_signing::{
	Flags, GuestAttributes, SecCode, SecRequirement, SecStaticCode,
};
use tempfile::{Builder as TempDirBuilder, TempDir};

const CHILD_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
pub(super) const PRIVATE_STDIO_STARTUP_ENV: &str =
	"CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED";
pub(super) const PRIVATE_STDIO_STARTUP_VALUE: &str = "1";
const MAX_CODE_IDENTITY_BYTES: usize = 64;
const DYNAMIC_CODE_LOOKUP_ATTEMPTS: usize = 50;
const DYNAMIC_CODE_LOOKUP_DELAY: Duration = Duration::from_millis(10);

// Darwin defines this flag in <sys/spawn.h>, but libc does not currently expose it for Apple
// targets. The other two Darwin flags are exposed as c_int and are converted together below.
const POSIX_SPAWN_SETSID_DARWIN: c_short = 0x0400;

// Static and dynamic validation must not consult the network. Static checking covers every
// architecture so a universal executable cannot pass because only its host-native slice was
// inspected.
const STATIC_CODE_VALIDATION_FLAGS: u32 = Flags::CHECK_ALL_ARCHITECTURES.bits()
	| Flags::STRICT_VALIDATE.bits()
	| Flags::NO_NETWORK_ACCESS.bits();
const DYNAMIC_CODE_VALIDATION_FLAGS: Flags =
	Flags::from_bits_retain(Flags::STRICT_VALIDATE.bits() | Flags::NO_NETWORK_ACCESS.bits());

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
	static kSecCodeInfoUnique: CFStringRef;

	fn SecCodeCopySigningInformation(
		code: *const c_void,
		flags: u32,
		information: *mut CFDictionaryRef,
	) -> OSStatus;
	fn SecStaticCodeCheckValidity(
		code: *const c_void,
		flags: u32,
		requirement: *const c_void,
	) -> OSStatus;
}

unsafe extern "C" {
	fn posix_spawn_file_actions_addchdir_np(
		actions: *mut posix_spawn_file_actions_t,
		path: *const c_char,
	) -> libc::c_int;
	fn posix_spawn_file_actions_addfchdir_np(
		actions: *mut posix_spawn_file_actions_t,
		descriptor: libc::c_int,
	) -> libc::c_int;
}

/// Statically validated identity rooted in a reference snapshot for one execution path.
#[derive(Clone, Eq, PartialEq)]
pub(super) struct AttestedCodeIdentity {
	execution_path: PathBuf,
	unique: [u8; MAX_CODE_IDENTITY_BYTES],
	unique_len: u8,
}

impl AttestedCodeIdentity {
	/// Capture an exact signed identity from a trusted snapshot and bind it to an execution path.
	pub(super) fn capture(reference_snapshot: &Path, execution_path: &Path) -> io::Result<Self> {
		let reference_snapshot = fs::canonicalize(reference_snapshot)?;
		let execution_path = fs::canonicalize(execution_path)?;
		let reference_code = validated_static_code(&reference_snapshot)?;
		let execution_code = validated_static_code(&execution_path)?;
		let (unique, unique_len) = copy_unique_identity(
			reference_code.as_concrete_TypeRef().cast::<c_void>().cast_const(),
		)?;
		let (execution_unique, execution_unique_len) = copy_unique_identity(
			execution_code.as_concrete_TypeRef().cast::<c_void>().cast_const(),
		)?;

		if execution_unique_len != unique_len
			|| execution_unique[..usize::from(execution_unique_len)]
				!= unique[..usize::from(unique_len)]
		{
			return Err(permission_denied(
				"execution path does not match the reference code identity",
			));
		}

		Ok(Self { execution_path, unique, unique_len })
	}

	/// Canonical path that can execute this captured identity.
	pub(super) fn execution_path(&self) -> &Path {
		&self.execution_path
	}

	fn unique(&self) -> &[u8] {
		&self.unique[..usize::from(self.unique_len)]
	}
}

impl Debug for AttestedCodeIdentity {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("AttestedCodeIdentity")
			.field("execution_path", &self.execution_path)
			.finish_non_exhaustive()
	}
}

/// One resumed child plus the parent sides of its protocol pipes.
pub(super) struct AttestedSpawn {
	pub(super) child: AttestedChild,
	pub(super) stdin: File,
	pub(super) stdout: File,
}

impl Debug for AttestedSpawn {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.debug_struct("AttestedSpawn").field("child", &self.child).finish_non_exhaustive()
	}
}

/// One child that cannot run user code until its dynamic identity is attested.
pub(super) struct SuspendedAttestedSpawn {
	execution_path: PathBuf,
	child: Option<AttestedChild>,
	stdin: Option<File>,
	stdout: Option<File>,
}

impl SuspendedAttestedSpawn {
	/// OS process identifier for bounded filesystem re-verification while the child is suspended.
	#[cfg(test)]
	pub(super) fn id(&self) -> u32 {
		self.child.as_ref().expect("a suspended spawn owns its child").id()
	}

	/// Verify the kernel's process identity and process-group contract, then resume the child.
	pub(super) fn attest_and_resume(
		mut self,
		identity: &AttestedCodeIdentity,
	) -> io::Result<AttestedSpawn> {
		if self.execution_path != identity.execution_path {
			return Err(permission_denied("suspended execution path changed before attestation"));
		}

		let pid = self.child.as_ref().expect("a suspended spawn owns its child").pid;

		verify_dynamic_identity(pid, identity)?;
		verify_session_and_process_group(pid)?;

		// SAFETY: the positive pid names the unreaped, suspended child owned by this value.
		if unsafe { libc::kill(pid, SIGCONT) } != 0 {
			return Err(io::Error::last_os_error());
		}

		Ok(AttestedSpawn {
			child: self.child.take().expect("a suspended spawn owns its child"),
			stdin: self.stdin.take().expect("a suspended spawn owns its stdin pipe"),
			stdout: self.stdout.take().expect("a suspended spawn owns its stdout pipe"),
		})
	}
}

impl Debug for SuspendedAttestedSpawn {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("SuspendedAttestedSpawn")
			.field("execution_path", &self.execution_path)
			.field("child", &self.child)
			.finish_non_exhaustive()
	}
}

impl Drop for SuspendedAttestedSpawn {
	fn drop(&mut self) {
		if let Some(child) = self.child.take() {
			kill_and_reap(child.pid);
		}
	}
}

/// Spawn one canonical executable and return it still suspended.
///
/// `args` excludes `argv[0]`; the canonical executable path is always used for `argv[0]` and the
/// executed path. The child receives only `HOME` and the fixed system `PATH` environment entries.
pub(super) fn spawn_suspended(
	identity: &AttestedCodeIdentity,
	args: &[OsString],
	working_directory: &Path,
	home: &Path,
) -> io::Result<SuspendedAttestedSpawn> {
	spawn_suspended_with_environment(
		identity,
		args,
		SuspendedWorkingDirectory::Path(working_directory),
		home,
		SuspendedEnvironment::HomeAndSystemPath,
	)
}

/// Spawn the accepted private-stdio profile from the canonical executable and keep it suspended.
///
/// This is a closed environment profile. It cannot project caller-selected names or values.
pub(super) fn spawn_private_stdio_suspended(
	identity: &AttestedCodeIdentity,
	args: &[OsString],
	working_directory: &Path,
	home: &Path,
) -> io::Result<SuspendedAttestedSpawn> {
	spawn_suspended_with_environment(
		identity,
		args,
		SuspendedWorkingDirectory::Path(working_directory),
		home,
		SuspendedEnvironment::PrivateStdioDisabledEphemeral,
	)
}

/// Spawn the private-stdio profile with cwd bound to one caller-retained directory descriptor.
pub(super) fn spawn_private_stdio_suspended_at(
	identity: &AttestedCodeIdentity,
	args: &[OsString],
	working_directory_descriptor: libc::c_int,
	home: &Path,
) -> io::Result<SuspendedAttestedSpawn> {
	spawn_suspended_with_environment(
		identity,
		args,
		SuspendedWorkingDirectory::Descriptor(working_directory_descriptor),
		home,
		SuspendedEnvironment::PrivateStdioDisabledEphemeral,
	)
}

#[derive(Clone, Copy)]
enum SuspendedEnvironment {
	HomeAndSystemPath,
	PrivateStdioDisabledEphemeral,
}

#[derive(Clone, Copy)]
enum SuspendedWorkingDirectory<'a> {
	Path(&'a Path),
	Descriptor(libc::c_int),
}

fn spawn_suspended_with_environment(
	identity: &AttestedCodeIdentity,
	args: &[OsString],
	working_directory: SuspendedWorkingDirectory<'_>,
	home: &Path,
	environment: SuspendedEnvironment,
) -> io::Result<SuspendedAttestedSpawn> {
	let spawned_execution_path = identity.execution_path.clone();
	let executable = os_string(identity.execution_path.as_os_str())?;
	let working_directory_path = match working_directory {
		SuspendedWorkingDirectory::Path(path) => Some(os_string(path.as_os_str())?),
		SuspendedWorkingDirectory::Descriptor(descriptor) if descriptor >= 0 => None,
		SuspendedWorkingDirectory::Descriptor(_) => {
			return Err(invalid_input("working-directory descriptor is invalid"));
		},
	};
	let mut argv = Vec::with_capacity(args.len() + 1);

	argv.push(os_string(identity.execution_path.as_os_str())?);
	argv.extend(args.iter().map(|arg| os_string(arg)).collect::<io::Result<Vec<_>>>()?);

	let home_environment = environment_entry(b"HOME", home.as_os_str())?;
	let path_environment = CString::new(format!("PATH={CHILD_PATH}"))
		.map_err(|_| invalid_input("child PATH contains a NUL byte"))?;
	let private_stdio_environment = match environment {
		SuspendedEnvironment::HomeAndSystemPath => None,
		SuspendedEnvironment::PrivateStdioDisabledEphemeral => Some(environment_entry(
			PRIVATE_STDIO_STARTUP_ENV.as_bytes(),
			OsStr::new(PRIVATE_STDIO_STARTUP_VALUE),
		)?),
	};
	let mut argv_pointers = argv
		.iter()
		.map(|value| value.as_ptr().cast_mut())
		.chain(std::iter::once(ptr::null_mut()))
		.collect::<Vec<_>>();
	let mut environment_pointers =
		[Some(&home_environment), Some(&path_environment), private_stdio_environment.as_ref()]
			.into_iter()
			.flatten()
			.map(|value| value.as_ptr().cast_mut())
			.chain(std::iter::once(ptr::null_mut()))
			.collect::<Vec<_>>();

	let protocol = ProtocolFifos::new()?;
	let mut actions = SpawnFileActions::new()?;

	actions.open(libc::STDIN_FILENO, protocol.stdin_path(), O_RDONLY | O_NOFOLLOW, 0)?;
	actions.open(libc::STDOUT_FILENO, protocol.stdout_path(), O_WRONLY | O_NOFOLLOW, 0)?;
	actions.open(libc::STDERR_FILENO, c"/dev/null", O_WRONLY, 0)?;
	match (working_directory, working_directory_path.as_ref()) {
		(SuspendedWorkingDirectory::Path(_), Some(path)) => actions.chdir(path)?,
		(SuspendedWorkingDirectory::Descriptor(descriptor), None) => actions.fchdir(descriptor)?,
		_ => return Err(invalid_input("working-directory action is incomplete")),
	}

	let mut attributes = SpawnAttributes::new()?;
	attributes.set_attested_flags()?;

	let mut pid = -1;
	// SAFETY: every pointer targets a live, NUL-terminated allocation for the duration of the call;
	// the action and attribute handles were initialized successfully.
	let result = unsafe {
		libc::posix_spawn(
			&mut pid,
			executable.as_ptr(),
			actions.as_ptr(),
			attributes.as_ptr(),
			argv_pointers.as_mut_ptr(),
			environment_pointers.as_mut_ptr(),
		)
	};

	if result != 0 {
		return Err(io::Error::from_raw_os_error(result));
	}
	if pid <= 0 {
		return Err(io::Error::other("posix_spawn returned an invalid child identifier"));
	}

	let mut suspended = SuspendedAttestedSpawn {
		execution_path: spawned_execution_path,
		child: Some(AttestedChild { pid, status: None }),
		stdin: None,
		stdout: None,
	};
	let (parent_stdin, parent_stdout) = protocol.finish_after_spawn()?;

	suspended.stdin = Some(parent_stdin);
	suspended.stdout = Some(parent_stdout);

	Ok(suspended)
}

/// Minimal owned child handle for a pid returned directly by `posix_spawn`.
pub(super) struct AttestedChild {
	pid: libc::pid_t,
	status: Option<ExitStatus>,
}

impl AttestedChild {
	pub(super) fn id(&self) -> u32 {
		u32::try_from(self.pid).expect("a positive macOS pid fits in u32")
	}

	pub(super) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
		if let Some(status) = self.status {
			return Ok(Some(status));
		}

		self.wait_with_options(WNOHANG)
	}

	pub(super) fn kill(&mut self) -> io::Result<()> {
		if self.status.is_some() {
			return Ok(());
		}

		// SAFETY: this handle retains exclusive wait ownership for its positive child pid.
		if unsafe { libc::kill(self.pid, SIGKILL) } == 0 {
			return Ok(());
		}

		let error = io::Error::last_os_error();
		if error.raw_os_error() == Some(ESRCH) && self.try_wait()?.is_some() {
			Ok(())
		} else {
			Err(error)
		}
	}

	pub(super) fn wait(&mut self) -> io::Result<ExitStatus> {
		if let Some(status) = self.status {
			return Ok(status);
		}

		self.wait_with_options(0)?
			.ok_or_else(|| io::Error::other("blocking wait returned no status"))
	}

	fn wait_with_options(&mut self, options: libc::c_int) -> io::Result<Option<ExitStatus>> {
		loop {
			let mut raw_status = 0;
			// SAFETY: this handle exclusively waits for its own positive child pid and supplies a
			// valid status pointer.
			let waited = unsafe { libc::waitpid(self.pid, &mut raw_status, options) };

			if waited == self.pid {
				let status = ExitStatus::from_raw(raw_status);

				self.status = Some(status);

				return Ok(Some(status));
			}
			if waited == 0 {
				return Ok(None);
			}
			if waited == -1 {
				let error = io::Error::last_os_error();

				if error.raw_os_error() == Some(EINTR) {
					continue;
				}

				return Err(error);
			}

			return Err(io::Error::other("waitpid returned an unexpected child identifier"));
		}
	}
}

impl Debug for AttestedChild {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("AttestedChild")
			.field("pid", &self.pid)
			.field("exited", &self.status.is_some())
			.finish()
	}
}

fn validated_static_code(canonical_path: &Path) -> io::Result<SecStaticCode> {
	let url = CFURL::from_path(canonical_path, false)
		.ok_or_else(|| invalid_input("canonical executable path is unavailable"))?;
	let code = SecStaticCode::from_path(&url, Flags::NONE)
		.map_err(|_| permission_denied("static code identity is unavailable"))?;

	check_static_validity(&code)?;

	let reported_path = code
		.path(Flags::NONE)
		.map_err(|_| invalid_data("static code path is unavailable"))?
		.to_path()
		.ok_or_else(|| invalid_data("static code path is malformed"))?;

	if reported_path != canonical_path {
		return Err(invalid_data("static code path does not match the canonical executable"));
	}

	Ok(code)
}

fn check_static_validity(code: &SecStaticCode) -> io::Result<()> {
	// SAFETY: `code` is a live Security-framework object. A null requirement requests signature
	// validation without an additional caller-defined requirement.
	let status = unsafe {
		SecStaticCodeCheckValidity(
			code.as_concrete_TypeRef().cast::<c_void>(),
			STATIC_CODE_VALIDATION_FLAGS,
			ptr::null(),
		)
	};

	security_status(status, "static code validation failed")
}

fn verify_dynamic_identity(pid: libc::pid_t, expected: &AttestedCodeIdentity) -> io::Result<()> {
	let code = dynamic_code_for_pid(pid)?;
	let requirement = exact_cdhash_requirement(expected.unique())?;

	code.check_validity(DYNAMIC_CODE_VALIDATION_FLAGS, &requirement)
		.map_err(|_| permission_denied("dynamic code validation failed"))?;

	let reported_path = code
		.path(Flags::NONE)
		.map_err(|_| invalid_data("dynamic code path is unavailable"))?
		.to_path()
		.ok_or_else(|| invalid_data("dynamic code path is malformed"))?;

	if reported_path != expected.execution_path {
		return Err(permission_denied("dynamic code path changed during spawn"));
	}

	let (unique, unique_len) =
		copy_unique_identity(code.as_concrete_TypeRef().cast::<c_void>().cast_const())?;

	if &unique[..usize::from(unique_len)] != expected.unique() {
		return Err(permission_denied("dynamic code identity changed during spawn"));
	}

	Ok(())
}

fn exact_cdhash_requirement(unique: &[u8]) -> io::Result<SecRequirement> {
	const HEX: &[u8; 16] = b"0123456789abcdef";

	let mut text = String::with_capacity("cdhash H\"\"".len() + unique.len() * 2);

	text.push_str("cdhash H\"");
	for byte in unique {
		text.push(char::from(HEX[usize::from(byte >> 4)]));
		text.push(char::from(HEX[usize::from(byte & 0x0f)]));
	}
	text.push('"');

	text.parse().map_err(|_| invalid_data("exact CDHash requirement is unavailable"))
}

fn verify_session_and_process_group(pid: libc::pid_t) -> io::Result<()> {
	// SAFETY: `pid` names the live suspended child; getsid only reads kernel process metadata.
	let session = unsafe { libc::getsid(pid) };

	if session == -1 {
		return Err(io::Error::last_os_error());
	}
	if session != pid {
		return Err(permission_denied("suspended child did not create its own session"));
	}

	// SAFETY: `pid` names the live suspended child; getpgid only reads kernel process metadata.
	let process_group = unsafe { libc::getpgid(pid) };

	if process_group == -1 {
		return Err(io::Error::last_os_error());
	}
	if process_group != pid {
		return Err(permission_denied("suspended child did not create its own process group"));
	}

	Ok(())
}

fn dynamic_code_for_pid(pid: libc::pid_t) -> io::Result<SecCode> {
	for attempt in 0..DYNAMIC_CODE_LOOKUP_ATTEMPTS {
		let mut attributes = GuestAttributes::new();

		attributes.set_pid(pid);

		match SecCode::copy_guest_with_attribues(None, &attributes, Flags::NONE) {
			Ok(code) => return Ok(code),
			Err(_) if attempt + 1 < DYNAMIC_CODE_LOOKUP_ATTEMPTS => {
				thread::sleep(DYNAMIC_CODE_LOOKUP_DELAY);
			},
			Err(_) => break,
		}
	}

	Err(io::Error::new(
		ErrorKind::TimedOut,
		"dynamic code identity was not available within the bounded lookup window",
	))
}

fn copy_unique_identity(code: *const c_void) -> io::Result<([u8; MAX_CODE_IDENTITY_BYTES], u8)> {
	let mut raw_information: CFDictionaryRef = ptr::null();
	// SAFETY: `code` is a live SecCodeRef or SecStaticCodeRef. The returned dictionary follows the
	// Create Rule and is immediately wrapped below.
	let status = unsafe { SecCodeCopySigningInformation(code, 0, &mut raw_information) };

	security_status(status, "code signing information is unavailable")?;
	if raw_information.is_null() {
		return Err(invalid_data("code signing information is empty"));
	}

	// SAFETY: a successful SecCodeCopySigningInformation call returns an owned CFDictionaryRef.
	let information = unsafe {
		CFDictionary::<*const c_void, *const c_void>::wrap_under_create_rule(raw_information)
	};
	// SAFETY: the dictionary and global key are live for this lookup.
	let value = unsafe {
		CFDictionaryGetValue(information.as_concrete_TypeRef(), kSecCodeInfoUnique.cast::<c_void>())
	};

	if value.is_null() {
		return Err(invalid_data("signed code has no unique identity"));
	}
	// SAFETY: `value` is a retained dictionary member for the lifetime of `information`.
	if unsafe { CFGetTypeID(value.cast::<c_void>() as CFTypeRef) } != unsafe { CFDataGetTypeID() } {
		return Err(invalid_data("unique code identity has an unexpected type"));
	}

	let data = value.cast::<c_void>() as CFDataRef;
	// SAFETY: the runtime type check above establishes that `data` is a CFDataRef.
	let length = unsafe { CFDataGetLength(data) };
	let length = usize::try_from(length)
		.map_err(|_| invalid_data("unique code identity has a negative length"))?;

	if length == 0 || length > MAX_CODE_IDENTITY_BYTES {
		return Err(invalid_data("unique code identity exceeds its mechanical bound"));
	}

	// SAFETY: CFData promises `length` readable bytes; a positive length requires a non-null byte
	// pointer. The bytes are copied before the dictionary is released.
	let source = unsafe { CFDataGetBytePtr(data) };
	if source.is_null() {
		return Err(invalid_data("unique code identity has no bytes"));
	}

	let mut unique = [0_u8; MAX_CODE_IDENTITY_BYTES];

	// SAFETY: both buffers are valid for `length`, which was checked against the destination bound.
	unsafe { ptr::copy_nonoverlapping(source, unique.as_mut_ptr(), length) };

	Ok((unique, u8::try_from(length).expect("the unique code identity bound fits in u8")))
}

struct ProtocolFifos {
	directory: TempDir,
	stdin_path: CString,
	stdout_path: CString,
	parent_stdin: File,
	parent_stdout: File,
}

impl ProtocolFifos {
	fn new() -> io::Result<Self> {
		let directory = TempDirBuilder::new().prefix("decodex-app-server-fifos-").tempdir()?;

		fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
		validate_private_directory(directory.path())?;

		let stdin_path = os_string(directory.path().join("stdin").as_os_str())?;
		let stdout_path = os_string(directory.path().join("stdout").as_os_str())?;

		create_fifo(&stdin_path)?;
		create_fifo(&stdout_path)?;

		// A temporary nonblocking reader lets the parent open its write endpoint atomically with
		// O_CLOEXEC before the child exists. The retained writer then lets the child's fd 0 open
		// complete during posix_spawn.
		let temporary_stdin_reader =
			open_fifo(&stdin_path, O_RDONLY | O_NONBLOCK | O_CLOEXEC | O_NOFOLLOW)?;
		let parent_stdin = open_fifo(&stdin_path, O_WRONLY | O_CLOEXEC | O_NOFOLLOW)?;

		drop(temporary_stdin_reader);

		// A nonblocking parent reader can open before a writer exists. It lets the child's fd 1
		// open complete; the nonblocking flag is cleared after posix_spawn returns.
		let parent_stdout =
			open_fifo(&stdout_path, O_RDONLY | O_NONBLOCK | O_CLOEXEC | O_NOFOLLOW)?;

		validate_fifo(&stdin_path, &parent_stdin)?;
		validate_fifo(&stdout_path, &parent_stdout)?;

		Ok(Self { directory, stdin_path, stdout_path, parent_stdin, parent_stdout })
	}

	fn stdin_path(&self) -> &std::ffi::CStr {
		&self.stdin_path
	}

	fn stdout_path(&self) -> &std::ffi::CStr {
		&self.stdout_path
	}

	fn finish_after_spawn(self) -> io::Result<(File, File)> {
		set_blocking(&self.parent_stdout)?;
		validate_fifo(&self.stdin_path, &self.parent_stdin)?;
		validate_fifo(&self.stdout_path, &self.parent_stdout)?;
		unlink_fifo(&self.stdin_path)?;
		unlink_fifo(&self.stdout_path)?;

		let Self { directory, stdin_path: _, stdout_path: _, parent_stdin, parent_stdout } = self;

		drop(directory);

		Ok((parent_stdin, parent_stdout))
	}
}

fn validate_private_directory(path: &Path) -> io::Result<()> {
	let metadata = path.symlink_metadata()?;

	if !metadata.is_dir()
		|| metadata.uid() != unsafe { libc::geteuid() }
		|| metadata.permissions().mode() & 0o777 != 0o700
	{
		return Err(permission_denied("protocol FIFO directory is not private"));
	}

	Ok(())
}

fn create_fifo(path: &std::ffi::CStr) -> io::Result<()> {
	// SAFETY: `path` is NUL-terminated and names a not-yet-existing entry in a private directory.
	if unsafe { libc::mkfifo(path.as_ptr(), 0o600) } == 0 {
		Ok(())
	} else {
		Err(io::Error::last_os_error())
	}
}

fn open_fifo(path: &std::ffi::CStr, flags: libc::c_int) -> io::Result<File> {
	// SAFETY: `path` is NUL-terminated. The flags establish an atomic close-on-exec descriptor and
	// O_NOFOLLOW rejects replacement by a symlink.
	let descriptor = unsafe { libc::open(path.as_ptr(), flags) };

	if descriptor == -1 {
		return Err(io::Error::last_os_error());
	}

	// SAFETY: open returned one newly owned descriptor.
	let file = unsafe { File::from_raw_fd(descriptor) };

	if descriptor <= libc::STDERR_FILENO || !descriptor_is_close_on_exec(&file)? {
		return Err(permission_denied("protocol FIFO descriptor boundary is invalid"));
	}

	Ok(file)
}

fn validate_fifo(path: &std::ffi::CStr, file: &File) -> io::Result<()> {
	let path = Path::new(OsStr::from_bytes(path.to_bytes()));
	let path_metadata = path.symlink_metadata()?;
	let file_metadata = file.metadata()?;
	let effective_user = unsafe { libc::geteuid() };
	let valid = path_metadata.file_type().is_fifo()
		&& file_metadata.file_type().is_fifo()
		&& path_metadata.uid() == effective_user
		&& file_metadata.uid() == effective_user
		&& path_metadata.nlink() == 1
		&& file_metadata.nlink() == 1
		&& path_metadata.permissions().mode() & 0o777 == 0o600
		&& file_metadata.permissions().mode() & 0o777 == 0o600
		&& path_metadata.dev() == file_metadata.dev()
		&& path_metadata.ino() == file_metadata.ino()
		&& descriptor_is_close_on_exec(file)?;

	if valid { Ok(()) } else { Err(permission_denied("protocol FIFO identity is invalid")) }
}

fn descriptor_is_close_on_exec(file: &File) -> io::Result<bool> {
	// SAFETY: the File owns a live descriptor and F_GETFD only reads descriptor flags.
	let flags = unsafe { libc::fcntl(file.as_raw_fd(), F_GETFD) };

	if flags == -1 { Err(io::Error::last_os_error()) } else { Ok(flags & FD_CLOEXEC != 0) }
}

fn set_blocking(file: &File) -> io::Result<()> {
	// SAFETY: the File owns a live descriptor and F_GETFL only reads its status flags.
	let flags = unsafe { libc::fcntl(file.as_raw_fd(), F_GETFL) };

	if flags == -1 {
		return Err(io::Error::last_os_error());
	}
	// SAFETY: the File owns a live descriptor and F_SETFL accepts the updated integer flags.
	if unsafe { libc::fcntl(file.as_raw_fd(), F_SETFL, flags & !O_NONBLOCK) } == -1 {
		Err(io::Error::last_os_error())
	} else {
		Ok(())
	}
}

fn unlink_fifo(path: &std::ffi::CStr) -> io::Result<()> {
	// SAFETY: `path` is a live NUL-terminated FIFO name in the private temporary directory.
	if unsafe { libc::unlink(path.as_ptr()) } == 0 {
		Ok(())
	} else {
		Err(io::Error::last_os_error())
	}
}

struct SpawnFileActions(posix_spawn_file_actions_t);

impl SpawnFileActions {
	fn new() -> io::Result<Self> {
		let mut actions = ptr::null_mut();
		// SAFETY: `actions` points to uninitialized storage expected by the initializer.
		let result = unsafe { libc::posix_spawn_file_actions_init(&mut actions) };

		if result == 0 { Ok(Self(actions)) } else { Err(io::Error::from_raw_os_error(result)) }
	}

	fn as_ptr(&self) -> *const posix_spawn_file_actions_t {
		&self.0
	}

	fn open(
		&mut self,
		descriptor: libc::c_int,
		path: &std::ffi::CStr,
		flags: libc::c_int,
		mode: libc::mode_t,
	) -> io::Result<()> {
		// SAFETY: the actions object is initialized and `path` is NUL-terminated.
		let result = unsafe {
			libc::posix_spawn_file_actions_addopen(
				&mut self.0,
				descriptor,
				path.as_ptr(),
				flags,
				mode,
			)
		};

		spawn_configuration_result(result)
	}

	fn chdir(&mut self, path: &CString) -> io::Result<()> {
		// SAFETY: the actions object is initialized and `path` is NUL-terminated.
		let result = unsafe { posix_spawn_file_actions_addchdir_np(&mut self.0, path.as_ptr()) };

		spawn_configuration_result(result)
	}

	fn fchdir(&mut self, descriptor: libc::c_int) -> io::Result<()> {
		// SAFETY: the actions object is initialized and the caller retains the live descriptor
		// through the complete posix_spawn call.
		let result = unsafe { posix_spawn_file_actions_addfchdir_np(&mut self.0, descriptor) };

		spawn_configuration_result(result)
	}
}

impl Drop for SpawnFileActions {
	fn drop(&mut self) {
		// SAFETY: this wrapper is constructed only after successful initialization and destroys the
		// action object exactly once.
		unsafe { libc::posix_spawn_file_actions_destroy(&mut self.0) };
	}
}

struct SpawnAttributes(posix_spawnattr_t);

impl SpawnAttributes {
	fn new() -> io::Result<Self> {
		let mut attributes = ptr::null_mut();
		// SAFETY: `attributes` points to uninitialized storage expected by the initializer.
		let result = unsafe { libc::posix_spawnattr_init(&mut attributes) };

		if result == 0 { Ok(Self(attributes)) } else { Err(io::Error::from_raw_os_error(result)) }
	}

	fn as_ptr(&self) -> *const posix_spawnattr_t {
		&self.0
	}

	fn set_attested_flags(&mut self) -> io::Result<()> {
		let mut default_signals = std::mem::MaybeUninit::<libc::sigset_t>::uninit();
		// SAFETY: sigemptyset initializes the complete output object before it is read.
		if unsafe { libc::sigemptyset(default_signals.as_mut_ptr()) } != 0 {
			return Err(io::Error::last_os_error());
		}
		// SAFETY: the set was initialized above and SIGPIPE is a valid signal number.
		if unsafe { libc::sigaddset(default_signals.as_mut_ptr(), libc::SIGPIPE) } != 0 {
			return Err(io::Error::last_os_error());
		}
		// SAFETY: both the spawn attributes and complete signal set are initialized and live.
		let result =
			unsafe { libc::posix_spawnattr_setsigdefault(&mut self.0, default_signals.as_ptr()) };

		spawn_configuration_result(result)?;

		let exposed = libc::POSIX_SPAWN_START_SUSPENDED
			| libc::POSIX_SPAWN_CLOEXEC_DEFAULT
			| libc::POSIX_SPAWN_SETSIGDEF;
		let flags = c_short::try_from(exposed)
			.map_err(|_| invalid_input("Darwin spawn flags do not fit posix_spawnattr_setflags"))?
			| POSIX_SPAWN_SETSID_DARWIN;
		// SAFETY: the attributes object is initialized and `flags` contains only Darwin-defined
		// posix_spawn bits.
		let result = unsafe { libc::posix_spawnattr_setflags(&mut self.0, flags) };

		spawn_configuration_result(result)
	}
}

impl Drop for SpawnAttributes {
	fn drop(&mut self) {
		// SAFETY: this wrapper is constructed only after successful initialization and destroys the
		// attribute object exactly once.
		unsafe { libc::posix_spawnattr_destroy(&mut self.0) };
	}
}

fn kill_and_reap(pid: libc::pid_t) {
	// SAFETY: the caller owns an unreaped positive child pid. ESRCH only means it has already
	// exited; waitpid still collects its terminal status.
	unsafe { libc::kill(pid, SIGKILL) };

	loop {
		let mut status = 0;
		// SAFETY: `pid` is the caller's child and `status` is a valid output pointer.
		let waited = unsafe { libc::waitpid(pid, &mut status, 0) };

		if waited == pid {
			return;
		}
		if waited == -1 && io::Error::last_os_error().raw_os_error() == Some(EINTR) {
			continue;
		}

		return;
	}
}

fn os_string(value: &OsStr) -> io::Result<CString> {
	CString::new(value.as_bytes()).map_err(|_| invalid_input("spawn value contains a NUL byte"))
}

fn environment_entry(name: &[u8], value: &OsStr) -> io::Result<CString> {
	let mut entry = Vec::with_capacity(name.len() + 1 + value.as_bytes().len());

	entry.extend_from_slice(name);
	entry.push(b'=');
	entry.extend_from_slice(value.as_bytes());

	CString::new(entry).map_err(|_| invalid_input("child environment value contains a NUL byte"))
}

fn security_status(status: OSStatus, message: &'static str) -> io::Result<()> {
	if status == 0 { Ok(()) } else { Err(permission_denied(message)) }
}

fn spawn_configuration_result(result: libc::c_int) -> io::Result<()> {
	if result == 0 { Ok(()) } else { Err(io::Error::from_raw_os_error(result)) }
}

fn invalid_input(message: &'static str) -> io::Error {
	io::Error::new(ErrorKind::InvalidInput, message)
}

fn invalid_data(message: &'static str) -> io::Error {
	io::Error::new(ErrorKind::InvalidData, message)
}

fn permission_denied(message: &'static str) -> io::Error {
	io::Error::new(ErrorKind::PermissionDenied, message)
}

#[cfg(test)]
mod tests {
	use std::{
		collections::BTreeMap,
		io::{Read as _, Write as _},
		process::Command,
	};

	use tempfile::TempDir;

	use super::*;

	fn system_identity(path: &str) -> AttestedCodeIdentity {
		let path = Path::new(path);

		AttestedCodeIdentity::capture(path, path).unwrap()
	}

	#[test]
	fn captures_reference_snapshot_identity_for_a_separate_execution_path() {
		let temporary = TempDir::new().unwrap();
		let snapshot = temporary.path().join("signed-reference-image");

		fs::copy("/bin/cat", &snapshot).unwrap();
		fs::set_permissions(&snapshot, fs::Permissions::from_mode(0o500)).unwrap();

		let identity = AttestedCodeIdentity::capture(&snapshot, Path::new("/bin/cat")).unwrap();

		assert_eq!(identity.execution_path(), fs::canonicalize("/bin/cat").unwrap());
		assert!(!identity.unique().is_empty());
		assert!(identity.unique().len() <= MAX_CODE_IDENTITY_BYTES);
	}

	#[test]
	fn captures_and_runs_the_exact_signed_identity_with_protocol_pipes() {
		let identity = system_identity("/bin/cat");
		let working = TempDir::new().unwrap();
		let suspended = spawn_suspended(&identity, &[], working.path(), working.path()).unwrap();
		let marker = b"attested-spawn-marker\n";

		assert!(suspended.id() > 0);
		assert!(suspended.stdin.as_ref().unwrap().as_raw_fd() > libc::STDERR_FILENO);
		assert!(suspended.stdout.as_ref().unwrap().as_raw_fd() > libc::STDERR_FILENO);

		let mut spawned = suspended.attest_and_resume(&identity).unwrap();

		spawned.stdin.write_all(marker).unwrap();
		drop(spawned.stdin);

		let mut output = Vec::new();

		spawned.stdout.read_to_end(&mut output).unwrap();

		assert_eq!(output, marker);
		assert!(spawned.child.wait().unwrap().success());
		assert!(spawned.child.try_wait().unwrap().unwrap().success());
	}

	#[test]
	fn protocol_fifo_parent_endpoints_are_atomic_cloexec_and_leave_no_names() {
		let protocol = ProtocolFifos::new().unwrap();
		let directory = protocol.directory.path().to_owned();
		let descriptors = [protocol.parent_stdin.as_raw_fd(), protocol.parent_stdout.as_raw_fd()];

		assert!(descriptors.iter().all(|descriptor| *descriptor > libc::STDERR_FILENO));
		assert!(descriptor_is_close_on_exec(&protocol.parent_stdin).unwrap());
		assert!(descriptor_is_close_on_exec(&protocol.parent_stdout).unwrap());

		let probe = Command::new("/usr/bin/python3")
			.arg("-c")
			.arg("import os,sys\ndef is_open(fd):\n    try:\n        os.fstat(fd)\n        return True\n    except OSError:\n        return False\nprint(','.join('open' if is_open(int(fd)) else 'closed' for fd in sys.argv[1:]))")
			.args(descriptors.map(|descriptor| descriptor.to_string()))
			.output()
			.unwrap();

		assert!(probe.status.success());
		assert_eq!(String::from_utf8(probe.stdout).unwrap().trim(), "closed,closed");

		let (parent_stdin, parent_stdout) = protocol.finish_after_spawn().unwrap();

		drop(parent_stdin);
		drop(parent_stdout);

		assert!(!directory.exists());
	}

	#[test]
	fn child_receives_only_home_and_the_fixed_path() {
		let identity = system_identity("/usr/bin/env");
		let working = TempDir::new().unwrap();
		let spawned = spawn_suspended(&identity, &[], working.path(), working.path())
			.unwrap()
			.attest_and_resume(&identity)
			.unwrap();

		drop(spawned.stdin);

		let mut output = String::new();
		let mut stdout = spawned.stdout;
		let mut child = spawned.child;

		stdout.read_to_string(&mut output).unwrap();

		let environment =
			output.lines().map(|line| line.split_once('=').unwrap()).collect::<BTreeMap<_, _>>();

		assert!(child.wait().unwrap().success());
		assert_eq!(environment.len(), 2);
		assert_eq!(environment.get("HOME"), Some(&working.path().to_str().unwrap()));
		assert_eq!(environment.get("PATH"), Some(&CHILD_PATH));
	}

	#[test]
	fn private_stdio_spawn_uses_canonical_path_and_exact_closed_environment() {
		let identity = system_identity("/usr/bin/env");
		let canonical = fs::canonicalize("/usr/bin/env").unwrap();
		let working = TempDir::new().unwrap();
		let suspended =
			spawn_private_stdio_suspended(&identity, &[], working.path(), working.path()).unwrap();

		assert_eq!(suspended.execution_path, canonical);

		let spawned = suspended.attest_and_resume(&identity).unwrap();

		drop(spawned.stdin);

		let mut output = String::new();
		let mut stdout = spawned.stdout;
		let mut child = spawned.child;

		stdout.read_to_string(&mut output).unwrap();

		let environment =
			output.lines().map(|line| line.split_once('=').unwrap()).collect::<BTreeMap<_, _>>();

		assert!(child.wait().unwrap().success());
		assert_eq!(environment.len(), 3);
		assert_eq!(environment.get("HOME"), Some(&working.path().to_str().unwrap()));
		assert_eq!(environment.get("PATH"), Some(&CHILD_PATH));
		assert_eq!(environment.get(PRIVATE_STDIO_STARTUP_ENV), Some(&PRIVATE_STDIO_STARTUP_VALUE));
	}

	#[test]
	fn identity_mismatch_fails_before_the_child_resumes() {
		let identity = system_identity("/bin/cat");
		let mut wrong_identity = identity.clone();
		let working = TempDir::new().unwrap();
		let suspended = spawn_suspended(&identity, &[], working.path(), working.path()).unwrap();

		wrong_identity.unique[0] ^= 0xff;

		let error = suspended.attest_and_resume(&wrong_identity).unwrap_err();

		assert_eq!(error.kind(), ErrorKind::PermissionDenied);
	}

	#[test]
	fn dropping_a_suspended_child_never_runs_user_code() {
		let identity = system_identity("/bin/sh");
		let working = TempDir::new().unwrap();
		let marker = working.path().join("must-not-exist");
		let arguments = [
			OsString::from("-c"),
			OsString::from("printf ran > \"$1\""),
			OsString::from("decodex-suspended-drop-test"),
			marker.as_os_str().to_owned(),
		];
		let suspended =
			spawn_suspended(&identity, &arguments, working.path(), working.path()).unwrap();

		drop(suspended);

		assert!(!marker.exists());
	}

	#[test]
	fn raw_child_handle_kills_and_reaps_once() {
		let identity = system_identity("/bin/sleep");
		let working = TempDir::new().unwrap();
		let mut spawned =
			spawn_suspended(&identity, &[OsString::from("30")], working.path(), working.path())
				.unwrap()
				.attest_and_resume(&identity)
				.unwrap();
		let pid = spawned.child.id();

		drop(spawned.stdin);
		drop(spawned.stdout);
		spawned.child.kill().unwrap();

		let status = spawned.child.wait().unwrap();

		assert!(!status.success());
		assert_eq!(spawned.child.id(), pid);
		assert_eq!(spawned.child.try_wait().unwrap(), Some(status));
		assert!(spawned.child.kill().is_ok());
	}

	#[test]
	fn child_restores_sigpipe_to_the_default_disposition() {
		let identity = system_identity("/bin/cat");
		let working = TempDir::new().unwrap();
		let mut spawned = spawn_suspended(&identity, &[], working.path(), working.path())
			.unwrap()
			.attest_and_resume(&identity)
			.unwrap();
		let pid = libc::pid_t::try_from(spawned.child.id()).unwrap();

		// SAFETY: the positive pid belongs to the live child retained above.
		assert_eq!(unsafe { libc::kill(pid, libc::SIGPIPE) }, 0);

		let status = spawned.child.wait().unwrap();

		assert_eq!(status.signal(), Some(libc::SIGPIPE));
	}
}
