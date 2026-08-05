//! Descriptor-pinned Unix-socket connection authority.
//!
//! The external trust anchor is the explicit operator-configured peer UID together with a
//! directory owned by that UID and not writable by group or other principals. PostgreSQL
//! authentication starts only after the connected stream's kernel-authenticated peer UID matches
//! that anchor. The configured pathname is also rebound to retained directory and socket
//! identities before and after every connection.

use std::{
	ffi::{CStr, CString, OsStr},
	fmt::{Display, Formatter},
	fs::{File, Metadata},
	future::Future,
	io::ErrorKind,
	mem::MaybeUninit,
	os::{
		fd::{AsRawFd as _, FromRawFd as _, RawFd},
		unix::{ffi::OsStrExt as _, fs::MetadataExt as _},
	},
	path::{Component, Path, PathBuf},
	pin::Pin,
	sync::Arc,
	task::{Context, Poll},
};

use deadpool_postgres::Connect;
use libc::{
	AT_FDCWD, AT_SYMLINK_NOFOLLOW, O_CLOEXEC, O_DIRECTORY, O_NOFOLLOW, S_IFMT, S_IFSOCK, stat,
};
#[cfg(test)] use tokio::sync::Barrier;
use tokio::{
	io::{AsyncRead, AsyncWrite, ReadBuf},
	net::UnixStream,
	task::JoinHandle,
};
use tokio_postgres::{Client, Config, NoTls};

use crate::StoreError;

type ConnectFuture<'a> = Pin<
	Box<
		dyn Future<Output = std::result::Result<(Client, JoinHandle<()>), tokio_postgres::Error>>
			+ Send
			+ 'a,
	>,
>;

#[cfg(target_vendor = "apple")]
const DIRECTORY_ACCESS: libc::c_int = libc::O_SEARCH;
#[cfg(not(target_vendor = "apple"))]
const DIRECTORY_ACCESS: libc::c_int = libc::O_RDONLY;

#[derive(Clone)]
pub(crate) struct VerifiedSocketConnect {
	authority: Arc<SocketAuthority>,
}
impl VerifiedSocketConnect {
	pub(crate) fn new(
		directory: &Path,
		port: u16,
		expected_peer_uid: u32,
	) -> std::result::Result<Self, StoreError> {
		let authority =
			SocketAuthority::open(directory, port, expected_peer_uid).map_err(|error| {
				if error.kind() == ErrorKind::NotFound {
					StoreError::SocketUnavailable
				} else {
					StoreError::UnsafeHostPath
				}
			})?;

		Ok(Self { authority: Arc::new(authority) })
	}

	pub(crate) fn verify(&self) -> std::result::Result<(), StoreError> {
		self.authority.verify_path_binding().map_err(|_| StoreError::UnsafeHostPath)
	}

	#[cfg(test)]
	fn install_connect_hook(&mut self, hook: Arc<TestConnectHook>) {
		Arc::get_mut(&mut self.authority)
			.expect("fixture connector authority is not shared")
			.before_connect = Some(hook);
	}
}

impl Connect for VerifiedSocketConnect {
	fn connect(&self, config: &Config) -> ConnectFuture<'_> {
		let config = config.clone();
		let authority = Arc::clone(&self.authority);

		Box::pin(async move {
			let stream = match authority.connect().await {
				Ok(stream) => VerifiedStream::Connected(stream),
				Err(failure) => VerifiedStream::Rejected(failure),
			};
			let (client, connection) = config.connect_raw(stream, NoTls).await?;
			let task = tokio::spawn(async move {
				let _ = connection.await;
			});

			Ok((client, task))
		})
	}
}

#[derive(Debug)]
struct RejectedEndpoint(SocketConnectFailure);
impl std::error::Error for RejectedEndpoint {}

impl Display for RejectedEndpoint {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str("verified Unix socket endpoint rejected")
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SocketConnectFailure {
	UnsafeAuthority,
	Unreachable,
}

struct SocketAuthority {
	directory: File,
	directory_path: PathBuf,
	directory_identity: FileIdentity,
	endpoint_path: PathBuf,
	socket_name: CString,
	socket_identity: SocketIdentity,
	expected_peer_uid: u32,
	#[cfg(test)]
	before_connect: Option<Arc<TestConnectHook>>,
}
impl SocketAuthority {
	fn open(directory_path: &Path, port: u16, expected_peer_uid: u32) -> std::io::Result<Self> {
		let directory = open_absolute_directory(directory_path)?;
		let directory_metadata = directory.metadata()?;

		if !secure_directory(&directory_metadata, expected_peer_uid) {
			return Err(std::io::Error::new(
				ErrorKind::PermissionDenied,
				"socket directory is not owned exclusively by the configured peer authority",
			));
		}

		let directory_identity = FileIdentity::from_metadata(&directory_metadata);
		let socket_name = CString::new(format!(".s.PGSQL.{port}"))
			.map_err(|_| std::io::Error::new(ErrorKind::InvalidInput, "invalid socket name"))?;
		let socket_identity = socket_identity(&directory, &socket_name)?;

		if socket_identity.uid != expected_peer_uid {
			return Err(std::io::Error::new(
				ErrorKind::PermissionDenied,
				"socket object is not owned by the configured peer authority",
			));
		}

		let endpoint_path = directory_path.join(OsStr::from_bytes(socket_name.to_bytes()));

		Ok(Self {
			directory,
			directory_path: directory_path.to_owned(),
			directory_identity,
			endpoint_path,
			socket_name,
			socket_identity,
			expected_peer_uid,
			#[cfg(test)]
			before_connect: None,
		})
	}

	async fn connect(&self) -> Result<UnixStream, SocketConnectFailure> {
		self.verify_path_binding().map_err(|_| SocketConnectFailure::UnsafeAuthority)?;

		#[cfg(test)]
		if let Some(hook) = &self.before_connect {
			hook.reached.wait().await;
			hook.resume.wait().await;
		}

		let stream = match UnixStream::connect(&self.endpoint_path).await {
			Ok(stream) => stream,
			Err(_) if self.verify_path_binding().is_err() =>
				return Err(SocketConnectFailure::UnsafeAuthority),
			Err(_) => return Err(SocketConnectFailure::Unreachable),
		};
		let peer = stream.peer_cred().map_err(|_| SocketConnectFailure::UnsafeAuthority)?;

		if peer.uid() != self.expected_peer_uid {
			return Err(SocketConnectFailure::UnsafeAuthority);
		}

		self.verify_path_binding().map_err(|_| SocketConnectFailure::UnsafeAuthority)?;

		Ok(stream)
	}

	fn verify_path_binding(&self) -> std::io::Result<()> {
		let current = open_absolute_directory(&self.directory_path)?;
		let pinned_metadata = self.directory.metadata()?;
		let current_metadata = current.metadata()?;
		let current_identity = FileIdentity::from_metadata(&current_metadata);

		if !secure_directory(&pinned_metadata, self.expected_peer_uid)
			|| !secure_directory(&current_metadata, self.expected_peer_uid)
			|| current_identity != self.directory_identity
			|| socket_identity(&self.directory, &self.socket_name)? != self.socket_identity
			|| socket_identity(&current, &self.socket_name)? != self.socket_identity
		{
			return Err(std::io::Error::new(
				ErrorKind::PermissionDenied,
				"Unix socket path no longer names the pinned endpoint",
			));
		}

		Ok(())
	}
}

#[cfg(test)]
struct TestConnectHook {
	reached: Barrier,
	resume: Barrier,
}
#[cfg(test)]
impl TestConnectHook {
	fn new() -> Self {
		Self { reached: Barrier::new(2), resume: Barrier::new(2) }
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileIdentity {
	device: u64,
	inode: u64,
}
impl FileIdentity {
	fn from_metadata(metadata: &Metadata) -> Self {
		Self { device: metadata.dev(), inode: metadata.ino() }
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SocketIdentity {
	file: FileIdentity,
	uid: u32,
}

enum VerifiedStream {
	Connected(UnixStream),
	Rejected(SocketConnectFailure),
}
impl AsyncRead for VerifiedStream {
	fn poll_read(
		self: Pin<&mut Self>,
		context: &mut Context<'_>,
		buffer: &mut ReadBuf<'_>,
	) -> Poll<std::io::Result<()>> {
		match self.get_mut() {
			Self::Connected(stream) => Pin::new(stream).poll_read(context, buffer),
			Self::Rejected(failure) => Poll::Ready(Err(rejected_endpoint_error(*failure))),
		}
	}
}

impl AsyncWrite for VerifiedStream {
	fn poll_write(
		self: Pin<&mut Self>,
		context: &mut Context<'_>,
		buffer: &[u8],
	) -> Poll<std::io::Result<usize>> {
		match self.get_mut() {
			Self::Connected(stream) => Pin::new(stream).poll_write(context, buffer),
			Self::Rejected(failure) => Poll::Ready(Err(rejected_endpoint_error(*failure))),
		}
	}

	fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
		match self.get_mut() {
			Self::Connected(stream) => Pin::new(stream).poll_flush(context),
			Self::Rejected(failure) => Poll::Ready(Err(rejected_endpoint_error(*failure))),
		}
	}

	fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<()>> {
		match self.get_mut() {
			Self::Connected(stream) => Pin::new(stream).poll_shutdown(context),
			Self::Rejected(failure) => Poll::Ready(Err(rejected_endpoint_error(*failure))),
		}
	}
}

pub(crate) fn rejected_endpoint_failure(
	error: &tokio_postgres::Error,
) -> Option<SocketConnectFailure> {
	let mut source = std::error::Error::source(error);

	while let Some(cause) = source {
		if let Some(rejected) = cause.downcast_ref::<RejectedEndpoint>() {
			return Some(rejected.0);
		}
		if let Some(rejected) = cause
			.downcast_ref::<std::io::Error>()
			.and_then(std::io::Error::get_ref)
			.and_then(|inner| inner.downcast_ref::<RejectedEndpoint>())
		{
			return Some(rejected.0);
		}

		source = cause.source();
	}

	None
}

fn rejected_endpoint_error(failure: SocketConnectFailure) -> std::io::Error {
	std::io::Error::new(ErrorKind::PermissionDenied, RejectedEndpoint(failure))
}

fn secure_directory(metadata: &Metadata, expected_peer_uid: u32) -> bool {
	metadata.uid() == expected_peer_uid && metadata.mode() & 0o022 == 0
}

fn socket_identity(directory: &File, name: &CStr) -> std::io::Result<SocketIdentity> {
	let mut metadata = MaybeUninit::<stat>::zeroed();
	// SAFETY: the directory descriptor and NUL-terminated name remain valid for the
	// duration of `fstatat`, which initializes `metadata` on success.
	let result = unsafe {
		libc::fstatat(
			directory.as_raw_fd(),
			name.as_ptr(),
			metadata.as_mut_ptr(),
			AT_SYMLINK_NOFOLLOW,
		)
	};

	if result == -1 {
		return Err(std::io::Error::last_os_error());
	}

	// SAFETY: successful `fstatat` initialized the complete `stat` value.
	let metadata = unsafe { metadata.assume_init() };

	if metadata.st_mode & S_IFMT != S_IFSOCK {
		return Err(std::io::Error::new(ErrorKind::InvalidData, "endpoint is not a Unix socket"));
	}

	Ok(SocketIdentity {
		file: FileIdentity { device: metadata.st_dev as u64, inode: metadata.st_ino },
		uid: metadata.st_uid,
	})
}

fn open_absolute_directory(path: &Path) -> std::io::Result<File> {
	if !path.is_absolute() {
		return Err(std::io::Error::new(ErrorKind::InvalidInput, "socket path is not absolute"));
	}

	let mut directory = open_directory(AT_FDCWD, c"/")?;

	for component in path.components() {
		match component {
			Component::RootDir => {},
			Component::Normal(name) => {
				let name = CString::new(name.as_bytes()).map_err(|_| {
					std::io::Error::new(ErrorKind::InvalidInput, "socket path contains NUL")
				})?;

				directory = open_directory(directory.as_raw_fd(), &name)?;
			},
			_ => {
				return Err(std::io::Error::new(
					ErrorKind::InvalidInput,
					"socket path is not normalized",
				));
			},
		}
	}

	Ok(directory)
}

fn open_directory(parent: RawFd, name: &CStr) -> std::io::Result<File> {
	// SAFETY: `parent` is AT_FDCWD or an open directory, `name` is NUL-terminated,
	// and a successful `openat` returns a new descriptor owned by this function.
	let descriptor = unsafe {
		libc::openat(parent, name.as_ptr(), DIRECTORY_ACCESS | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
	};

	if descriptor == -1 {
		return Err(std::io::Error::last_os_error());
	}

	// SAFETY: a successful `openat` returned a fresh descriptor transferred to `File`.
	Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(test)]
mod tests {
	use std::{
		fs::{self, Permissions},
		io,
		os::unix::{fs::PermissionsExt as _, net::UnixListener},
		path::{Path, PathBuf},
		process,
		sync::{
			Arc,
			atomic::{AtomicU64, Ordering},
		},
	};

	use deadpool_postgres::Connect as _;
	use tokio_postgres::Config;

	use crate::{
		BootstrapFailure, PostgresStore, StoreError,
		socket::{SocketAuthority, SocketConnectFailure, TestConnectHook, VerifiedSocketConnect},
	};

	const PORT: u16 = 54_321;
	static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

	struct TempDir(PathBuf);
	impl TempDir {
		fn new() -> io::Result<Self> {
			let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
			let path = std::env::temp_dir().join(format!("dxs-{}-{sequence}", process::id()));

			fs::create_dir(&path)?;
			fs::set_permissions(&path, Permissions::from_mode(0o700))?;

			Ok(Self(path))
		}

		fn path(&self) -> &Path {
			&self.0
		}
	}

	impl Drop for TempDir {
		fn drop(&mut self) {
			let _ = fs::remove_dir_all(&self.0);
		}
	}

	fn effective_uid() -> u32 {
		// SAFETY: `geteuid` has no arguments, retained pointers, or failure mode.
		unsafe { libc::geteuid() }
	}

	fn bind(directory: &Path) -> UnixListener {
		UnixListener::bind(directory.join(format!(".s.PGSQL.{PORT}"))).expect("bind Unix fixture")
	}

	#[test]
	fn preexisting_socket_cannot_override_a_different_operator_uid_pin() {
		let temp = TempDir::new().expect("spoof temp");
		let base = temp.path().canonicalize().expect("canonical spoof temp");
		let listener = bind(&base);
		let untrusted_uid = effective_uid().checked_add(1).expect("fixture UID has successor");
		let error = match SocketAuthority::open(&base, PORT, untrusted_uid) {
			Err(error) => error,
			Ok(_) => panic!("preexisting socket cannot override the operator UID pin"),
		};

		assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);

		drop(listener);
	}

	#[test]
	fn same_uid_preexisting_socket_in_world_writable_directory_is_rejected() {
		let temp = TempDir::new().expect("writable spoof temp");
		let base = temp.path().canonicalize().expect("canonical writable spoof temp");
		let listener = bind(&base);

		fs::set_permissions(&base, Permissions::from_mode(0o777))
			.expect("make configured directory world-writable");

		let error = match SocketAuthority::open(&base, PORT, effective_uid()) {
			Err(error) => error,
			Ok(_) => panic!("same-UID socket in an untrusted-writable directory must be rejected"),
		};

		assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);

		drop(listener);
	}

	#[tokio::test]
	async fn pinned_endpoint_accepts_its_kernel_authenticated_peer() {
		let temp = TempDir::new().expect("socket temp");
		let base = temp.path().canonicalize().expect("canonical socket temp");
		let listener = bind(&base);
		let authority =
			SocketAuthority::open(&base, PORT, effective_uid()).expect("pin socket authority");
		let stream = authority.connect().await.expect("connect pinned socket");
		let _accepted = listener.accept().expect("accept pinned client");

		drop(stream);
	}

	#[tokio::test]
	async fn replacing_an_ancestor_cannot_redirect_the_pinned_endpoint() {
		let temp = TempDir::new().expect("ancestor temp");
		let base = temp.path().canonicalize().expect("canonical ancestor temp");
		let configured = base.join("configured");

		fs::create_dir(&configured).expect("configured directory");

		let original_listener = bind(&configured);
		let authority = SocketAuthority::open(&configured, PORT, effective_uid())
			.expect("pin socket authority");
		let pinned = base.join("pinned");

		fs::rename(&configured, &pinned).expect("move pinned directory");
		fs::create_dir(&configured).expect("replacement directory");

		let replacement_listener = bind(&configured);
		let error = authority.connect().await.expect_err("replacement ancestor is rejected");

		assert_eq!(error, SocketConnectFailure::UnsafeAuthority);

		drop((original_listener, replacement_listener));
	}

	#[tokio::test]
	async fn replacing_the_socket_object_cannot_redirect_the_pinned_endpoint() {
		let temp = TempDir::new().expect("endpoint temp");
		let base = temp.path().canonicalize().expect("canonical endpoint temp");
		let endpoint = base.join(format!(".s.PGSQL.{PORT}"));
		let original_listener = bind(&base);
		let authority =
			SocketAuthority::open(&base, PORT, effective_uid()).expect("pin socket authority");

		fs::remove_file(&endpoint).expect("unlink original endpoint");

		let replacement_listener = bind(&base);
		let error = authority.connect().await.expect_err("replacement endpoint is rejected");

		assert_eq!(error, SocketConnectFailure::UnsafeAuthority);

		drop((original_listener, replacement_listener));
	}

	#[tokio::test]
	async fn rejected_connections_carry_attempt_local_unsafe_path_classification() {
		let temp = TempDir::new().expect("classification temp");
		let base = temp.path().canonicalize().expect("canonical classification temp");
		let endpoint = base.join(format!(".s.PGSQL.{PORT}"));
		let original_listener = bind(&base);
		let connector = VerifiedSocketConnect::new(&base, PORT, effective_uid())
			.expect("pin classified socket authority");

		fs::remove_file(&endpoint).expect("unlink classified endpoint");

		let replacement_listener = bind(&base);
		let mut config = Config::new();

		config.user("fixture");

		let error = match connector.connect(&config).await {
			Err(error) => error,
			Ok(_) => panic!("replacement endpoint cannot authenticate"),
		};

		assert_eq!(
			super::rejected_endpoint_failure(&error),
			Some(SocketConnectFailure::UnsafeAuthority),
			"{error:?}"
		);
		assert_eq!(
			StoreError::Pool(deadpool_postgres::PoolError::Backend(error)).bootstrap_failure(),
			BootstrapFailure::UnsafeHostPath
		);

		drop((original_listener, replacement_listener));
	}

	#[tokio::test]
	async fn replacement_after_precheck_is_attempt_local_unsafe_path() {
		let temp = TempDir::new().expect("connect race temp");
		let base = temp.path().canonicalize().expect("canonical connect race temp");
		let endpoint = base.join(format!(".s.PGSQL.{PORT}"));
		let original_listener = bind(&base);
		let hook = Arc::new(TestConnectHook::new());
		let mut connector = VerifiedSocketConnect::new(&base, PORT, effective_uid())
			.expect("pin raced socket authority");

		connector.install_connect_hook(Arc::clone(&hook));

		let mut config = Config::new();

		config.user("fixture");

		let attempt = tokio::spawn(async move { connector.connect(&config).await });

		hook.reached.wait().await;

		fs::remove_file(&endpoint).expect("unlink endpoint after verified precheck");

		let replacement = bind(&base);

		drop(replacement);

		hook.resume.wait().await;

		let error = match attempt.await.expect("join raced connection attempt") {
			Err(error) => error,
			Ok(_) => panic!("replacement endpoint cannot authenticate"),
		};

		assert_eq!(
			super::rejected_endpoint_failure(&error),
			Some(SocketConnectFailure::UnsafeAuthority),
			"{error:?}"
		);
		assert_eq!(
			StoreError::Pool(deadpool_postgres::PoolError::Backend(error)).bootstrap_failure(),
			BootstrapFailure::UnsafeHostPath
		);

		drop(original_listener);
	}

	#[tokio::test]
	async fn secure_stale_socket_is_typed_unreachable_through_store_bootstrap() {
		let temp = TempDir::new().expect("stale socket temp");
		let base = temp.path().canonicalize().expect("canonical stale socket temp");
		let listener = bind(&base);

		drop(listener);

		let mut runtime = Config::new();

		runtime.host_path(&base).port(PORT).dbname("fixture").user("fixture_runtime");

		let error = match PostgresStore::connect_runtime(runtime, effective_uid()).await {
			Err(error) => error,
			Ok(_) => panic!("stale socket cannot bootstrap a PostgreSQL store"),
		};

		assert_eq!(error.bootstrap_failure(), BootstrapFailure::Unreachable);
	}
}
