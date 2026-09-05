//! Process-level proof that Unix termination signals reach exact transport cleanup.

#![cfg(any(target_os = "linux", target_os = "macos"))]
#![allow(unused_crate_dependencies)]

use std::{
	fs,
	io::{BufRead as _, BufReader},
	os::unix::fs::{MetadataExt as _, PermissionsExt as _},
	path::{Path, PathBuf},
	process::{Child, Command, ExitStatus, Stdio},
	sync::mpsc,
	thread::{self, JoinHandle},
	time::{Duration, Instant},
};

use tempfile::TempDir;

const READY_LINE: &str = "decodex serving WebSocket /v1/ws over same-UID local transport";

#[test]
fn sigint_performs_exact_local_transport_cleanup() {
	assert_signal_cleanup(libc::SIGINT);
}

#[test]
fn sigterm_performs_exact_local_transport_cleanup() {
	assert_signal_cleanup(libc::SIGTERM);
}

#[test]
fn sigkill_stale_socket_is_recovered_by_the_next_daemon() {
	let (_home, canonical_home, socket) = fixture();
	let crashed = RunningDaemon::start(&canonical_home).signal(libc::SIGKILL);

	assert!(!crashed.success(), "SIGKILL must not masquerade as graceful exit");
	assert!(socket.exists(), "SIGKILL leaves the published pathname stale");

	let recovered = RunningDaemon::start(&canonical_home).signal(libc::SIGTERM);

	assert!(recovered.success(), "replacement daemon must shut down cleanly: {recovered}");
	assert!(!socket.exists(), "replacement daemon must clean its exact publication");
}

fn fixture() -> (TempDir, PathBuf, PathBuf) {
	#[cfg(target_os = "macos")]
	let home = TempDir::new_in("/private/tmp").expect("create short daemon test home");
	#[cfg(not(target_os = "macos"))]
	let home = TempDir::new().expect("create daemon test home");
	let canonical_home = home.path().canonicalize().expect("canonicalize daemon test home");
	let root = canonical_home.join(".decodex");
	let bin = canonical_home.join("bin");
	let server = root.join("server");
	let config_file = root.join("config.toml");
	let socket = server.join("decodex.sock");

	fs::create_dir(&root).expect("create daemon test root");
	fs::create_dir(&bin).expect("create isolated daemon test PATH");
	fs::create_dir(&server).expect("create daemon server directory");
	fs::set_permissions(&bin, fs::Permissions::from_mode(0o700))
		.expect("scope isolated daemon test PATH");
	fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("scope daemon test root");
	fs::set_permissions(&server, fs::Permissions::from_mode(0o700))
		.expect("scope daemon server directory");
	fs::write(
		&config_file,
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
			fs::metadata(&root).expect("read daemon root metadata").uid(),
		),
	)
	.expect("write daemon test config");
	fs::set_permissions(config_file, fs::Permissions::from_mode(0o600))
		.expect("scope daemon test config");

	(home, canonical_home, socket)
}

fn assert_signal_cleanup(signal: libc::c_int) {
	let (_home, canonical_home, socket) = fixture();
	let daemon = RunningDaemon::start(&canonical_home);

	assert!(socket.exists(), "ready daemon must retain its published socket");

	let status = daemon.signal(signal);

	assert!(status.success(), "graceful daemon shutdown must succeed: {status}");
	assert!(!socket.exists(), "graceful shutdown must remove the retained socket");

	let lock = socket.with_file_name("decodex.lock");
	let metadata = fs::metadata(&lock).expect("persistent namespace lock remains");

	assert!(metadata.is_file());
	assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
	assert_eq!(metadata.nlink(), 1);
}

struct RunningDaemon {
	child: Child,
	reader: JoinHandle<()>,
}

impl RunningDaemon {
	fn start(home: &Path) -> Self {
		let mut child = Command::new(env!("CARGO_BIN_EXE_decodex"))
			.arg("serve")
			.env("HOME", home)
			.env("PATH", home.join("bin"))
			.stdout(Stdio::piped())
			.spawn()
			.expect("start daemon test process");
		let stdout = child.stdout.take().expect("capture daemon stdout");
		let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
		let reader = thread::spawn(move || {
			for line in BufReader::new(stdout).lines() {
				let line = line.expect("read daemon stdout");

				if line == READY_LINE {
					let _ = ready_sender.send(());
				}
			}
		});

		if ready_receiver.recv_timeout(Duration::from_secs(20)).is_err() {
			let _ = child.kill();
			let status = child.wait().expect("reap unready daemon");
			reader.join().expect("join daemon output reader");
			panic!("daemon did not become ready before timeout: {status}");
		}

		Self { child, reader }
	}

	fn signal(mut self, signal: libc::c_int) -> ExitStatus {
		assert_eq!(
			// SAFETY: the child PID came from `Child`; tests pass only defined Unix signals.
			unsafe { libc::kill(self.child.id() as libc::pid_t, signal) },
			0,
			"send Unix process signal",
		);

		let deadline = Instant::now() + Duration::from_secs(20);
		let status = loop {
			if let Some(status) = self.child.try_wait().expect("poll daemon exit") {
				break status;
			}
			if Instant::now() >= deadline {
				let _ = self.child.kill();
				let status = self.child.wait().expect("reap daemon after shutdown timeout");
				self.reader.join().expect("join daemon output reader");
				panic!("daemon did not exit after Unix signal: {status}");
			}
			thread::sleep(Duration::from_millis(20));
		};

		self.reader.join().expect("join daemon output reader");

		status
	}
}
