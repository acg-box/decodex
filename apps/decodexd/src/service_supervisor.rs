//! Credential-negative local lifecycle owner for PostgreSQL and `decodexd`.

use std::{
	error::Error,
	ffi::OsString,
	fmt::{Display, Formatter},
	fs::{self, File, OpenOptions},
	io::{Read as _, Write as _},
	os::unix::fs::{FileTypeExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _},
	path::{Path, PathBuf},
	process::{Child, Command, ExitStatus, Stdio},
	thread,
	time::{Duration, Instant},
};

use crate::ShutdownSignals;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const DAEMON_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(240);
const POSTGRES_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(250);

pub(super) struct ServiceSupervisorConfig {
	pub(super) postgres: PathBuf,
	pub(super) pg_isready: PathBuf,
	pub(super) data_directory: PathBuf,
	pub(super) socket_directory: PathBuf,
	pub(super) port: u16,
	pub(super) working_directory: PathBuf,
}

pub(super) async fn supervise(
	config: ServiceSupervisorConfig,
) -> Result<(), ServiceSupervisorError> {
	validate_config(&config)?;
	let socket_directory_identity = secure_directory_identity(&config.socket_directory)?;
	let executable = std::env::current_exe()
		.map_err(|_| ServiceSupervisorError::new("cannot resolve decodexd"))?;
	validate_command_path(&executable)?;
	let mut signals = ShutdownSignals::new()
		.map_err(|_| ServiceSupervisorError::new("cannot install supervisor signal handlers"))?;
	let mut postgres = spawn_postgres(&config)?;
	let postgres_identity =
		match wait_for_postgres(&config, &mut postgres, &mut signals, socket_directory_identity)
			.await
		{
			Ok(PostgresStartup::Ready(identity)) => identity,
			Ok(PostgresStartup::ShutdownRequested) => {
				stop_child(&mut postgres, ChildKind::Postgres);
				return Ok(());
			},
			Err(error) => {
				stop_child(&mut postgres, ChildKind::Postgres);
				return Err(error);
			},
		};
	let mut daemon = match spawn_daemon(&config, &executable) {
		Ok(child) => child,
		Err(error) => {
			stop_child(&mut postgres, ChildKind::Postgres);
			return Err(error);
		},
	};

	let _ = writeln!(std::io::stdout().lock(), "decodexd local supervisor ready");
	loop {
		tokio::select! {
			signal = signals.recv() => {
				if signal.is_err() {
					stop_child(&mut daemon, ChildKind::Daemon);
					stop_child(&mut postgres, ChildKind::Postgres);
					return Err(ServiceSupervisorError::new("supervisor signal handling failed"));
				}
				stop_child(&mut daemon, ChildKind::Daemon);
				stop_child(&mut postgres, ChildKind::Postgres);
				return Ok(());
			},
			_ = tokio::time::sleep(POLL_INTERVAL) => {},
		}

		if child_exit(&mut postgres)?.is_some() {
			stop_child(&mut daemon, ChildKind::Daemon);
			return Err(ServiceSupervisorError::new("PostgreSQL exited"));
		}
		if !postgres_identity.is_current(&config)? {
			stop_child(&mut daemon, ChildKind::Daemon);
			stop_child(&mut postgres, ChildKind::Postgres);
			return Err(ServiceSupervisorError::new("PostgreSQL identity changed"));
		}
		if child_exit(&mut daemon)?.is_some() {
			stop_child(&mut postgres, ChildKind::Postgres);
			return Err(ServiceSupervisorError::new("decodexd child exited"));
		}
	}
}

fn validate_config(config: &ServiceSupervisorConfig) -> Result<(), ServiceSupervisorError> {
	for path in [
		&config.postgres,
		&config.pg_isready,
		&config.data_directory,
		&config.socket_directory,
		&config.working_directory,
	] {
		if !path.is_absolute() {
			return Err(ServiceSupervisorError::new("local supervisor paths must be absolute"));
		}
	}
	if config.port == 0 {
		return Err(ServiceSupervisorError::new("PostgreSQL port is invalid"));
	}
	validate_command_path(&config.postgres)?;
	validate_command_path(&config.pg_isready)?;
	validate_private_data_directory(&config.data_directory)?;
	validate_directory(&config.working_directory)
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct StableObjectIdentity {
	device: u64,
	inode: u64,
	uid: u32,
	mode: u32,
	kind: FileKind,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum FileKind {
	Directory,
	Regular,
	Socket,
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct FileIdentity {
	stable: StableObjectIdentity,
	links: u64,
	length: u64,
	modified_seconds: i64,
	modified_nanoseconds: i64,
	changed_seconds: i64,
	changed_nanoseconds: i64,
}

impl FileIdentity {
	fn from_metadata(metadata: &fs::Metadata) -> Result<Self, ServiceSupervisorError> {
		let file_type = metadata.file_type();
		let kind = if file_type.is_dir() {
			FileKind::Directory
		} else if file_type.is_file() {
			FileKind::Regular
		} else if file_type.is_socket() {
			FileKind::Socket
		} else {
			return Err(ServiceSupervisorError::new("unsupported filesystem object"));
		};
		Ok(Self {
			stable: StableObjectIdentity {
				device: metadata.dev(),
				inode: metadata.ino(),
				uid: metadata.uid(),
				mode: metadata.permissions().mode() & 0o777,
				kind,
			},
			links: metadata.nlink(),
			length: metadata.len(),
			modified_seconds: metadata.mtime(),
			modified_nanoseconds: metadata.mtime_nsec(),
			changed_seconds: metadata.ctime(),
			changed_nanoseconds: metadata.ctime_nsec(),
		})
	}
}

#[derive(Clone, Copy)]
struct PostgresIdentity {
	process_id: u32,
	socket_directory: StableObjectIdentity,
	socket: StableObjectIdentity,
	generation: StableObjectIdentity,
}

impl PostgresIdentity {
	fn is_current(self, config: &ServiceSupervisorConfig) -> Result<bool, ServiceSupervisorError> {
		if secure_directory_identity(&config.socket_directory)? != self.socket_directory
			|| socket_identity(&postgres_socket_path(config))? != self.socket
		{
			return Ok(false);
		}
		let generation_path = config.data_directory.join("postmaster.pid");
		let generation = secure_regular_file_identity(&generation_path, 64 * 1024)?.stable;
		Ok(generation == self.generation
			&& read_postmaster_pid(&generation_path)? == self.process_id)
	}
}

enum PostgresStartup {
	Ready(PostgresIdentity),
	ShutdownRequested,
}

fn validate_command_path(path: &Path) -> Result<(), ServiceSupervisorError> {
	let metadata = fs::symlink_metadata(path)
		.map_err(|_| ServiceSupervisorError::new("required executable is unavailable"))?;
	if metadata.file_type().is_symlink()
		|| !metadata.is_file()
		|| metadata.permissions().mode() & 0o111 == 0
	{
		return Err(ServiceSupervisorError::new("required executable is unsafe"));
	}
	Ok(())
}

fn validate_private_data_directory(path: &Path) -> Result<(), ServiceSupervisorError> {
	let metadata = fs::symlink_metadata(path)
		.map_err(|_| ServiceSupervisorError::new("PostgreSQL data directory is unavailable"))?;
	let effective_uid = unsafe { libc::geteuid() };
	if metadata.file_type().is_symlink()
		|| !metadata.is_dir()
		|| metadata.uid() != effective_uid
		|| metadata.permissions().mode() & 0o777 != 0o700
	{
		return Err(ServiceSupervisorError::new("PostgreSQL data directory is unsafe"));
	}
	Ok(())
}

fn validate_directory(path: &Path) -> Result<(), ServiceSupervisorError> {
	let metadata = fs::symlink_metadata(path)
		.map_err(|_| ServiceSupervisorError::new("required directory missing"))?;
	if metadata.file_type().is_symlink() || !metadata.is_dir() {
		return Err(ServiceSupervisorError::new("required directory is unsafe"));
	}
	Ok(())
}

fn secure_directory_identity(path: &Path) -> Result<StableObjectIdentity, ServiceSupervisorError> {
	let metadata = fs::symlink_metadata(path)
		.map_err(|_| ServiceSupervisorError::new("PostgreSQL socket directory is unavailable"))?;
	let identity = FileIdentity::from_metadata(&metadata)?;
	let effective_uid = unsafe { libc::geteuid() };
	if metadata.file_type().is_symlink()
		|| identity.stable.kind != FileKind::Directory
		|| identity.stable.uid != effective_uid
		|| identity.stable.mode != 0o700
	{
		return Err(ServiceSupervisorError::new("PostgreSQL socket directory is unsafe"));
	}
	Ok(identity.stable)
}

fn secure_regular_file_identity(
	path: &Path,
	maximum_bytes: u64,
) -> Result<FileIdentity, ServiceSupervisorError> {
	let metadata = fs::symlink_metadata(path)
		.map_err(|_| ServiceSupervisorError::new("PostgreSQL generation is unavailable"))?;
	let identity = FileIdentity::from_metadata(&metadata)?;
	let effective_uid = unsafe { libc::geteuid() };
	if metadata.file_type().is_symlink()
		|| identity.stable.kind != FileKind::Regular
		|| identity.stable.uid != effective_uid
		|| identity.links != 1
		|| identity.length > maximum_bytes
	{
		return Err(ServiceSupervisorError::new("PostgreSQL generation is unsafe"));
	}
	Ok(identity)
}

fn open_secure_regular(path: &Path, maximum_bytes: u64) -> Result<File, ServiceSupervisorError> {
	let expected = secure_regular_file_identity(path, maximum_bytes)?;
	let file = OpenOptions::new()
		.read(true)
		.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
		.open(path)
		.map_err(|_| ServiceSupervisorError::new("PostgreSQL generation cannot be opened"))?;
	let actual =
		FileIdentity::from_metadata(&file.metadata().map_err(|_| {
			ServiceSupervisorError::new("PostgreSQL generation cannot be inspected")
		})?)?;
	if actual != expected {
		return Err(ServiceSupervisorError::new("PostgreSQL generation changed during open"));
	}
	Ok(file)
}

fn spawn_postgres(config: &ServiceSupervisorConfig) -> Result<Child, ServiceSupervisorError> {
	Command::new(&config.postgres)
		.arg("-D")
		.arg(&config.data_directory)
		.arg("-k")
		.arg(&config.socket_directory)
		.arg("-p")
		.arg(config.port.to_string())
		.arg("-c")
		.arg("listen_addresses=")
		.current_dir(&config.working_directory)
		.stdin(Stdio::null())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.spawn()
		.map_err(|_| ServiceSupervisorError::new("PostgreSQL cannot be started"))
}

async fn wait_for_postgres(
	config: &ServiceSupervisorConfig,
	postgres: &mut Child,
	signals: &mut ShutdownSignals,
	socket_directory_identity: StableObjectIdentity,
) -> Result<PostgresStartup, ServiceSupervisorError> {
	let deadline = Instant::now() + STARTUP_TIMEOUT;
	loop {
		if child_exit(postgres)?.is_some() {
			return Err(ServiceSupervisorError::new("PostgreSQL exited before readiness"));
		}
		if secure_directory_identity(&config.socket_directory)? != socket_directory_identity {
			return Err(ServiceSupervisorError::new("PostgreSQL socket directory changed"));
		}
		if postgres_is_ready(config)? {
			let generation_path = config.data_directory.join("postmaster.pid");
			let generation = secure_regular_file_identity(&generation_path, 64 * 1024)?.stable;
			let process_id = read_postmaster_pid(&generation_path)?;
			if process_id != postgres.id() {
				return Err(ServiceSupervisorError::new("PostgreSQL generation is invalid"));
			}
			return Ok(PostgresStartup::Ready(PostgresIdentity {
				process_id,
				socket_directory: socket_directory_identity,
				socket: socket_identity(&postgres_socket_path(config))?,
				generation,
			}));
		}
		if Instant::now() >= deadline {
			return Err(ServiceSupervisorError::new("PostgreSQL readiness timed out"));
		}
		tokio::select! {
			signal = signals.recv() => {
				signal.map_err(|_| ServiceSupervisorError::new("supervisor signal handling failed"))?;
				return Ok(PostgresStartup::ShutdownRequested);
			},
			_ = tokio::time::sleep(POLL_INTERVAL) => {},
		}
	}
}

fn postgres_is_ready(config: &ServiceSupervisorConfig) -> Result<bool, ServiceSupervisorError> {
	let status = Command::new(&config.pg_isready)
		.arg("-q")
		.arg("-h")
		.arg(&config.socket_directory)
		.arg("-p")
		.arg(config.port.to_string())
		.arg("-d")
		.arg("postgres")
		.arg("-t")
		.arg("1")
		.current_dir(&config.working_directory)
		.stdin(Stdio::null())
		.stdout(Stdio::null())
		.stderr(Stdio::null())
		.status()
		.map_err(|_| ServiceSupervisorError::new("PostgreSQL readiness probe failed"))?;
	Ok(status.success())
}

fn postgres_socket_path(config: &ServiceSupervisorConfig) -> PathBuf {
	config.socket_directory.join(format!(".s.PGSQL.{}", config.port))
}

fn socket_identity(path: &Path) -> Result<StableObjectIdentity, ServiceSupervisorError> {
	let metadata = fs::symlink_metadata(path)
		.map_err(|_| ServiceSupervisorError::new("PostgreSQL socket is unavailable"))?;
	let identity = FileIdentity::from_metadata(&metadata)?;
	let effective_uid = unsafe { libc::geteuid() };
	if metadata.file_type().is_symlink()
		|| identity.stable.kind != FileKind::Socket
		|| identity.stable.uid != effective_uid
	{
		return Err(ServiceSupervisorError::new("PostgreSQL socket is unsafe"));
	}
	Ok(identity.stable)
}

fn read_postmaster_pid(path: &Path) -> Result<u32, ServiceSupervisorError> {
	let mut bytes = Vec::new();
	open_secure_regular(path, 64 * 1024)?
		.take(64 * 1024 + 1)
		.read_to_end(&mut bytes)
		.map_err(|_| ServiceSupervisorError::new("PostgreSQL generation cannot be read"))?;
	let first_line = bytes
		.split(|byte| *byte == b'\n')
		.next()
		.ok_or_else(|| ServiceSupervisorError::new("PostgreSQL generation is invalid"))?;
	std::str::from_utf8(first_line)
		.map_err(|_| ServiceSupervisorError::new("PostgreSQL generation is invalid"))?
		.parse::<u32>()
		.map_err(|_| ServiceSupervisorError::new("PostgreSQL generation is invalid"))
}

fn spawn_daemon(
	config: &ServiceSupervisorConfig,
	executable: &Path,
) -> Result<Child, ServiceSupervisorError> {
	let home = std::env::var_os("HOME")
		.filter(|value| !value.is_empty())
		.ok_or_else(|| ServiceSupervisorError::new("HOME is unavailable"))?;
	let path = std::env::var_os("PATH")
		.filter(|value| !value.is_empty())
		.unwrap_or_else(|| OsString::from("/usr/bin:/bin:/usr/sbin:/sbin"));
	Command::new(executable)
		.arg("serve")
		.current_dir(&config.working_directory)
		.env_clear()
		.env("HOME", home)
		.env("PATH", path)
		.stdin(Stdio::null())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.spawn()
		.map_err(|_| ServiceSupervisorError::new("decodexd child cannot be started"))
}

fn child_exit(child: &mut Child) -> Result<Option<ExitStatus>, ServiceSupervisorError> {
	child.try_wait().map_err(|_| ServiceSupervisorError::new("child process cannot be inspected"))
}

#[derive(Clone, Copy)]
enum ChildKind {
	Daemon,
	Postgres,
}

fn stop_child(child: &mut Child, kind: ChildKind) {
	if child.try_wait().ok().flatten().is_some() {
		return;
	}
	let _ = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
	let deadline = Instant::now() + child_shutdown_timeout(kind);
	while Instant::now() < deadline {
		match child.try_wait() {
			Ok(Some(_)) => return,
			Ok(None) => thread::sleep(Duration::from_millis(20)),
			Err(_) => break,
		}
	}
	let _ = child.kill();
	let _ = child.wait();
	let message = match kind {
		ChildKind::Daemon => "decodexd child required forced termination",
		ChildKind::Postgres => "PostgreSQL child required forced termination",
	};
	let _ = writeln!(std::io::stderr().lock(), "{message}");
}

const fn child_shutdown_timeout(kind: ChildKind) -> Duration {
	match kind {
		ChildKind::Daemon => DAEMON_SHUTDOWN_TIMEOUT,
		ChildKind::Postgres => POSTGRES_SHUTDOWN_TIMEOUT,
	}
}

pub(super) struct ServiceSupervisorError {
	message: &'static str,
}

impl ServiceSupervisorError {
	const fn new(message: &'static str) -> Self {
		Self { message }
	}
}

impl Display for ServiceSupervisorError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(self.message)
	}
}

impl std::fmt::Debug for ServiceSupervisorError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(self.message)
	}
}

impl Error for ServiceSupervisorError {}
