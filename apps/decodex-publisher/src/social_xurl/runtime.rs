use std::{
	ffi::{CStr, OsString},
	fs::{self, File, OpenOptions},
	io::{Read as _, Write as _},
	mem::MaybeUninit,
	ops::Deref,
	os::{
		fd::{AsRawFd as _, FromRawFd as _},
		unix::{
			ffi::{OsStrExt as _, OsStringExt as _},
			fs::OpenOptionsExt as _,
			process::CommandExt,
		},
	},
	path::{Path, PathBuf},
	process::{Command, Output, Stdio},
	ptr,
	sync::mpsc,
	thread,
	time::{Duration, Instant},
};

#[cfg(unix)] use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

use rustix::{
	fs::{self as unix_fs, AtFlags, Dir, Mode, OFlags},
	io::Errno,
};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use wait_timeout::ChildExt as _;

use super::{
	auth_contract::{APPROVED_XURL_SHA256, APPROVED_XURL_VERSION, VerifiedAuthorizationContract},
	model::{TARGET_ACCOUNT, VerifiedIdentity, XURL_APP},
};
use crate::prelude::{Result, eyre};

const XURL_HOME_RELATIVE_ENTRYPOINT: &str = ".local/bin/xurl";
const PRIVATE_RUNTIME_DIR: &str = ".agent/automations/decodex/cache/social/x/xurl-runtime";
const MAX_XURL_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_XURL_BINARY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_XURL_RUNTIME_ENTRIES: usize = 64;
const XURL_DEADLINE: Duration = Duration::from_secs(45);

#[derive(Debug)]
pub(super) struct TrustedXurlBinary {
	file: File,
	directory: File,
	home: PathBuf,
	digest: String,
	deadline: Instant,
}

pub(super) struct AuthenticatedOutput {
	output: Output,
}

impl Deref for AuthenticatedOutput {
	type Target = Output;

	fn deref(&self) -> &Self::Target {
		&self.output
	}
}

impl TrustedXurlBinary {
	#[cfg(test)]
	pub(super) fn open_for_test(path: &Path) -> Result<Self> {
		let reader = OpenOptions::new()
			.read(true)
			.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
			.open(path)?;
		let metadata = reader.metadata()?;
		validate_executable_metadata(&metadata, false)?;
		let file = open_executable_path(path)?;
		require_same_executable(&metadata, &file.metadata()?)?;
		let parent = path.parent().ok_or_else(|| eyre::eyre!("test executable has no parent"))?;
		let directory = File::open(parent)?;
		validate_execution_directory(&directory.metadata()?)?;
		lock_execution_directory(&directory)?;
		let home = parent
			.ancestors()
			.find(|candidate| candidate.join("xurl-authorization-contract.json").is_file())
			.unwrap_or(parent)
			.to_path_buf();

		Ok(Self {
			file,
			directory,
			home,
			digest: APPROVED_XURL_SHA256.into(),
			deadline: Instant::now() + XURL_DEADLINE,
		})
	}

	pub(super) fn require_approved_release(&self) -> Result<()> {
		if self.digest != APPROVED_XURL_SHA256 {
			return Err(eyre::eyre!(
				"xurl executable does not match the approved official 1.3.1 release digest"
			));
		}
		Ok(())
	}

	pub(super) fn require_command_time_remaining(&self) -> Result<()> {
		require_time_remaining(self.deadline)
	}
}

pub(super) fn trusted_xurl_binary() -> Result<TrustedXurlBinary> {
	let deadline = Instant::now() + XURL_DEADLINE;
	let home = trusted_home_directory()?;
	require_time_remaining(deadline)?;
	let entrypoint = resolve_trusted_xurl_entrypoint(&home)?;
	require_time_remaining(deadline)?;
	let (bytes, digest) = read_verified_binary(&entrypoint)?;
	require_time_remaining(deadline)?;
	install_private_copy(&bytes, &digest, &home, deadline)
}

fn resolve_trusted_xurl_entrypoint(home: &Path) -> Result<PathBuf> {
	if !home.is_absolute() {
		return Err(eyre::eyre!("operating-system home directory is not absolute"));
	}
	let entrypoint = home.join(XURL_HOME_RELATIVE_ENTRYPOINT);
	validate_path_chain(&entrypoint)?;

	Ok(entrypoint)
}

fn validate_path_chain(path: &Path) -> Result<()> {
	let current_uid = current_uid();
	let components = path.ancestors().collect::<Vec<_>>();
	for (index, component) in components.iter().rev().enumerate() {
		if component.as_os_str().is_empty() {
			continue;
		}
		let metadata = fs::symlink_metadata(component)
			.map_err(|_| eyre::eyre!("xurl path component is unavailable"))?;
		let is_final = index + 1 == components.len();
		if metadata.file_type().is_symlink() {
			return Err(eyre::eyre!("xurl path contains an unexpected symlink"));
		} else if is_final {
			if !metadata.is_file() {
				return Err(eyre::eyre!("resolved xurl target is not a regular file"));
			}
		} else if !metadata.is_dir() {
			return Err(eyre::eyre!("xurl parent path is not a directory"));
		}
		validate_owner_mode(&metadata, current_uid, is_final)?;
	}

	Ok(())
}

fn validate_owner_mode(metadata: &fs::Metadata, current_uid: u32, executable: bool) -> Result<()> {
	if !matches!(metadata.uid(), 0) && metadata.uid() != current_uid {
		return Err(eyre::eyre!("xurl path owner is not trusted"));
	}
	let mode = metadata.permissions().mode();
	if metadata.uid() == 0 && mode & 0o022 != 0
		|| metadata.uid() == current_uid && mode & 0o022 != 0
		|| executable && (mode & 0o022 != 0 || mode & 0o111 == 0)
	{
		return Err(eyre::eyre!("xurl path permissions are not trusted"));
	}

	Ok(())
}

fn read_verified_binary(path: &Path) -> Result<(Vec<u8>, String)> {
	let mut file = OpenOptions::new()
		.read(true)
		.custom_flags(libc::O_NOFOLLOW)
		.open(path)
		.map_err(|_| eyre::eyre!("resolved xurl target cannot be opened safely"))?;
	let before = file.metadata()?;
	if !before.is_file() || before.len() == 0 || before.len() > MAX_XURL_BINARY_BYTES {
		return Err(eyre::eyre!("resolved xurl target size is invalid"));
	}
	let path_metadata = fs::symlink_metadata(path)?;
	if before.dev() != path_metadata.dev() || before.ino() != path_metadata.ino() {
		return Err(eyre::eyre!("resolved xurl target changed during open"));
	}
	let mut bytes = Vec::with_capacity(before.len() as usize);
	(&mut file).take(MAX_XURL_BINARY_BYTES + 1).read_to_end(&mut bytes)?;
	let after = file.metadata()?;
	if bytes.len() as u64 != before.len()
		|| before.dev() != after.dev()
		|| before.ino() != after.ino()
		|| before.len() != after.len()
		|| before.modified()? != after.modified()?
	{
		return Err(eyre::eyre!("resolved xurl target changed during copy"));
	}
	let digest = sha256(&bytes);
	if digest != APPROVED_XURL_SHA256 {
		return Err(eyre::eyre!(
			"xurl executable does not match the approved official 1.3.1 release digest"
		));
	}

	Ok((bytes, digest))
}

fn install_private_copy(
	bytes: &[u8],
	digest: &str,
	home: &Path,
	deadline: Instant,
) -> Result<TrustedXurlBinary> {
	let repo_root = crate::repo_root()?;
	let runtime_dir = repo_root.join(PRIVATE_RUNTIME_DIR);
	let runtime = crate::filesystem::open_private_directory_descriptor(&runtime_dir, true)?;
	require_time_remaining(deadline)?;
	install_private_copy_in(&runtime, bytes, digest, home, deadline)
}

fn install_private_copy_in(
	runtime: &File,
	bytes: &[u8],
	digest: &str,
	home: &Path,
	deadline: Instant,
) -> Result<TrustedXurlBinary> {
	lock_execution_directory(runtime)?;
	runtime.set_permissions(fs::Permissions::from_mode(0o700))?;
	validate_runtime_directory(runtime)?;
	require_time_remaining(deadline)?;
	let destination = OsString::from(format!("xurl-{digest}"));
	let mut executable = match open_runtime_file(runtime, &destination) {
		Ok(file) => file,
		Err(error) if error.downcast_ref::<Errno>() == Some(&Errno::NOENT) => {
			create_private_copy(runtime, &destination, bytes)?;
			open_runtime_file(runtime, &destination)?
		},
		Err(error) => return Err(error),
	};
	validate_private_copy(&mut executable, bytes, digest)?;
	let metadata = executable.metadata()?;
	prune_runtime_copies(runtime, &destination, &metadata)?;
	require_time_remaining(deadline)?;
	let file = open_runtime_executable(runtime, &destination)?;
	require_same_executable(&metadata, &file.metadata()?)?;

	Ok(TrustedXurlBinary {
		file,
		directory: runtime.try_clone()?,
		home: home.to_path_buf(),
		digest: digest.into(),
		deadline,
	})
}

fn create_private_copy(runtime: &File, destination: &OsString, bytes: &[u8]) -> Result<()> {
	let stage = OsString::from(format!(".stage-{}", random_suffix()?));
	let fd = unix_fs::openat(
		runtime,
		&stage,
		OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
		Mode::from_bits_retain(0o500),
	)?;
	let mut file = File::from(fd);
	file.set_permissions(fs::Permissions::from_mode(0o500))?;
	file.write_all(bytes)?;
	file.sync_all()?;
	validate_runtime_file_metadata(&file.metadata()?)?;
	drop(file);

	let linked = unix_fs::linkat(runtime, &stage, runtime, destination, AtFlags::empty());
	let cleanup = unix_fs::unlinkat(runtime, &stage, AtFlags::empty());
	if let Err(error) = linked {
		if cleanup.is_err() {
			return Err(eyre::eyre!("failed to install and clean the private xurl copy"));
		}
		if error != Errno::EXIST {
			return Err(eyre::eyre!("failed to install the private xurl copy: {error}"));
		}
	} else {
		cleanup?;
	}
	runtime.sync_all()?;

	Ok(())
}

fn open_runtime_file(runtime: &File, name: &OsString) -> Result<File> {
	let fd = unix_fs::openat(
		runtime,
		name,
		OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
		Mode::empty(),
	)?;
	let file = File::from(fd);
	validate_runtime_file_metadata(&file.metadata()?)?;

	Ok(file)
}

fn open_runtime_executable(runtime: &File, name: &OsString) -> Result<File> {
	let name = std::ffi::CString::new(name.as_bytes())
		.map_err(|_| eyre::eyre!("private xurl runtime filename is invalid"))?;
	let fd = unsafe {
		libc::openat(
			runtime.as_raw_fd(),
			name.as_ptr(),
			libc::O_EXEC | libc::O_CLOEXEC | libc::O_NOFOLLOW,
		)
	};
	if fd == -1 {
		return Err(std::io::Error::last_os_error().into());
	}
	let file = unsafe { File::from_raw_fd(fd) };
	validate_executable_metadata(&file.metadata()?, true)?;

	Ok(file)
}

fn open_executable_path(path: &Path) -> Result<File> {
	let path = std::ffi::CString::new(path.as_os_str().as_bytes())
		.map_err(|_| eyre::eyre!("xurl executable path is invalid"))?;
	let fd =
		unsafe { libc::open(path.as_ptr(), libc::O_EXEC | libc::O_CLOEXEC | libc::O_NOFOLLOW) };
	if fd == -1 {
		return Err(std::io::Error::last_os_error().into());
	}
	let file = unsafe { File::from_raw_fd(fd) };
	validate_executable_metadata(&file.metadata()?, false)?;

	Ok(file)
}

fn validate_runtime_directory(runtime: &File) -> Result<()> {
	let metadata = runtime.metadata()?;
	if !metadata.is_dir()
		|| metadata.uid() != current_uid()
		|| metadata.permissions().mode() & 0o777 != 0o700
	{
		return Err(eyre::eyre!("private xurl runtime directory is not trusted"));
	}

	Ok(())
}

fn validate_execution_directory(metadata: &fs::Metadata) -> Result<()> {
	if !metadata.is_dir()
		|| metadata.uid() != current_uid()
		|| metadata.permissions().mode() & 0o022 != 0
	{
		return Err(eyre::eyre!("xurl execution directory is not trusted"));
	}

	Ok(())
}

fn validate_runtime_file_metadata(metadata: &fs::Metadata) -> Result<()> {
	if !metadata.is_file()
		|| metadata.len() == 0
		|| metadata.len() > MAX_XURL_BINARY_BYTES
		|| metadata.uid() != current_uid()
		|| metadata.permissions().mode() & 0o777 != 0o500
		|| metadata.nlink() != 1
	{
		return Err(eyre::eyre!("private xurl runtime copy is not trusted"));
	}

	Ok(())
}

fn validate_executable_metadata(metadata: &fs::Metadata, private_copy: bool) -> Result<()> {
	if !metadata.is_file()
		|| metadata.len() == 0
		|| metadata.len() > MAX_XURL_BINARY_BYTES
		|| metadata.uid() != current_uid()
		|| metadata.permissions().mode() & 0o111 == 0
		|| metadata.permissions().mode() & 0o022 != 0
		|| private_copy && metadata.permissions().mode() & 0o777 != 0o500
	{
		return Err(eyre::eyre!("xurl executable descriptor is not trusted"));
	}

	Ok(())
}

fn require_same_executable(readable: &fs::Metadata, executable: &fs::Metadata) -> Result<()> {
	if readable.dev() != executable.dev()
		|| readable.ino() != executable.ino()
		|| readable.len() != executable.len()
		|| readable.modified()? != executable.modified()?
	{
		return Err(eyre::eyre!("xurl executable changed while binding its descriptor"));
	}

	Ok(())
}

fn descriptor_execution_path(binary: &TrustedXurlBinary) -> Result<(PathBuf, File)> {
	let mut buffer = [0_i8; libc::PATH_MAX as usize];
	let result =
		unsafe { libc::fcntl(binary.file.as_raw_fd(), libc::F_GETPATH, buffer.as_mut_ptr()) };
	if result == -1 {
		return Err(eyre::eyre!(
			"trusted xurl descriptor no longer has an executable filesystem path"
		));
	}
	let bytes = unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_bytes();
	if bytes.is_empty() || bytes.contains(&0) {
		return Err(eyre::eyre!("trusted xurl descriptor path is invalid"));
	}
	let path = PathBuf::from(OsString::from_vec(bytes.to_vec()));
	let descriptor_metadata = binary.file.metadata()?;
	let path_metadata = fs::symlink_metadata(&path)?;
	if path_metadata.file_type().is_symlink() {
		return Err(eyre::eyre!("trusted xurl descriptor path became a symlink"));
	}
	require_same_executable(&descriptor_metadata, &path_metadata)?;
	let rebound = open_executable_path(&path)?;
	require_same_executable(&descriptor_metadata, &rebound.metadata()?)?;

	Ok((path, rebound))
}

fn validate_private_copy(file: &mut File, expected: &[u8], digest: &str) -> Result<()> {
	let before = file.metadata()?;
	validate_runtime_file_metadata(&before)?;
	let mut bytes = Vec::with_capacity(expected.len());
	file.take(MAX_XURL_BINARY_BYTES + 1).read_to_end(&mut bytes)?;
	let after = file.metadata()?;
	if before.dev() != after.dev()
		|| before.ino() != after.ino()
		|| before.len() != after.len()
		|| before.modified()? != after.modified()?
		|| bytes != expected
		|| sha256(&bytes) != digest
	{
		return Err(eyre::eyre!("private xurl runtime copy digest does not match"));
	}

	Ok(())
}

fn prune_runtime_copies(
	runtime: &File,
	current_name: &OsString,
	current_metadata: &fs::Metadata,
) -> Result<()> {
	let mut names = Vec::new();
	for entry in Dir::read_from(runtime)? {
		let entry = entry?;
		let name = OsString::from_vec(entry.file_name().to_bytes().to_vec());
		if name != "." && name != ".." {
			names.push(name);
		}
		if names.len() > MAX_XURL_RUNTIME_ENTRIES {
			return Err(eyre::eyre!(
				"private xurl runtime exceeds {MAX_XURL_RUNTIME_ENTRIES} entries"
			));
		}
	}
	names.sort();

	for name in names {
		if !is_runtime_copy_name(&name) && !is_runtime_stage_name(&name) {
			return Err(eyre::eyre!("private xurl runtime contains an unexpected entry"));
		}
		if is_runtime_stage_name(&name) {
			let file = open_runtime_gc_entry(runtime, &name)?;
			validate_runtime_gc_metadata(&file.metadata()?)?;
			unix_fs::unlinkat(runtime, &name, AtFlags::empty())?;
			continue;
		}
		let mut file = open_runtime_file(runtime, &name)?;
		if name == *current_name {
			let metadata = file.metadata()?;
			if metadata.dev() != current_metadata.dev()
				|| metadata.ino() != current_metadata.ino()
				|| metadata.len() != current_metadata.len()
			{
				return Err(eyre::eyre!("private xurl runtime current copy changed during GC"));
			}
			continue;
		}
		if let Some(expected_digest) = name.to_str().and_then(|value| value.strip_prefix("xurl-")) {
			let before = file.metadata()?;
			let mut bytes = Vec::with_capacity(usize::try_from(before.len()).unwrap_or(0));
			(&mut file).take(MAX_XURL_BINARY_BYTES + 1).read_to_end(&mut bytes)?;
			let after = file.metadata()?;
			if before.dev() != after.dev()
				|| before.ino() != after.ino()
				|| before.len() != after.len()
				|| before.modified()? != after.modified()?
				|| sha256(&bytes) != expected_digest
			{
				return Err(eyre::eyre!("stale private xurl runtime copy is invalid"));
			}
		}
		unix_fs::unlinkat(runtime, &name, AtFlags::empty())?;
	}
	runtime.sync_all()?;
	require_only_current_copy(runtime, current_name, current_metadata)
}

fn open_runtime_gc_entry(runtime: &File, name: &OsString) -> Result<File> {
	let fd = unix_fs::openat(
		runtime,
		name,
		OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
		Mode::empty(),
	)?;

	Ok(File::from(fd))
}

fn validate_runtime_gc_metadata(metadata: &fs::Metadata) -> Result<()> {
	if !metadata.is_file()
		|| metadata.len() > MAX_XURL_BINARY_BYTES
		|| metadata.uid() != current_uid()
		|| metadata.permissions().mode() & 0o777 != 0o500
		|| metadata.nlink() != 1
	{
		return Err(eyre::eyre!("private xurl runtime GC entry is not trusted"));
	}

	Ok(())
}

fn lock_execution_directory(directory: &File) -> Result<()> {
	if unsafe { libc::flock(directory.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == -1 {
		return Err(eyre::eyre!(
			"another trusted xurl runtime operation is active: {}",
			std::io::Error::last_os_error()
		));
	}

	Ok(())
}

fn require_only_current_copy(
	runtime: &File,
	current_name: &OsString,
	current_metadata: &fs::Metadata,
) -> Result<()> {
	let names = Dir::read_from(runtime)?
		.map(|entry| entry.map(|entry| OsString::from_vec(entry.file_name().to_bytes().to_vec())))
		.collect::<std::result::Result<Vec<_>, _>>()?;
	let retained = names.into_iter().filter(|name| name != "." && name != "..").collect::<Vec<_>>();
	if retained.len() != 1 || retained[0] != *current_name {
		return Err(eyre::eyre!("private xurl runtime GC did not retain exactly the current copy"));
	}
	let file = open_runtime_file(runtime, current_name)?;
	let metadata = file.metadata()?;
	if metadata.dev() != current_metadata.dev()
		|| metadata.ino() != current_metadata.ino()
		|| metadata.len() != current_metadata.len()
	{
		return Err(eyre::eyre!("private xurl runtime current copy changed after GC"));
	}

	Ok(())
}

fn is_runtime_copy_name(name: &OsString) -> bool {
	name.to_str().and_then(|value| value.strip_prefix("xurl-")).is_some_and(|digest| {
		digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
	})
}

fn is_runtime_stage_name(name: &OsString) -> bool {
	name.to_str().and_then(|value| value.strip_prefix(".stage-")).is_some_and(|suffix| {
		suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
	})
}

fn random_suffix() -> Result<String> {
	let mut bytes = [0_u8; 16];
	getrandom::fill(&mut bytes).map_err(|_| eyre::eyre!("xurl runtime nonce failed"))?;
	Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn current_uid() -> u32 {
	unsafe { libc::geteuid() }
}

pub(super) fn verify_runtime(binary: &TrustedXurlBinary) -> Result<String> {
	binary.require_approved_release()?;
	let output = run(binary, ["--version"])?;
	if !output.status.success() {
		return Err(failure("version probe", &output));
	}
	let stdout = output_text(&output.stdout, "xurl version output")?;
	let version = stdout
		.split_whitespace()
		.last()
		.ok_or_else(|| eyre::eyre!("xurl version output is empty"))?;
	if version != APPROVED_XURL_VERSION {
		return Err(eyre::eyre!(
			"xurl {version} is unsupported; require the approved official {APPROVED_XURL_VERSION} release"
		));
	}

	Ok(version.into())
}

pub(super) fn verify_auth_status(binary: &TrustedXurlBinary) -> Result<()> {
	let output = run(binary, ["--app", XURL_APP, "auth", "status"])?;
	if !output.status.success() {
		return Err(failure("authentication probe", &output));
	}
	let stdout = output_text(&output.stdout, "xurl authentication output")?;
	validate_auth_status_output(stdout)
}

pub(super) fn verify_ready(
	binary: &TrustedXurlBinary,
	contract: &VerifiedAuthorizationContract,
) -> Result<String> {
	contract.require_runtime(binary)?;
	let version = verify_runtime(binary)?;
	verify_auth_status(binary)?;
	contract.require_runtime(binary)?;

	Ok(version)
}

fn validate_auth_status_output(stdout: &str) -> Result<()> {
	let clean = strip_ansi(stdout);
	let mut default_sections = 0_usize;
	let mut in_default_section = false;
	let mut target_tokens = 0_usize;
	for line in clean.lines() {
		if let Some(app) = auth_app_header(line) {
			in_default_section = app == XURL_APP;
			if in_default_section {
				default_sections += 1;
			}
			continue;
		}
		if !in_default_section {
			continue;
		}
		let trimmed = line.trim();
		let normalized = trimmed.strip_prefix('▸').map(str::trim).unwrap_or(trimmed);
		if normalized.strip_prefix("oauth2:").map(str::trim) == Some(TARGET_ACCOUNT) {
			target_tokens += 1;
		}
	}
	if default_sections != 1 || target_tokens != 1 {
		return Err(eyre::eyre!(
			"xurl app {XURL_APP} does not have exactly one OAuth2 token labeled {TARGET_ACCOUNT}"
		));
	}

	Ok(())
}

fn auth_app_header(line: &str) -> Option<&str> {
	let content = if let Some(content) = line.strip_prefix("▸ ") {
		content
	} else {
		let content = line.strip_prefix("  ")?;
		if content.chars().next().is_some_and(char::is_whitespace) || content.starts_with('▸') {
			return None;
		}
		content
	};
	let (app, detail) = content.split_once("  [")?;
	if app.is_empty()
		|| app.chars().any(char::is_whitespace)
		|| !detail.ends_with(']')
		|| detail.contains('\n')
	{
		return None;
	}
	Some(app)
}

pub(super) fn whoami(
	binary: &TrustedXurlBinary,
	contract: &VerifiedAuthorizationContract,
) -> Result<AuthenticatedOutput> {
	authenticated_run(
		binary,
		contract,
		["--app", XURL_APP, "/2/users/me", "--auth", "oauth2", "--username", TARGET_ACCOUNT],
	)
}

pub(super) fn create(
	binary: &TrustedXurlBinary,
	contract: &VerifiedAuthorizationContract,
	text: &str,
) -> Result<AuthenticatedOutput> {
	authenticated_run(
		binary,
		contract,
		["--app", XURL_APP, "post", text, "--auth", "oauth2", "--username", TARGET_ACCOUNT],
	)
}

pub(super) fn read(
	binary: &TrustedXurlBinary,
	contract: &VerifiedAuthorizationContract,
	post_id: &str,
	_operation: &str,
) -> Result<AuthenticatedOutput> {
	authenticated_run(
		binary,
		contract,
		["--app", XURL_APP, "read", post_id, "--auth", "oauth2", "--username", TARGET_ACCOUNT],
	)
}

pub(super) fn parse_identity(
	output: &mut AuthenticatedOutput,
	_contract: &VerifiedAuthorizationContract,
) -> Result<VerifiedIdentity> {
	parse_identity_output(&output.output)
}

fn parse_identity_output(output: &Output) -> Result<VerifiedIdentity> {
	if !output.status.success() {
		return Err(failure("identity read", output));
	}
	let response = parse_json_output(&output.stdout, "xurl identity response")?;
	let data = response
		.get("data")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("xurl identity response is missing data"))?;
	if data.get("username").and_then(Value::as_str) != Some(TARGET_ACCOUNT) {
		return Err(eyre::eyre!("xurl identity read did not verify @{TARGET_ACCOUNT}"));
	}
	let user_id = numeric_id(data.get("id"), "xurl identity response user id")?.to_owned();

	Ok(VerifiedIdentity { user_id, response_sha256: sha256(&output.stdout) })
}

pub(super) fn parse_create(
	output: &mut AuthenticatedOutput,
	_contract: &VerifiedAuthorizationContract,
	text: &str,
) -> Result<(String, String)> {
	parse_create_output(&output.output, text)
}

fn parse_create_output(output: &Output, text: &str) -> Result<(String, String)> {
	if !output.status.success() {
		return Err(failure("post creation", output));
	}
	let response = parse_json_output(&output.stdout, "xurl post response")?;
	let data = response
		.get("data")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("xurl post response is missing data"))?;
	let post_id = numeric_id(data.get("id"), "xurl post response id")?.to_owned();
	if data.get("text").and_then(Value::as_str) != Some(text) {
		return Err(eyre::eyre!("xurl post response text does not match the candidate"));
	}

	Ok((post_id, sha256(&output.stdout)))
}

pub(super) fn parse_read(
	output: &mut AuthenticatedOutput,
	_contract: &VerifiedAuthorizationContract,
	post_id: &str,
	text: &str,
	verified_user_id: &str,
) -> Result<(Value, String)> {
	parse_read_output(&output.output, post_id, text, verified_user_id)
}

fn parse_read_output(
	output: &Output,
	post_id: &str,
	text: &str,
	verified_user_id: &str,
) -> Result<(Value, String)> {
	if !output.status.success() {
		return Err(failure("post readback", output));
	}
	let response = parse_json_output(&output.stdout, "xurl read response")?;
	verify_read_response(&response, post_id, text, verified_user_id)?;
	let digest = sha256(&output.stdout);

	Ok((response, digest))
}

fn authenticated_run<I, S>(
	binary: &TrustedXurlBinary,
	contract: &VerifiedAuthorizationContract,
	arguments: I,
) -> Result<AuthenticatedOutput>
where
	I: IntoIterator<Item = S>,
	S: AsRef<std::ffi::OsStr>,
{
	contract.require_runtime(binary)?;
	let output = run(binary, arguments)?;
	contract.require_runtime(binary)?;
	Ok(AuthenticatedOutput { output })
}

fn verify_read_response(
	response: &Value,
	post_id: &str,
	text: &str,
	verified_user_id: &str,
) -> Result<()> {
	let data = response
		.get("data")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("xurl read response is missing data"))?;
	if data.get("id").and_then(Value::as_str) != Some(post_id)
		|| data.get("text").and_then(Value::as_str) != Some(text)
		|| data.get("author_id").and_then(Value::as_str) != Some(verified_user_id)
	{
		return Err(eyre::eyre!("xurl readback does not match the created post and identity"));
	}
	let authors = response
		.get("includes")
		.and_then(Value::as_object)
		.and_then(|includes| includes.get("users"))
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("xurl readback is missing author expansion"))?;
	let matches = authors
		.iter()
		.filter(|author| {
			author.get("id").and_then(Value::as_str) == Some(verified_user_id)
				&& author.get("username").and_then(Value::as_str) == Some(TARGET_ACCOUNT)
		})
		.count();
	if matches != 1 {
		return Err(eyre::eyre!(
			"xurl readback did not verify exactly one @{TARGET_ACCOUNT} author"
		));
	}

	Ok(())
}

fn numeric_id<'a>(value: Option<&'a Value>, label: &str) -> Result<&'a str> {
	value
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
		.ok_or_else(|| eyre::eyre!("{label} is invalid"))
}

fn run<I, S>(binary: &TrustedXurlBinary, arguments: I) -> Result<Output>
where
	I: IntoIterator<Item = S>,
	S: AsRef<std::ffi::OsStr>,
{
	run_with_deadline(binary, arguments, XURL_DEADLINE)
}

fn run_with_deadline<I, S>(
	binary: &TrustedXurlBinary,
	arguments: I,
	deadline: Duration,
) -> Result<Output>
where
	I: IntoIterator<Item = S>,
	S: AsRef<std::ffi::OsStr>,
{
	run_with_deadline_inner(binary, arguments, deadline, || {})
}

fn run_with_deadline_inner<I, S, F>(
	binary: &TrustedXurlBinary,
	arguments: I,
	deadline: Duration,
	before_spawn: F,
) -> Result<Output>
where
	I: IntoIterator<Item = S>,
	S: AsRef<std::ffi::OsStr>,
	F: FnOnce(),
{
	let started = Instant::now();
	let local_deadline = started
		.checked_add(deadline)
		.ok_or_else(|| eyre::eyre!("xurl execution deadline overflowed"))?;
	let deadline = binary.deadline.min(local_deadline);
	validate_home_directory(&binary.home)?;
	require_time_remaining(deadline)?;
	validate_execution_directory(&binary.directory.metadata()?)?;
	let (execution_path, rebound) = descriptor_execution_path(binary)?;
	require_time_remaining(deadline)?;
	let mut command = Command::new(execution_path);
	command
		.args(arguments)
		.env_clear()
		.env("HOME", &binary.home)
		.env("PATH", "/usr/bin:/bin")
		.process_group(0)
		.stdout(Stdio::piped())
		.stderr(Stdio::piped());
	before_spawn();
	require_time_remaining(deadline)?;
	let mut child = command
		.spawn()
		.map_err(|error| eyre::eyre!("failed to execute the trusted xurl binary: {error}"))?;
	drop(rebound);
	let stdout = child.stdout.take().ok_or_else(|| eyre::eyre!("xurl stdout pipe is missing"))?;
	let stderr = child.stderr.take().ok_or_else(|| eyre::eyre!("xurl stderr pipe is missing"))?;
	let (stdout_receiver, stdout_reader) = spawn_bounded_reader(stdout);
	let (stderr_receiver, stderr_reader) = spawn_bounded_reader(stderr);
	let status = match child.wait_timeout(remaining_time(deadline)?)? {
		Some(status) => {
			kill_process_group(child.id());
			status
		},
		None => {
			kill_process_group(child.id());
			let _ = child.kill();
			return Err(eyre::eyre!("xurl execution exceeded its bounded deadline"));
		},
	};
	let stdout =
		receive_bounded_reader(stdout_receiver, stdout_reader, deadline, "xurl stdout reader")?;
	let stderr =
		receive_bounded_reader(stderr_receiver, stderr_reader, deadline, "xurl stderr reader")?;

	Ok(Output { status, stdout, stderr })
}

type ReaderResult = std::io::Result<Vec<u8>>;

fn spawn_bounded_reader(
	reader: impl std::io::Read + Send + 'static,
) -> (mpsc::Receiver<ReaderResult>, thread::JoinHandle<()>) {
	let (sender, receiver) = mpsc::sync_channel(1);
	let handle = thread::spawn(move || {
		let _ = sender.send(drain_bounded(reader));
	});
	(receiver, handle)
}

fn receive_bounded_reader(
	receiver: mpsc::Receiver<ReaderResult>,
	handle: thread::JoinHandle<()>,
	deadline: Instant,
	label: &str,
) -> Result<Vec<u8>> {
	let output =
		receiver.recv_timeout(remaining_time(deadline)?).map_err(|error| match error {
			mpsc::RecvTimeoutError::Timeout => {
				eyre::eyre!("xurl execution exceeded its bounded deadline during output drain")
			},
			mpsc::RecvTimeoutError::Disconnected => eyre::eyre!("{label} failed"),
		})??;
	handle.join().map_err(|_| eyre::eyre!("{label} failed"))?;
	require_time_remaining(deadline)?;
	Ok(output)
}

fn remaining_time(deadline: Instant) -> Result<Duration> {
	deadline
		.checked_duration_since(Instant::now())
		.filter(|remaining| !remaining.is_zero())
		.ok_or_else(|| eyre::eyre!("xurl execution exceeded its bounded deadline"))
}

fn require_time_remaining(deadline: Instant) -> Result<()> {
	remaining_time(deadline).map(|_| ())
}

fn trusted_home_directory() -> Result<PathBuf> {
	let uid = current_uid();
	let suggested = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
	let buffer_size =
		if suggested > 0 { usize::try_from(suggested).unwrap_or(16 * 1024) } else { 16 * 1024 }
			.clamp(1024, 1024 * 1024);
	let mut buffer = vec![0_u8; buffer_size];
	let mut password = MaybeUninit::<libc::passwd>::zeroed();
	let mut result = ptr::null_mut();
	let code = unsafe {
		libc::getpwuid_r(
			uid,
			password.as_mut_ptr(),
			buffer.as_mut_ptr().cast(),
			buffer.len(),
			&mut result,
		)
	};
	if code != 0 || result.is_null() {
		return Err(eyre::eyre!("operating-system home directory is unavailable"));
	}
	let password = unsafe { password.assume_init() };
	if password.pw_dir.is_null() {
		return Err(eyre::eyre!("operating-system home directory is unavailable"));
	}
	let bytes = unsafe { CStr::from_ptr(password.pw_dir) }.to_bytes();
	if bytes.is_empty() || bytes.contains(&0) {
		return Err(eyre::eyre!("operating-system home directory is invalid"));
	}
	let path = PathBuf::from(OsString::from_vec(bytes.to_vec()));
	validate_home_directory(&path)?;

	Ok(path)
}

fn validate_home_directory(path: &Path) -> Result<()> {
	let uid = current_uid();
	let metadata = fs::symlink_metadata(path)
		.map_err(|_| eyre::eyre!("operating-system home directory is unavailable"))?;
	if !path.is_absolute()
		|| metadata.file_type().is_symlink()
		|| !metadata.is_dir()
		|| metadata.uid() != uid
		|| metadata.permissions().mode() & 0o022 != 0
	{
		return Err(eyre::eyre!("operating-system home directory is not trusted"));
	}

	Ok(())
}

fn kill_process_group(child_id: u32) {
	if let Ok(process_group) = i32::try_from(child_id) {
		unsafe {
			libc::kill(-process_group, libc::SIGKILL);
		}
	}
}

fn drain_bounded(mut reader: impl std::io::Read) -> std::io::Result<Vec<u8>> {
	let mut retained = Vec::new();
	let mut buffer = [0_u8; 8192];
	loop {
		let read = reader.read(&mut buffer)?;
		if read == 0 {
			break;
		}
		let remaining = (MAX_XURL_OUTPUT_BYTES + 1).saturating_sub(retained.len());
		retained.extend_from_slice(&buffer[..read.min(remaining)]);
	}

	Ok(retained)
}

fn parse_json_output(bytes: &[u8], label: &str) -> Result<Value> {
	let text = output_text(bytes, label)?;
	let clean = strip_ansi(text);
	if clean.len() > MAX_XURL_OUTPUT_BYTES {
		return Err(eyre::eyre!("{label} exceeds the size limit"));
	}
	serde_json::from_str(clean.trim()).map_err(|_| eyre::eyre!("{label} is not valid JSON"))
}

fn output_text<'a>(bytes: &'a [u8], label: &str) -> Result<&'a str> {
	if bytes.len() > MAX_XURL_OUTPUT_BYTES {
		return Err(eyre::eyre!("{label} exceeds the size limit"));
	}
	std::str::from_utf8(bytes).map_err(|_| eyre::eyre!("{label} is not UTF-8"))
}

fn strip_ansi(value: &str) -> String {
	let mut output = String::with_capacity(value.len());
	let mut characters = value.chars().peekable();
	while let Some(character) = characters.next() {
		if character == '\u{1b}' && characters.peek() == Some(&'[') {
			characters.next();
			for next in characters.by_ref() {
				if next.is_ascii() && ('@'..='~').contains(&next) {
					break;
				}
			}
		} else {
			output.push(character);
		}
	}
	output
}

pub(super) fn failure(operation: &str, output: &Output) -> color_eyre::Report {
	eyre::eyre!(
		"xurl {operation} failed with status {}; stderr_sha256={}",
		output.status,
		sha256(&output.stderr)
	)
}

pub(super) fn canonical_status_url(post_id: &str) -> String {
	format!("https://x.com/{TARGET_ACCOUNT}/status/{post_id}")
}

pub(super) fn sha256(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
	use std::{
		ffi::OsString,
		fs::{self, File},
		io,
		os::unix::{
			ffi::OsStrExt as _,
			fs::{PermissionsExt as _, symlink},
		},
		thread,
		time::{Duration, Instant},
	};

	use super::{
		MAX_XURL_OUTPUT_BYTES, TrustedXurlBinary, install_private_copy_in, read_verified_binary,
		receive_bounded_reader, resolve_trusted_xurl_entrypoint, run_with_deadline,
		run_with_deadline_inner, sha256, spawn_bounded_reader, strip_ansi, trusted_home_directory,
		validate_auth_status_output,
	};

	struct SlowReader;

	impl io::Read for SlowReader {
		fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
			thread::sleep(Duration::from_millis(200));
			Ok(0)
		}
	}

	#[test]
	fn auth_status_uses_only_the_literal_default_app_section() {
		validate_auth_status_output(concat!(
			"▸ personal  [client_id: first…]\n",
			"      oauth2: decodexspace\n",
			"  default  [client_id: second…]\n",
			"      oauth2: hackink\n",
			"    ▸ oauth2: decodexspace\n",
			"  duplicate  [client_id: third…]\n",
			"      oauth2: decodexspace\n",
		))
		.expect("one target label in the literal default section");
		assert!(
			validate_auth_status_output(concat!(
				"▸ personal  [client_id: first…]\n",
				"      oauth2: decodexspace\n",
				"  default  [client_id: second…]\n",
				"      oauth2: hackink\n",
			))
			.is_err()
		);
		assert!(
			validate_auth_status_output(concat!(
				"▸ default  [client_id: first…]\n",
				"      oauth2: decodexspace\n",
				"    ▸ oauth2: decodexspace\n",
				"  other  [client_id: second…]\n",
				"      oauth2: hackink\n",
			))
			.is_err()
		);
	}

	#[test]
	fn ansi_stripping_preserves_unicode() {
		assert_eq!(strip_ansi("中文 \u{1b}[31mverified\u{1b}[0m"), "中文 verified");
	}

	#[test]
	fn bounded_runner_drops_inherited_xurl_endpoint_overrides() {
		let temp = tempfile::tempdir().expect("tempdir");
		let script = executable_script(
			temp.path(),
			"clean-environment",
			"#!/bin/sh\n[ -z \"${API_BASE_URL+x}\" ] || exit 1\nprintf '%s' \"$HOME\"\n",
		);
		let previous = std::env::var_os("API_BASE_URL");
		unsafe {
			std::env::set_var("API_BASE_URL", "https://attacker.invalid");
		}
		let binary = TrustedXurlBinary::open_for_test(&script).expect("trusted test binary");
		let result = run_with_deadline(&binary, std::iter::empty::<&str>(), Duration::from_secs(3));
		unsafe {
			if let Some(value) = previous {
				std::env::set_var("API_BASE_URL", value);
			} else {
				std::env::remove_var("API_BASE_URL");
			}
		}
		let output = result.expect("clean environment probe");
		assert!(output.status.success());
		assert_eq!(output.stdout, binary.home.as_os_str().as_bytes());
	}

	#[test]
	fn bounded_runner_kills_a_hung_process_group() {
		let temp = tempfile::tempdir().expect("tempdir");
		let script = executable_script(temp.path(), "hang", "#!/bin/sh\n/bin/sleep 60\n");
		let binary = TrustedXurlBinary::open_for_test(&script).expect("trusted test binary");
		let started = Instant::now();
		let error =
			run_with_deadline(&binary, std::iter::empty::<&str>(), Duration::from_millis(100))
				.expect_err("hung process must time out")
				.to_string();
		assert!(error.contains("bounded deadline"));
		assert!(started.elapsed() < Duration::from_secs(3));
	}

	#[test]
	fn bounded_runner_deadline_includes_output_drain_and_join() {
		let (receiver, handle) = spawn_bounded_reader(SlowReader);
		let started = Instant::now();
		let error = receive_bounded_reader(
			receiver,
			handle,
			Instant::now() + Duration::from_millis(50),
			"slow reader",
		)
		.expect_err("slow output drain must time out")
		.to_string();

		assert!(error.contains("output drain"), "{error}");
		assert!(started.elapsed() < Duration::from_millis(150));
	}

	#[test]
	fn bounded_runner_drains_but_limits_captured_output() {
		let temp = tempfile::tempdir().expect("tempdir");
		let script = executable_script(
			temp.path(),
			"large-output",
			"#!/bin/sh\nexec /bin/dd if=/dev/zero bs=2097152 count=1 2>/dev/null\n",
		);
		let binary = TrustedXurlBinary::open_for_test(&script).expect("trusted test binary");
		let output = run_with_deadline(&binary, std::iter::empty::<&str>(), Duration::from_secs(3))
			.expect("large output process must finish");
		assert!(output.status.success());
		assert_eq!(output.stdout.len(), MAX_XURL_OUTPUT_BYTES + 1);
	}

	#[test]
	fn bounded_runner_executes_the_pinned_file_after_runtime_path_replacement() {
		let temp = tempfile::tempdir().expect("tempdir");
		let runtime = temp.path().join("runtime");
		let retained = temp.path().join("retained");
		let attacker = temp.path().join("attacker");
		fs::create_dir(&runtime).expect("runtime directory");
		fs::create_dir(&attacker).expect("attacker directory");
		let trusted_path =
			executable_script(&runtime, "xurl-current", "#!/bin/sh\nprintf 'trusted\\n'\n");
		let mut binary =
			TrustedXurlBinary::open_for_test(&trusted_path).expect("pinned test binary");
		binary.home = temp.path().to_path_buf();
		executable_script(&attacker, "xurl-current", "#!/bin/sh\nprintf 'malicious\\n'\nexit 99\n");
		fs::rename(&runtime, &retained).expect("move validated runtime directory");
		symlink(&attacker, &runtime).expect("replace runtime path");

		let output = run_with_deadline(&binary, std::iter::empty::<&str>(), Duration::from_secs(3))
			.expect("pinned executable must run");

		assert!(output.status.success());
		assert_eq!(output.stdout, b"trusted\n");
	}

	#[test]
	fn bounded_runner_deadline_includes_setup_before_spawn() {
		let temp = tempfile::tempdir().expect("tempdir");
		let trusted_path = executable_script(
			temp.path(),
			"xurl-current",
			"#!/bin/sh\nprintf 'spawned' > \"$HOME/spawned\"\n",
		);
		let binary = TrustedXurlBinary::open_for_test(&trusted_path).expect("pinned test binary");
		let started = Instant::now();

		let error = run_with_deadline_inner(
			&binary,
			std::iter::empty::<&str>(),
			Duration::from_millis(50),
			|| {
				thread::sleep(Duration::from_millis(100));
			},
		)
		.expect_err("setup that exhausts the command budget must time out")
		.to_string();

		assert!(error.contains("bounded deadline"));
		assert!(started.elapsed() < Duration::from_secs(2));
		assert!(!temp.path().join("spawned").exists());
	}

	#[test]
	fn private_runtime_gc_retains_only_the_current_copy() {
		let temp = tempfile::tempdir().expect("tempdir");
		let runtime_path = temp.path().join("runtime");
		fs::create_dir(&runtime_path).expect("runtime directory");
		fs::set_permissions(&runtime_path, fs::Permissions::from_mode(0o700))
			.expect("runtime permissions");
		let runtime = File::open(&runtime_path).expect("runtime descriptor");
		let stale = b"#!/bin/sh\nprintf 'stale\\n'\n";
		let stale_digest = sha256(stale);
		write_runtime_entry(&runtime_path, &format!("xurl-{stale_digest}"), stale);
		write_runtime_entry(&runtime_path, ".stage-0123456789abcdef0123456789abcdef", b"partial");
		let current = b"#!/bin/sh\nprintf 'current\\n'\n";
		let current_digest = sha256(current);
		let home = trusted_home_directory().expect("trusted home");

		let binary = install_private_copy_in(
			&runtime,
			current,
			&current_digest,
			&home,
			Instant::now() + Duration::from_secs(3),
		)
		.expect("bounded install");
		let retained = fs::read_dir(&runtime_path)
			.expect("runtime entries")
			.map(|entry| entry.expect("runtime entry").file_name())
			.collect::<Vec<_>>();
		assert_eq!(retained, [OsString::from(format!("xurl-{current_digest}"))]);
		let output = run_with_deadline(&binary, std::iter::empty::<&str>(), Duration::from_secs(3))
			.expect("current copy executes");
		assert_eq!(output.stdout, b"current\n");
	}

	#[test]
	fn private_runtime_gc_fails_closed_on_a_symlink() {
		let temp = tempfile::tempdir().expect("tempdir");
		let runtime_path = temp.path().join("runtime");
		fs::create_dir(&runtime_path).expect("runtime directory");
		fs::set_permissions(&runtime_path, fs::Permissions::from_mode(0o700))
			.expect("runtime permissions");
		let runtime = File::open(&runtime_path).expect("runtime descriptor");
		let current = b"#!/bin/sh\nprintf 'current\\n'\n";
		let current_digest = sha256(current);
		let home = trusted_home_directory().expect("trusted home");
		symlink(
			"/bin/sh",
			runtime_path
				.join("xurl-0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"),
		)
		.expect("malicious symlink");

		let error = install_private_copy_in(
			&runtime,
			current,
			&current_digest,
			&home,
			Instant::now() + Duration::from_secs(3),
		)
		.expect_err("symlink must stop runtime GC")
		.to_string();

		assert!(error.contains("symlink") || error.contains("Too many levels"));
	}

	#[test]
	fn private_xurl_entrypoint_accepts_only_the_fixed_secure_home_relative_path() {
		let root = crate::repo_root().expect("repository root");
		let temp = tempfile::tempdir_in(root).expect("tempdir");
		let bin = temp.path().join(".local/bin");
		fs::create_dir_all(&bin).expect("private bin");
		let script = executable_script(&bin, "xurl", "#!/bin/sh\nexit 0\n");

		let resolved =
			resolve_trusted_xurl_entrypoint(temp.path()).expect("secure private entrypoint");

		assert_eq!(resolved, script);
	}

	#[test]
	fn private_xurl_entrypoint_rejects_group_or_world_writable_home_parents_and_file() {
		let root = crate::repo_root().expect("repository root");
		for mode in [0o770, 0o707] {
			for relative in ["", ".local", ".local/bin", ".local/bin/xurl"] {
				let temp = tempfile::tempdir_in(&root).expect("tempdir");
				let bin = temp.path().join(".local/bin");
				fs::create_dir_all(&bin).expect("private bin");
				executable_script(&bin, "xurl", "#!/bin/sh\nexit 0\n");
				let insecure = temp.path().join(relative);
				fs::set_permissions(&insecure, fs::Permissions::from_mode(mode))
					.expect("unsafe permissions");

				let error = resolve_trusted_xurl_entrypoint(temp.path())
					.expect_err("writable entrypoint chain must fail")
					.to_string();

				assert!(
					error.contains("permissions are not trusted"),
					"{relative} mode {mode:o}: {error}"
				);
			}
		}
	}

	#[test]
	fn private_xurl_entrypoint_rejects_a_final_symlink() {
		let root = crate::repo_root().expect("repository root");
		let temp = tempfile::tempdir_in(root).expect("tempdir");
		let bin = temp.path().join(".local/bin");
		fs::create_dir_all(&bin).expect("private bin");
		let target = executable_script(&bin, "xurl-real", "#!/bin/sh\nexit 0\n");
		symlink(target, bin.join("xurl")).expect("xurl symlink");

		let error = resolve_trusted_xurl_entrypoint(temp.path())
			.expect_err("xurl symlink must fail")
			.to_string();

		assert!(error.contains("unexpected symlink"));
	}

	#[test]
	fn production_binary_reader_rejects_every_unapproved_digest() {
		let temp = tempfile::tempdir().expect("tempdir");
		let binary =
			executable_script(temp.path(), "xurl", "#!/bin/sh\nprintf 'xurl version 1.3.1\\n'\n");

		let error = read_verified_binary(&binary)
			.expect_err("an arbitrary self-reporting binary must fail")
			.to_string();

		assert!(error.contains("approved official 1.3.1 release digest"));
	}

	#[test]
	fn repository_automation_has_no_raw_xurl_execution_path() {
		let root = crate::repo_root().expect("repository root");
		let mut files = Vec::new();
		collect_source_files(&root.join("automations"), &mut files);
		collect_source_files(&root.join("apps/decodex-publisher/src"), &mut files);
		for path in files {
			if path.ends_with("social_xurl/runtime.rs")
				|| path.ends_with("social_xurl/auth_contract.rs")
			{
				continue;
			}
			let source = fs::read_to_string(&path).expect("audited source");
			for forbidden in [
				"subprocess.run([\"xurl",
				"subprocess.run(['xurl",
				"subprocess.Popen([\"xurl",
				"subprocess.Popen(['xurl",
				"os.system(\"xurl",
				"os.system('xurl",
				"Command::new(\"xurl",
				"Command::new('xurl",
				"exec xurl",
				"`xurl ",
			] {
				assert!(
					!source.contains(forbidden),
					"raw xurl execution marker {forbidden:?} found in {}",
					path.display()
				);
			}
			for line in source.lines() {
				let raw_command = line
					.trim_start()
					.strip_prefix("xurl ")
					.and_then(|arguments| arguments.split_whitespace().next())
					.is_some_and(|argument| {
						argument.starts_with('-')
							|| argument.starts_with('/')
							|| matches!(argument, "auth" | "post" | "read")
					});
				assert!(!raw_command, "raw xurl command found in {}", path.display());
			}
		}
	}

	fn write_runtime_entry(root: &std::path::Path, name: &str, bytes: &[u8]) {
		let path = root.join(name);
		fs::write(&path, bytes).expect("runtime entry");
		fs::set_permissions(&path, fs::Permissions::from_mode(0o500))
			.expect("runtime entry permissions");
	}

	fn executable_script(root: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
		let path = root.join(name);
		fs::write(&path, body).expect("script");
		let mut permissions = fs::metadata(&path).expect("metadata").permissions();
		permissions.set_mode(0o700);
		fs::set_permissions(&path, permissions).expect("permissions");
		path
	}

	fn collect_source_files(root: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
		for entry in fs::read_dir(root).expect("source directory") {
			let entry = entry.expect("source entry");
			let path = entry.path();
			if path.is_dir() {
				collect_source_files(&path, files);
			} else if matches!(
				path.extension().and_then(|value| value.to_str()),
				Some("py" | "rs" | "sh" | "toml")
			) || path.extension().and_then(|value| value.to_str()) == Some("md")
				&& (path.components().any(|component| component.as_os_str() == "prompts")
					|| path.file_name().is_some_and(|name| name == "SKILL.md"))
			{
				files.push(path);
			}
		}
	}
}
