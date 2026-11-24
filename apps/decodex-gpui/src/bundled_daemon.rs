//! On-demand launcher for the independent local service in Decodex.app.
use std::{path::Path, process::Stdio};

use tokio::{process::Command, task::JoinHandle};

use crate::{
	client_lifecycle::{LocalServiceLauncher, ServiceStartup},
	component_runtime::{PreparationState, RuntimePreparation},
};
use decodex_protocol::{ClientProfile, ProfileKind};

pub(crate) struct BundledService {
	command: Command,
	preparation: RuntimePreparation,
	waiter: Option<JoinHandle<()>>,
}

impl BundledService {
	pub(crate) fn for_profile(
		profile: &ClientProfile,
		preparation: RuntimePreparation,
	) -> Option<Self> {
		if !cfg!(target_os = "macos") || profile.kind() != ProfileKind::Local {
			return None;
		}
		let executable = std::env::current_exe().ok()?;
		let macos = executable.parent().filter(|dir| dir.ends_with("MacOS"))?;
		let contents = macos.parent().filter(|dir| dir.ends_with("Contents"))?;
		Self::new(&contents.join("Helpers/decodex"), preparation)
	}

	fn new(executable: &Path, preparation: RuntimePreparation) -> Option<Self> {
		let metadata = executable.symlink_metadata().ok()?;
		if !metadata.is_file() {
			return None;
		}
		#[cfg(unix)]
		{
			use std::os::unix::fs::PermissionsExt as _;
			if metadata.permissions().mode() & 0o111 == 0 {
				return None;
			}
		}
		let mut command = Command::new(executable);
		command.arg("serve").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
		#[cfg(unix)]
		command.process_group(0);
		Some(Self { command, preparation, waiter: None })
	}
}

impl LocalServiceLauncher for BundledService {
	fn start(&mut self) -> ServiceStartup {
		if self.waiter.as_ref().is_some_and(|waiter| !waiter.is_finished()) {
			return ServiceStartup::Connecting;
		}

		match self.preparation.snapshot().state {
			PreparationState::Unchecked => {
				self.preparation.prepare();
				return ServiceStartup::Preparing { failed: false };
			},
			PreparationState::Preparing(_) => return ServiceStartup::Preparing { failed: false },
			PreparationState::Failed(_) => return ServiceStartup::Preparing { failed: true },
			PreparationState::Ready => {},
		}

		// The service arbitrates concurrent starts with its existing endpoint lock.
		// The task reaps our child. Disconnecting or dropping it never signals the service.
		if let Ok(mut child) = self.command.spawn() {
			self.waiter = Some(tokio::spawn(async move {
				let _ = child.wait().await;
			}));
		}
		ServiceStartup::Connecting
	}
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
	use super::{
		BundledService, ClientProfile, LocalServiceLauncher, PreparationState, RuntimePreparation,
		ServiceStartup,
	};
	use decodex_protocol::DoctorClient;
	use std::{
		fs,
		os::unix::fs::{MetadataExt as _, PermissionsExt as _},
		path::PathBuf,
		time::{Duration, Instant},
	};
	use tempfile::TempDir;
	struct ProcessFixture {
		_temporary: TempDir,
		daemon: PathBuf,
		home: PathBuf,
		path: PathBuf,
		root: PathBuf,
		socket: PathBuf,
	}

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

			let socket = server.join("decodex.sock");

			Self { _temporary: temporary, daemon, home, path, root, socket }
		}

		fn launcher(&self, preparation: RuntimePreparation) -> BundledService {
			let mut launcher =
				BundledService::new(&self.daemon, preparation).expect("real service executable");
			launcher.command.env("HOME", &self.home).env("PATH", &self.path);
			launcher
		}
	}

	#[tokio::test]
	#[ignore = "run through scripts/test_gpui_bundled_daemon_supervision.sh with a freshly built decodex"]
	async fn process_keeps_the_same_service_after_client_exit_and_concurrent_starts() {
		let fixture = ProcessFixture::new();
		let preparation = RuntimePreparation::fixture(PreparationState::Preparing(
			decodex_components::InstallProgress::Waiting,
		));
		let mut first = fixture.launcher(preparation.clone());
		assert!(matches!(first.start(), ServiceStartup::Preparing { failed: false }));
		preparation.finish_fixture(Err("Setup failed".into()));
		assert!(matches!(first.start(), ServiceStartup::Preparing { failed: true }));
		assert!(first.waiter.is_none());

		preparation.finish_fixture(Ok(()));
		let mut second = fixture.launcher(preparation);
		first.start();
		second.start();
		let pid = wait_for_service(&fixture).await;
		let inode = fixture.socket.metadata().unwrap().ino();

		drop(first);
		drop(second);
		assert_eq!(wait_for_service(&fixture).await, pid);
		assert_eq!(fixture.socket.metadata().unwrap().ino(), inode);

		// The peer PID came from the private fixture socket, whose directory we still own.
		assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
		let deadline = Instant::now() + Duration::from_secs(20);
		while fixture.socket.exists() {
			assert!(Instant::now() < deadline, "service did not release its endpoint");
			tokio::time::sleep(Duration::from_millis(25)).await;
		}
	}

	async fn wait_for_service(fixture: &ProcessFixture) -> i32 {
		let deadline = Instant::now() + Duration::from_secs(20);
		loop {
			if let Ok(profile) = ClientProfile::load(&fixture.root, None)
				&& DoctorClient::new(profile).query().await.is_ok()
			{
				let stream = tokio::net::UnixStream::connect(&fixture.socket).await.unwrap();
				return stream.peer_cred().unwrap().pid().unwrap();
			}
			assert!(Instant::now() < deadline, "service did not accept a fresh client");
			tokio::time::sleep(Duration::from_millis(25)).await;
		}
	}
}
