//! Local-only launcher for the `decodex serve` payload carried by `Decodex.app`.

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
use std::{
	future::Future,
	pin::Pin,
	sync::{Arc, Mutex},
};

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

const MAX_RECOVERY_RESTARTS: u8 = 2;

#[cfg(target_os = "macos")]
#[derive(Clone)]
struct DaemonEnvironment {
	home: PathBuf,
	path: PathBuf,
}

struct BundledDaemonGuard {
	#[cfg(target_os = "macos")]
	parent_lifetime: Option<UnixStream>,
	#[cfg(target_os = "macos")]
	child: Option<std::process::Child>,
}

impl BundledDaemonGuard {
	#[cfg(target_os = "macos")]
	fn launch_macos(
		daemon: &Path,
		environment: Option<&DaemonEnvironment>,
	) -> Result<Self, BundledDaemonFailure> {
		let metadata = std::fs::symlink_metadata(daemon)
			.map_err(|_| BundledDaemonFailure::PayloadUnavailable)?;
		if !is_executable_regular_file(&metadata) || metadata.file_type().is_symlink() {
			return Err(BundledDaemonFailure::PayloadUnavailable);
		}

		let (parent_lifetime, child_lifetime) =
			lifetime_channel().map_err(|_| BundledDaemonFailure::LifetimeChannelUnavailable)?;
		let child_fd = child_lifetime.as_raw_fd();
		let mut command = Command::new(daemon);
		command
			.arg("serve")
			.arg("--parent-fd")
			.arg(child_fd.to_string())
			.stdin(Stdio::null())
			.stdout(Stdio::null())
			.stderr(Stdio::null());
		if let Some(environment) = environment {
			command.env("HOME", &environment.home).env("PATH", &environment.path);
		}
		let child = command.spawn().map_err(|_| BundledDaemonFailure::LaunchFailed)?;
		drop(child_lifetime);

		Ok(Self { parent_lifetime: Some(parent_lifetime), child: Some(child) })
	}

	#[cfg(target_os = "macos")]
	fn request_shutdown(&mut self) {
		if let Some(parent_lifetime) = self.parent_lifetime.take() {
			let _ = parent_lifetime.shutdown(std::net::Shutdown::Both);
		}
	}

	#[cfg(target_os = "macos")]
	fn stop_for_restart(mut self) {
		self.request_shutdown();
		let Some(mut child) = self.child.take() else {
			return;
		};
		for _ in 0..20 {
			if child.try_wait().ok().flatten().is_some() {
				return;
			}
			std::thread::sleep(std::time::Duration::from_millis(25));
		}
		// This handle names only the child spawned with our private lifetime channel.
		let _ = child.kill();
		let _ = child.wait();
	}
}

#[cfg(target_os = "macos")]
impl Drop for BundledDaemonGuard {
	fn drop(&mut self) {
		self.request_shutdown();
		if let Some(mut child) = self.child.take() {
			let _ = std::thread::Builder::new().name("decodex-service-reaper".into()).spawn(
				move || {
					let _ = child.wait();
				},
			);
		}
	}
}

struct SupervisorState {
	guard: Option<BundledDaemonGuard>,
	restarts: u8,
}

/// Ownership-aware recovery for the daemon child carried by this exact app bundle.
pub(crate) struct BundledDaemonSupervisor {
	#[cfg(target_os = "macos")]
	daemon: PathBuf,
	#[cfg(target_os = "macos")]
	environment: Option<DaemonEnvironment>,
	state: Mutex<SupervisorState>,
}

impl crate::client_lifecycle::AppOwnedDaemonRecovery for BundledDaemonSupervisor {
	fn recover_transport(&self) -> bool {
		self.recover_transport()
	}
}

impl BundledDaemonSupervisor {
	pub(crate) fn launch_for_profile(
		profile: &ClientProfile,
	) -> Result<Option<Arc<Self>>, BundledDaemonFailure> {
		if profile.kind() == ProfileKind::Remote {
			return Ok(None);
		}

		#[cfg(target_os = "macos")]
		{
			let executable =
				std::env::current_exe().map_err(|_| BundledDaemonFailure::NotBundled)?;
			let daemon = bundled_daemon_path(&executable)?;
			let guard = BundledDaemonGuard::launch_macos(&daemon, None)?;
			Ok(Some(Arc::new(Self {
				daemon,
				environment: None,
				state: Mutex::new(SupervisorState { guard: Some(guard), restarts: 0 }),
			})))
		}
		#[cfg(not(target_os = "macos"))]
		{
			Err(BundledDaemonFailure::UnsupportedPlatform)
		}
	}

	/// Restart only the child represented by the retained private lifetime channel.
	/// Return whether this supervisor retains authority for a later bounded attempt.
	pub(crate) fn recover_transport(&self) -> bool {
		let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
		if state.restarts >= MAX_RECOVERY_RESTARTS {
			return false;
		}
		state.restarts += 1;

		#[cfg(target_os = "macos")]
		{
			if let Some(guard) = state.guard.take() {
				guard.stop_for_restart();
			}
			match BundledDaemonGuard::launch_macos(&self.daemon, self.environment.as_ref()) {
				Ok(guard) => {
					state.guard = Some(guard);
					true
				},
				Err(_) => state.restarts < MAX_RECOVERY_RESTARTS,
			}
		}
		#[cfg(not(target_os = "macos"))]
		{
			false
		}
	}

	#[cfg(all(test, target_os = "macos"))]
	fn launch_test(
		daemon: PathBuf,
		home: PathBuf,
		path: PathBuf,
	) -> Result<Arc<Self>, BundledDaemonFailure> {
		let environment = DaemonEnvironment { home, path };
		let guard = BundledDaemonGuard::launch_macos(&daemon, Some(&environment))?;
		Ok(Arc::new(Self {
			daemon,
			environment: Some(environment),
			state: Mutex::new(SupervisorState { guard: Some(guard), restarts: 0 }),
		}))
	}

	#[cfg(all(test, target_os = "macos"))]
	fn child_id(&self) -> Option<u32> {
		self.state
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
			.guard
			.as_ref()
			.and_then(|guard| guard.child.as_ref())
			.map(std::process::Child::id)
	}

	#[cfg(all(test, target_os = "macos"))]
	fn child_has_exited(&self) -> bool {
		self.state
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
			.guard
			.as_mut()
			.and_then(|guard| guard.child.as_mut())
			.is_some_and(|child| child.try_wait().ok().flatten().is_some())
	}

	fn shutdown(&self) {
		self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).guard.take();
	}
}

struct BundledDaemonOwner {
	supervisor: Arc<BundledDaemonSupervisor>,
	_quit: Option<Subscription>,
}

impl BundledDaemonOwner {
	fn new(supervisor: Arc<BundledDaemonSupervisor>, cx: &mut Context<Self>) -> Self {
		let quit = cx.on_app_quit(|owner, _| owner.shutdown());
		Self { supervisor, _quit: Some(quit) }
	}

	fn shutdown(&mut self) -> Pin<Box<dyn Future<Output = ()> + 'static>> {
		self.supervisor.shutdown();
		Box::pin(async {})
	}
}

struct BundledDaemonGlobal {
	_owner: Entity<BundledDaemonOwner>,
}

impl Global for BundledDaemonGlobal {}

pub(crate) fn retain(supervisor: Arc<BundledDaemonSupervisor>, cx: &mut App) {
	debug_assert!(
		!cx.has_global::<BundledDaemonGlobal>(),
		"Decodex retains at most one bundled-daemon lifetime"
	);
	let owner = cx.new(|cx| BundledDaemonOwner::new(supervisor, cx));
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
	Ok(contents.join("Helpers/decodex"))
}

#[cfg(target_os = "macos")]
fn is_executable_regular_file(metadata: &Metadata) -> bool {
	metadata.file_type().is_file() && metadata.mode() & 0o111 != 0
}

#[cfg(target_os = "macos")]
fn lifetime_channel() -> io::Result<(UnixStream, OwnedFd)> {
	let mut descriptors = [-1_i32; 2];
	// SAFETY: `descriptors` has room for the two fds written by `socketpair`.
	if unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, descriptors.as_mut_ptr()) }
		!= 0
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
	use std::{
		fs,
		os::unix::fs::PermissionsExt as _,
		process::Child,
		time::{Duration, Instant},
	};

	use decodex_protocol::DoctorClient;
	use tempfile::TempDir;

	use super::*;

	#[cfg(target_os = "macos")]
	#[test]
	fn staged_executable_resolves_the_one_helper_payload() {
		let executable = Path::new("/Applications/Decodex.app/Contents/MacOS/decodex-gpui");
		assert_eq!(
			bundled_daemon_path(executable).expect("resolve bundled daemon"),
			Path::new("/Applications/Decodex.app/Contents/Helpers/decodex"),
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

	#[cfg(target_os = "macos")]
	#[test]
	#[ignore = "run through scripts/test_gpui_bundled_daemon_supervision.sh with a freshly built decodex"]
	fn process_listener_loss_restarts_exact_owned_daemon_and_rebinds_client() {
		let fixture = ProcessFixture::new();
		let supervisor = BundledDaemonSupervisor::launch_test(
			fixture.daemon.clone(),
			fixture.home.clone(),
			fixture.path.clone(),
		)
		.expect("launch isolated app-owned daemon");
		let runtime = tokio::runtime::Builder::new_current_thread()
			.enable_all()
			.build()
			.expect("build isolated client runtime");

		wait_for_client(&runtime, &fixture.root);
		let first_pid = supervisor.child_id().expect("owned daemon PID is retained");
		fs::remove_file(&fixture.socket).expect("remove exact isolated canonical socket");
		wait_until("listener owner exits after publication loss", || supervisor.child_has_exited());

		assert!(supervisor.recover_transport(), "owned transport recovery remains available");
		wait_for_client(&runtime, &fixture.root);
		let replacement_pid = supervisor.child_id().expect("replacement daemon PID is retained");

		assert_ne!(replacement_pid, first_pid, "recovery must launch a fresh owned process");
		assert!(process_is_alive(replacement_pid));
		supervisor.shutdown();
		wait_until("recovered owned daemon exits with the app lifetime", || {
			!process_is_alive(replacement_pid)
		});
	}

	#[cfg(target_os = "macos")]
	#[test]
	#[ignore = "run through scripts/test_gpui_bundled_daemon_supervision.sh with a freshly built decodex"]
	fn process_recovery_never_terminates_independently_managed_daemon() {
		let fixture = ProcessFixture::new();
		let mut independent = fixture.launch_independent();
		let runtime = tokio::runtime::Builder::new_current_thread()
			.enable_all()
			.build()
			.expect("build isolated client runtime");
		wait_for_client(&runtime, &fixture.root);
		let independent_pid = independent.id();
		let independent_socket_inode =
			fs::symlink_metadata(&fixture.socket).expect("read independent daemon socket").ino();

		let supervisor = BundledDaemonSupervisor::launch_test(
			fixture.daemon.clone(),
			fixture.home.clone(),
			fixture.path.clone(),
		)
		.expect("launch isolated bundled contender");
		wait_until("bundled contender observes the independent singleton", || {
			supervisor.child_has_exited()
		});
		assert!(supervisor.recover_transport(), "bounded recovery may launch one more contender");
		wait_until("replacement contender also preserves the independent singleton", || {
			supervisor.child_has_exited()
		});

		assert!(process_is_alive(independent_pid), "supervisor must not kill an unowned daemon");
		assert_eq!(
			fs::symlink_metadata(&fixture.socket)
				.expect("independent socket remains published")
				.ino(),
			independent_socket_inode,
			"supervisor must not unlink or replace an unowned publication"
		);
		wait_for_client(&runtime, &fixture.root);
		supervisor.shutdown();
		assert!(process_is_alive(independent_pid), "supervisor shutdown owns no foreign process");
		independent.stop();
	}

	#[cfg(target_os = "macos")]
	struct ProcessFixture {
		_temporary: TempDir,
		daemon: PathBuf,
		home: PathBuf,
		path: PathBuf,
		root: PathBuf,
		socket: PathBuf,
	}

	#[cfg(target_os = "macos")]
	impl ProcessFixture {
		fn new() -> Self {
			let temporary =
				TempDir::new_in("/private/tmp").expect("create short process test home");
			let home = temporary.path().canonicalize().expect("canonicalize process test home");
			let root = home.join(".decodex");
			let path = home.join("bin");
			let server = root.join("server");
			fs::create_dir(&root).expect("create isolated Decodex root");
			fs::create_dir(&path).expect("create isolated executable path");
			fs::create_dir(&server).expect("create isolated server root");
			fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
				.expect("scope isolated Decodex root");
			fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
				.expect("scope isolated executable path");
			fs::set_permissions(&server, fs::Permissions::from_mode(0o700))
				.expect("scope isolated server root");
			let config = root.join("config.toml");
			fs::write(
				&config,
				format!(
					r#"version = 1
active_profile = "local"

[profiles.local]
kind = "local"
policy = "same_uid"
service_owner_uid = {}

[cache]
max_entries = 16
max_bytes = 1048576
max_entry_bytes = 65536
"#,
					fs::metadata(&root).expect("read isolated root metadata").uid(),
				),
			)
			.expect("write isolated client/server config");
			fs::set_permissions(&config, fs::Permissions::from_mode(0o600))
				.expect("scope isolated client/server config");
			let daemon = PathBuf::from(
				std::env::var_os("DECODEX_TEST_SERVICE")
					.expect("repository process test runner sets DECODEX_TEST_SERVICE"),
			);
			assert!(
				is_executable_regular_file(
					&fs::symlink_metadata(&daemon).expect("read real decodex test binary")
				),
				"process fixture requires an executable real decodex binary"
			);
			let socket = server.join("decodex.sock");
			Self { _temporary: temporary, daemon, home, path, root, socket }
		}

		fn launch_independent(&self) -> IndependentDaemon {
			let child = Command::new(&self.daemon)
				.arg("serve")
				.env("HOME", &self.home)
				.env("PATH", &self.path)
				.stdin(Stdio::null())
				.stdout(Stdio::null())
				.stderr(Stdio::null())
				.spawn()
				.expect("launch independently managed daemon");
			IndependentDaemon { child: Some(child) }
		}
	}

	#[cfg(target_os = "macos")]
	struct IndependentDaemon {
		child: Option<Child>,
	}

	#[cfg(target_os = "macos")]
	impl IndependentDaemon {
		fn id(&self) -> u32 {
			self.child.as_ref().expect("independent daemon remains retained").id()
		}

		fn stop(&mut self) {
			let Some(mut child) = self.child.take() else {
				return;
			};
			// SAFETY: this PID is the exact child owned by this isolated process fixture.
			let _ = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
			let deadline = Instant::now() + Duration::from_secs(20);
			loop {
				if child.try_wait().expect("poll independent daemon").is_some() {
					return;
				}
				if Instant::now() >= deadline {
					let _ = child.kill();
					let _ = child.wait();
					panic!("independent daemon did not stop before timeout");
				}
				std::thread::sleep(Duration::from_millis(20));
			}
		}
	}

	#[cfg(target_os = "macos")]
	impl Drop for IndependentDaemon {
		fn drop(&mut self) {
			if let Some(mut child) = self.child.take() {
				let _ = child.kill();
				let _ = child.wait();
			}
		}
	}

	#[cfg(target_os = "macos")]
	fn wait_for_client(runtime: &tokio::runtime::Runtime, root: &Path) {
		let deadline = Instant::now() + Duration::from_secs(20);
		loop {
			if let Ok(profile) = ClientProfile::load(root, None)
				&& runtime.block_on(DoctorClient::new(profile).query()).is_ok()
			{
				return;
			}
			assert!(Instant::now() < deadline, "new client did not connect before timeout");
			std::thread::sleep(Duration::from_millis(25));
		}
	}

	#[cfg(target_os = "macos")]
	fn wait_until(label: &str, mut predicate: impl FnMut() -> bool) {
		let deadline = Instant::now() + Duration::from_secs(20);
		while !predicate() {
			assert!(Instant::now() < deadline, "{label}");
			std::thread::sleep(Duration::from_millis(20));
		}
	}

	#[cfg(target_os = "macos")]
	fn process_is_alive(pid: u32) -> bool {
		// SAFETY: signal 0 performs a liveness/permission check and has no process effect.
		unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
	}
}
