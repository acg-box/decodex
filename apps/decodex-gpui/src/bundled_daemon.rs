//! Local-only launcher for the `decodexd` payload carried by `Decodex.app`.

#[cfg(target_os = "macos")]
use std::{
	fs::Metadata,
	io,
	os::{
		fd::{AsRawFd as _, FromRawFd as _, OwnedFd},
		unix::{fs::MetadataExt as _, net::UnixStream},
	},
	path::{Path, PathBuf},
	process::{Command, Stdio},
};
use std::{future::Future, pin::Pin};

use decodex_protocol::{ClientProfile, ProfileKind};
use gpui::{App, AppContext as _, Context, Entity, Global, Subscription};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(
	dead_code,
	reason = "failure variants are selected by the production platform and staged-bundle path"
)]
pub(crate) enum BundledDaemonFailure {
	NotBundled,
	PayloadUnavailable,
	LifetimeChannelUnavailable,
	LaunchFailed,
	UnsupportedPlatform,
}

pub(crate) struct BundledDaemonGuard {
	#[cfg(target_os = "macos")]
	parent_lifetime: UnixStream,
}

impl BundledDaemonGuard {
	pub(crate) fn launch_for_profile(
		profile: &ClientProfile,
	) -> Result<Option<Self>, BundledDaemonFailure> {
		if profile.kind() == ProfileKind::Remote {
			return Ok(None);
		}

		#[cfg(target_os = "macos")]
		{
			Self::launch_macos().map(Some)
		}
		#[cfg(not(target_os = "macos"))]
		{
			Err(BundledDaemonFailure::UnsupportedPlatform)
		}
	}

	#[cfg(target_os = "macos")]
	fn launch_macos() -> Result<Self, BundledDaemonFailure> {
		let executable = std::env::current_exe().map_err(|_| BundledDaemonFailure::NotBundled)?;
		let daemon = bundled_daemon_path(&executable)?;
		let metadata = std::fs::symlink_metadata(&daemon)
			.map_err(|_| BundledDaemonFailure::PayloadUnavailable)?;
		if !is_executable_regular_file(&metadata) || metadata.file_type().is_symlink() {
			return Err(BundledDaemonFailure::PayloadUnavailable);
		}

		let (parent_lifetime, child_lifetime) = lifetime_channel()
			.map_err(|_| BundledDaemonFailure::LifetimeChannelUnavailable)?;
		let child_fd = child_lifetime.as_raw_fd();
		let mut child = Command::new(daemon)
			.arg("serve")
			.arg("--parent-fd")
			.arg(child_fd.to_string())
			.stdin(Stdio::null())
			.stdout(Stdio::null())
			.stderr(Stdio::null())
			.spawn()
			.map_err(|_| BundledDaemonFailure::LaunchFailed)?;
		drop(child_lifetime);

		let _ = std::thread::Builder::new()
			.name("decodexd-reaper".into())
			.spawn(move || {
				let _ = child.wait();
			});

		Ok(Self { parent_lifetime })
	}
}

#[cfg(target_os = "macos")]
impl Drop for BundledDaemonGuard {
	fn drop(&mut self) {
		let _ = self.parent_lifetime.shutdown(std::net::Shutdown::Both);
	}
}

struct BundledDaemonOwner {
	guard: Option<BundledDaemonGuard>,
	_quit: Option<Subscription>,
}

impl BundledDaemonOwner {
	fn new(guard: BundledDaemonGuard, cx: &mut Context<Self>) -> Self {
		let quit = cx.on_app_quit(|owner, _| owner.shutdown());
		Self { guard: Some(guard), _quit: Some(quit) }
	}

	fn shutdown(&mut self) -> Pin<Box<dyn Future<Output = ()> + 'static>> {
		self.guard.take();
		Box::pin(async {})
	}
}

struct BundledDaemonGlobal {
	_owner: Entity<BundledDaemonOwner>,
}

impl Global for BundledDaemonGlobal {}

pub(crate) fn retain(guard: BundledDaemonGuard, cx: &mut App) {
	debug_assert!(
		!cx.has_global::<BundledDaemonGlobal>(),
		"Decodex retains at most one bundled-daemon lifetime"
	);
	let owner = cx.new(|cx| BundledDaemonOwner::new(guard, cx));
	cx.set_global(BundledDaemonGlobal { _owner: owner });
}

#[cfg(target_os = "macos")]
fn bundled_daemon_path(executable: &Path) -> Result<PathBuf, BundledDaemonFailure> {
	let macos = executable.parent().ok_or(BundledDaemonFailure::NotBundled)?;
	if macos.file_name().and_then(|name| name.to_str()) != Some("MacOS") {
		return Err(BundledDaemonFailure::NotBundled);
	}
	let contents = macos.parent().ok_or(BundledDaemonFailure::NotBundled)?;
	if contents.file_name().and_then(|name| name.to_str()) != Some("Contents") {
		return Err(BundledDaemonFailure::NotBundled);
	}
	Ok(contents.join("Helpers/decodexd"))
}

#[cfg(target_os = "macos")]
fn is_executable_regular_file(metadata: &Metadata) -> bool {
	metadata.file_type().is_file() && metadata.mode() & 0o111 != 0
}

#[cfg(target_os = "macos")]
fn lifetime_channel() -> io::Result<(UnixStream, OwnedFd)> {
	let mut descriptors = [-1_i32; 2];
	// SAFETY: `descriptors` has room for the two fds written by `socketpair`.
	if unsafe {
		libc::socketpair(
			libc::AF_UNIX,
			libc::SOCK_STREAM,
			0,
			descriptors.as_mut_ptr(),
		)
	} != 0
	{
		return Err(io::Error::last_os_error());
	}
	// SAFETY: successful `socketpair` returned two uniquely owned live descriptors.
	let parent = unsafe { OwnedFd::from_raw_fd(descriptors[0]) };
	// SAFETY: same ownership transfer for the second descriptor.
	let child = unsafe { OwnedFd::from_raw_fd(descriptors[1]) };
	set_close_on_exec(parent.as_raw_fd(), true)?;
	set_close_on_exec(child.as_raw_fd(), true)?;
	set_close_on_exec(child.as_raw_fd(), false)?;
	Ok((UnixStream::from(parent), child))
}

#[cfg(target_os = "macos")]
fn set_close_on_exec(raw_fd: i32, enabled: bool) -> io::Result<()> {
	// SAFETY: `fcntl` reads flags from one live owned descriptor.
	let flags = unsafe { libc::fcntl(raw_fd, libc::F_GETFD) };
	if flags < 0 {
		return Err(io::Error::last_os_error());
	}
	let updated = if enabled { flags | libc::FD_CLOEXEC } else { flags & !libc::FD_CLOEXEC };
	// SAFETY: `updated` changes only the close-on-exec descriptor flag.
	if unsafe { libc::fcntl(raw_fd, libc::F_SETFD, updated) } != 0 {
		return Err(io::Error::last_os_error());
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[cfg(target_os = "macos")]
	#[test]
	fn staged_executable_resolves_the_one_helper_payload() {
		let executable = Path::new("/Applications/Decodex.app/Contents/MacOS/decodex-gpui");
		assert_eq!(
			bundled_daemon_path(executable).expect("resolve bundled daemon"),
			Path::new("/Applications/Decodex.app/Contents/Helpers/decodexd"),
		);
		assert_eq!(
			bundled_daemon_path(Path::new("/tmp/decodex-gpui")),
			Err(BundledDaemonFailure::NotBundled),
		);
	}

	#[cfg(target_os = "macos")]
	#[test]
	fn lifetime_child_fd_is_inherited_but_parent_fd_is_not() {
		let (parent, child) = lifetime_channel().expect("create lifetime channel");
		// SAFETY: `fcntl` only reads descriptor flags.
		let parent_flags = unsafe { libc::fcntl(parent.as_raw_fd(), libc::F_GETFD) };
		// SAFETY: same read for the child endpoint.
		let child_flags = unsafe { libc::fcntl(child.as_raw_fd(), libc::F_GETFD) };
		assert_ne!(parent_flags & libc::FD_CLOEXEC, 0);
		assert_eq!(child_flags & libc::FD_CLOEXEC, 0);
	}
}
