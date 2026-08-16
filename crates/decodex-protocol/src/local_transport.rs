//! Same-effective-UID local transport authority.
//!
//! This module owns the one production local endpoint. The endpoint has a fixed
//! name below the typed Decodex server directory. A persistent, single-link file
//! carries an exclusive kernel lock from stale-endpoint inspection through final
//! cleanup. Publication binds the fixed staging name and uses descriptor-relative
//! `renameat` in the same directory. The authority validates the current namespace
//! at publication, admission, reconnect, and cleanup. Each accepted staging or
//! canonical socket identity has exactly one link.
//!
//! The lock serializes cooperating Decodex daemons. This boundary does not claim
//! confinement against hostile code that already runs with the service-owner UID.

use std::{
	fmt::{Debug, Display, Formatter},
	path::{Path, PathBuf},
};
#[cfg(target_os = "macos")] use std::{fs::File, os::fd::RawFd};

use decodex_core::{DecodexPaths, LocalTrustPolicy};

/// One kernel-authenticated local byte stream on a supported Unix host.
#[cfg(unix)]
pub type LocalTransportStream = tokio::net::UnixStream;

/// Uninhabited local stream facade for unsupported build targets.
#[cfg(not(unix))]
pub struct LocalTransportStream {
	_private: (),
}

#[cfg(not(unix))]
impl tokio::io::AsyncRead for LocalTransportStream {
	fn poll_read(
		self: std::pin::Pin<&mut Self>,
		_context: &mut std::task::Context<'_>,
		_buffer: &mut tokio::io::ReadBuf<'_>,
	) -> std::task::Poll<std::io::Result<()>> {
		std::task::Poll::Ready(Err(std::io::Error::new(
			std::io::ErrorKind::Unsupported,
			"local transport is unsupported on this platform",
		)))
	}
}

#[cfg(not(unix))]
impl tokio::io::AsyncWrite for LocalTransportStream {
	fn poll_write(
		self: std::pin::Pin<&mut Self>,
		_context: &mut std::task::Context<'_>,
		_buffer: &[u8],
	) -> std::task::Poll<std::io::Result<usize>> {
		std::task::Poll::Ready(Err(std::io::Error::new(
			std::io::ErrorKind::Unsupported,
			"local transport is unsupported on this platform",
		)))
	}

	fn poll_flush(
		self: std::pin::Pin<&mut Self>,
		_context: &mut std::task::Context<'_>,
	) -> std::task::Poll<std::io::Result<()>> {
		std::task::Poll::Ready(Err(std::io::Error::new(
			std::io::ErrorKind::Unsupported,
			"local transport is unsupported on this platform",
		)))
	}

	fn poll_shutdown(
		self: std::pin::Pin<&mut Self>,
		_context: &mut std::task::Context<'_>,
	) -> std::task::Poll<std::io::Result<()>> {
		std::task::Poll::Ready(Err(std::io::Error::new(
			std::io::ErrorKind::Unsupported,
			"local transport is unsupported on this platform",
		)))
	}
}

/// The complete V2.4 local endpoint authority.
#[derive(Clone, Eq, PartialEq)]
pub struct LocalTransportAuthority {
	paths: DecodexPaths,
	endpoint_path: PathBuf,
	policy: LocalTrustPolicy,
	service_owner_uid: u32,
}

impl LocalTransportAuthority {
	/// Resolve one enabled same-UID authority from the durable host policy.
	pub fn new(
		paths: DecodexPaths,
		policy: LocalTrustPolicy,
		service_owner_uid: Option<u32>,
	) -> Result<Self, LocalTransportRefusal> {
		let service_owner_uid = match (policy, service_owner_uid) {
			(LocalTrustPolicy::Disabled, None) => return Err(LocalTransportRefusal::Disabled),
			(LocalTrustPolicy::SameUid, Some(uid)) => uid,
			_ => return Err(LocalTransportRefusal::InvalidPolicy),
		};

		if !cfg!(any(target_os = "linux", target_os = "macos")) {
			return Err(LocalTransportRefusal::UnsupportedPlatform);
		}

		let authority = Self {
			endpoint_path: paths.local_transport_socket(),
			paths,
			policy,
			service_owner_uid,
		};

		#[cfg(any(target_os = "linux", target_os = "macos"))]
		authority.verify_process_owner()?;

		Ok(authority)
	}

	/// Bind and atomically publish the fixed owner-only endpoint.
	pub async fn bind(&self) -> Result<LocalTransportListener, LocalTransportRefusal> {
		#[cfg(any(target_os = "linux", target_os = "macos"))]
		{
			platform::bind(self).await
		}

		#[cfg(not(any(target_os = "linux", target_os = "macos")))]
		{
			Err(LocalTransportRefusal::UnsupportedPlatform)
		}
	}

	/// Connect after current endpoint identity and kernel server-peer validation.
	///
	/// Each call captures and validates the current publication. No endpoint or
	/// peer observation is reused across reconnects.
	pub async fn connect(&self) -> Result<LocalTransportStream, LocalTransportRefusal> {
		#[cfg(any(target_os = "linux", target_os = "macos"))]
		{
			platform::connect(self).await
		}

		#[cfg(not(any(target_os = "linux", target_os = "macos")))]
		{
			Err(LocalTransportRefusal::UnsupportedPlatform)
		}
	}

	/// Validate and retain one installer-shaped inherited descriptor without acquiring a lock.
	#[cfg(target_os = "macos")]
	#[doc(hidden)]
	pub fn validate_installer_namespace_lock_fd(
		&self,
		raw_fd: RawFd,
	) -> Result<File, LocalTransportRefusal> {
		self.verify_process_owner()?;
		platform::validate_installer_namespace_lock_fd(self, raw_fd)
	}

	fn endpoint_path(&self) -> &Path {
		&self.endpoint_path
	}

	#[cfg(any(target_os = "linux", target_os = "macos"))]
	fn verify_process_owner(&self) -> Result<(), LocalTransportRefusal> {
		if platform::effective_user_id() == self.service_owner_uid {
			Ok(())
		} else {
			Err(LocalTransportRefusal::EffectiveUidMismatch)
		}
	}
}

impl Debug for LocalTransportAuthority {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("LocalTransportAuthority")
			.field("policy", &self.policy)
			.field("endpoint", &"<redacted>")
			.field("service_owner_uid", &"<configured>")
			.finish()
	}
}

/// The non-cloneable singleton daemon capability.
///
/// It owns the published listener, retained directory identity, and one lifetime
/// namespace lock from acquisition through release-last cleanup.
pub struct LocalTransportListener {
	authority: LocalTransportAuthority,
	#[cfg(any(target_os = "linux", target_os = "macos"))]
	listener: Option<tokio::net::UnixListener>,
	#[cfg(any(target_os = "linux", target_os = "macos"))]
	binding: Option<platform::EndpointBinding>,
	#[cfg(any(target_os = "linux", target_os = "macos"))]
	namespace_lock: Option<platform::NamespaceLock>,
}

impl LocalTransportListener {
	/// Accept one stream after point-in-time namespace and peer validation.
	///
	/// A peer-credential refusal applies only to this connection. A namespace or
	/// listener refusal invalidates the published listener.
	pub async fn accept(&mut self) -> Result<LocalTransportStream, LocalTransportRefusal> {
		#[cfg(any(target_os = "linux", target_os = "macos"))]
		{
			let listener =
				self.listener.as_mut().ok_or(LocalTransportRefusal::EndpointUnavailable)?;
			let (stream, _) =
				listener.accept().await.map_err(|_| LocalTransportRefusal::EndpointUnavailable)?;
			let peer = stream
				.peer_cred()
				.map_err(|_| LocalTransportRefusal::PeerCredentialsUnavailable)?;

			if peer.uid() != self.authority.service_owner_uid {
				return Err(LocalTransportRefusal::PeerUidMismatch);
			}

			self.revalidate()?;

			Ok(stream)
		}

		#[cfg(not(any(target_os = "linux", target_os = "macos")))]
		{
			Err(LocalTransportRefusal::UnsupportedPlatform)
		}
	}

	/// Validate the current published path, retained directory, and namespace lock.
	pub fn revalidate(&self) -> Result<(), LocalTransportRefusal> {
		#[cfg(any(target_os = "linux", target_os = "macos"))]
		{
			let listener =
				self.listener.as_ref().ok_or(LocalTransportRefusal::EndpointUnavailable)?;
			let binding =
				self.binding.as_ref().ok_or(LocalTransportRefusal::EndpointUnavailable)?;
			let namespace_lock =
				self.namespace_lock.as_ref().ok_or(LocalTransportRefusal::EndpointUnavailable)?;

			platform::revalidate_listener(&self.authority, listener, binding, namespace_lock)
		}

		#[cfg(not(any(target_os = "linux", target_os = "macos")))]
		{
			Err(LocalTransportRefusal::UnsupportedPlatform)
		}
	}

	/// Remove only the retained publication, close the listener, and release the
	/// namespace lock last.
	///
	/// The runtime calls this only after it has harvested every owned task.
	pub fn cleanup(mut self) -> Result<(), LocalTransportRefusal> {
		self.release(true)
	}

	#[cfg(any(target_os = "linux", target_os = "macos"))]
	fn release(&mut self, report: bool) -> Result<(), LocalTransportRefusal> {
		let result =
			match (self.listener.as_ref(), self.binding.as_ref(), self.namespace_lock.as_ref()) {
				(Some(listener), Some(binding), Some(namespace_lock)) =>
					platform::remove_publication(&self.authority, listener, binding, namespace_lock),
				_ => Err(LocalTransportRefusal::EndpointUnavailable),
			};

		drop(self.listener.take());
		drop(self.binding.take());
		drop(self.namespace_lock.take());

		if report { result } else { Ok(()) }
	}

	#[cfg(not(any(target_os = "linux", target_os = "macos")))]
	fn release(&mut self, _report: bool) -> Result<(), LocalTransportRefusal> {
		Err(LocalTransportRefusal::UnsupportedPlatform)
	}
}

impl Debug for LocalTransportListener {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("LocalTransportListener")
			.field("endpoint", &"<redacted>")
			.field("same_uid", &true)
			.finish()
	}
}

impl Drop for LocalTransportListener {
	fn drop(&mut self) {
		let _ = self.release(false);
	}
}

/// Closed local admission and endpoint-integrity refusal classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalTransportRefusal {
	/// Durable host policy disables local admission.
	Disabled,
	/// Policy and configured service-owner UID do not form one valid state.
	InvalidPolicy,
	/// No validated host configuration supplied local transport authority.
	ConfigurationUnavailable,
	/// The build target has no accepted local peer-credential implementation.
	UnsupportedPlatform,
	/// The process effective UID differs from the configured service owner.
	EffectiveUidMismatch,
	/// The endpoint directory is missing, linked, replaced, wrongly owned, or wrongly scoped.
	UnsafeDirectory,
	/// A socket or namespace-lock entry has an unsafe type, owner, mode, or link count.
	UnsafeEndpoint,
	/// No current endpoint can be reached or created.
	EndpointUnavailable,
	/// Another daemon holds the namespace or one observed endpoint may still be live.
	EndpointInUse,
	/// A retained directory, lock, or socket identity changed or became ambiguous.
	EndpointReplaced,
	/// The kernel did not provide an unambiguous peer credential.
	PeerCredentialsUnavailable,
	/// The kernel-authenticated peer UID differs from the service owner.
	PeerUidMismatch,
}

impl LocalTransportRefusal {
	/// Whether a bound service must stop accepting later connections.
	pub const fn invalidates_listener(self) -> bool {
		!matches!(self, Self::PeerCredentialsUnavailable | Self::PeerUidMismatch)
	}
}

impl Display for LocalTransportRefusal {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::Disabled => "local transport is disabled",
			Self::InvalidPolicy => "local transport policy is invalid",
			Self::ConfigurationUnavailable => "local transport configuration is unavailable",
			Self::UnsupportedPlatform => "local peer identity is unsupported on this platform",
			Self::EffectiveUidMismatch =>
				"process effective UID does not match the local service owner",
			Self::UnsafeDirectory => "local endpoint directory is unsafe",
			Self::UnsafeEndpoint => "local endpoint or namespace lock is unsafe",
			Self::EndpointUnavailable => "local endpoint is unavailable",
			Self::EndpointInUse => "local endpoint namespace may have a live daemon",
			Self::EndpointReplaced => "local endpoint identity was replaced or became ambiguous",
			Self::PeerCredentialsUnavailable => "kernel peer identity is unavailable",
			Self::PeerUidMismatch => "kernel peer UID does not match the local service owner",
		})
	}
}

impl std::error::Error for LocalTransportRefusal {}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod platform {
	use std::{
		ffi::{CStr, CString},
		fs::{File, Metadata},
		io::{self, ErrorKind},
		mem::MaybeUninit,
		os::{
			fd::{AsRawFd as _, FromRawFd as _, RawFd},
			unix::{ffi::OsStrExt as _, fs::MetadataExt as _},
		},
		path::{Component, Path, PathBuf},
		time::Duration,
	};

	use libc::{
		AT_FDCWD, AT_SYMLINK_NOFOLLOW, LOCK_EX, LOCK_NB, O_CLOEXEC, O_CREAT, O_DIRECTORY, O_EXCL,
		O_NOFOLLOW, O_RDWR, S_IFMT, S_IFREG, S_IFSOCK, mode_t, sockaddr_un, stat,
	};
	use tokio::{
		net::{UnixListener, UnixStream},
		time,
	};

	use super::{
		LocalTransportAuthority, LocalTransportListener, LocalTransportRefusal,
		LocalTransportStream,
	};

	const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
	const PRIVATE_FILE_MODE: u32 = 0o600;
	const STALE_PROBE_TIMEOUT: Duration = Duration::from_millis(250);
	const CANONICAL_NAME: &CStr = c"decodex.sock";
	const STAGE_NAME: &CStr = c"decodex.sock.stage";
	const NAMESPACE_LOCK_NAME: &CStr = c"decodex.lock";
	#[cfg(target_vendor = "apple")]
	const DIRECTORY_ACCESS: libc::c_int = libc::O_SEARCH;
	#[cfg(not(target_vendor = "apple"))]
	const DIRECTORY_ACCESS: libc::c_int = libc::O_RDONLY;

	pub(super) async fn bind(
		authority: &LocalTransportAuthority,
	) -> Result<LocalTransportListener, LocalTransportRefusal> {
		authority.verify_process_owner()?;

		let stage_path = stage_path(authority.endpoint_path())?;

		if !endpoint_path_fits(authority.endpoint_path()) || !endpoint_path_fits(&stage_path) {
			return Err(LocalTransportRefusal::UnsafeEndpoint);
		}

		authority
			.paths
			.ensure_local_transport_layout()
			.map_err(|_| LocalTransportRefusal::UnsafeDirectory)?;
		let directory =
			DirectoryBinding::open(authority.endpoint_path(), authority.service_owner_uid)?;
		let namespace_lock = NamespaceLock::acquire(&directory)?;

		recover_name(authority, &directory, &namespace_lock, STAGE_NAME, &stage_path).await?;
		recover_name(
			authority,
			&directory,
			&namespace_lock,
			CANONICAL_NAME,
			authority.endpoint_path(),
		)
		.await?;

		authority.verify_process_owner()?;
		directory.verify_namespace_lock(&namespace_lock)?;
		directory.verify_absent(STAGE_NAME)?;
		directory.verify_absent(CANONICAL_NAME)?;

		let listener = UnixListener::bind(&stage_path)
			.map_err(|_| LocalTransportRefusal::EndpointUnavailable)?;
		let initial = directory
			.socket_identity(STAGE_NAME)
			.map_err(|_| LocalTransportRefusal::EndpointReplaced)?;

		if initial.uid != authority.service_owner_uid || initial.links != 1 {
			drop(listener);

			return Err(LocalTransportRefusal::UnsafeEndpoint);
		}

		if chmod_socket(&directory, STAGE_NAME, PRIVATE_FILE_MODE).is_err() {
			directory.remove_if_file_identity(&namespace_lock, STAGE_NAME, initial);
			drop(listener);

			return Err(LocalTransportRefusal::UnsafeEndpoint);
		}

		let identity = match directory.socket_identity(STAGE_NAME) {
			Ok(identity)
				if identity.file == initial.file
					&& identity.links == initial.links
					&& secure_socket(identity, authority.service_owner_uid) =>
				identity,
			_ => {
				directory.remove_if_file_identity(&namespace_lock, STAGE_NAME, initial);
				drop(listener);

				return Err(LocalTransportRefusal::EndpointReplaced);
			},
		};
		let local_path = match listener.local_addr() {
			Ok(address) => address.as_pathname().map(Path::to_owned),
			Err(_) => {
				directory.remove_if_identity(&namespace_lock, STAGE_NAME, identity);
				drop(listener);

				return Err(LocalTransportRefusal::EndpointUnavailable);
			},
		};

		if local_path.as_deref() != Some(stage_path.as_path()) {
			directory.remove_if_file_identity(&namespace_lock, STAGE_NAME, identity);
			drop(listener);

			return Err(LocalTransportRefusal::EndpointReplaced);
		}

		PendingPublication {
			authority,
			directory: Some(directory),
			namespace_lock: Some(namespace_lock),
			listener: Some(listener),
			identity,
			stage_path,
			published: false,
		}
		.publish()
	}

	async fn recover_name(
		authority: &LocalTransportAuthority,
		directory: &DirectoryBinding,
		namespace_lock: &NamespaceLock,
		name: &CStr,
		path: &Path,
	) -> Result<(), LocalTransportRefusal> {
		let identity = match directory.socket_identity(name) {
			Ok(identity) => identity,
			Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
			Err(_) => return Err(LocalTransportRefusal::UnsafeEndpoint),
		};

		if !secure_socket(identity, authority.service_owner_uid) {
			return Err(LocalTransportRefusal::UnsafeEndpoint);
		}

		directory.verify_socket_while_locked(namespace_lock, name, identity)?;

		match time::timeout(STALE_PROBE_TIMEOUT, UnixStream::connect(path)).await {
			Ok(Ok(_)) | Err(_) => Err(LocalTransportRefusal::EndpointInUse),
			Ok(Err(error)) if error.kind() == ErrorKind::ConnectionRefused => {
				authority.verify_process_owner()?;
				directory.verify_socket_while_locked(namespace_lock, name, identity)?;
				directory.unlink_socket_while_locked(namespace_lock, name, identity)
			},
			Ok(Err(_)) => {
				if directory.verify_socket_while_locked(namespace_lock, name, identity).is_err() {
					Err(LocalTransportRefusal::EndpointReplaced)
				} else {
					Err(LocalTransportRefusal::EndpointUnavailable)
				}
			},
		}
	}

	struct PendingPublication<'a> {
		authority: &'a LocalTransportAuthority,
		directory: Option<DirectoryBinding>,
		namespace_lock: Option<NamespaceLock>,
		listener: Option<UnixListener>,
		identity: SocketIdentity,
		stage_path: PathBuf,
		published: bool,
	}

	impl PendingPublication<'_> {
		fn publish(mut self) -> Result<LocalTransportListener, LocalTransportRefusal> {
			let directory =
				self.directory.as_ref().ok_or(LocalTransportRefusal::EndpointUnavailable)?;
			let namespace_lock =
				self.namespace_lock.as_ref().ok_or(LocalTransportRefusal::EndpointUnavailable)?;
			let listener =
				self.listener.as_ref().ok_or(LocalTransportRefusal::EndpointUnavailable)?;

			self.authority.verify_process_owner()?;
			directory.verify_namespace_lock(namespace_lock)?;
			directory.verify_socket_while_locked(namespace_lock, STAGE_NAME, self.identity)?;
			directory.verify_absent(CANONICAL_NAME)?;

			let local_path = listener
				.local_addr()
				.map_err(|_| LocalTransportRefusal::EndpointUnavailable)?
				.as_pathname()
				.map(Path::to_owned);

			if local_path.as_deref() != Some(self.stage_path.as_path()) {
				return Err(LocalTransportRefusal::EndpointReplaced);
			}

			directory.rename_stage_to_canonical(
				namespace_lock,
				self.identity,
				&mut self.published,
			)?;
			directory.verify_namespace_lock(namespace_lock)?;
			directory.verify_absent(STAGE_NAME)?;
			directory.verify_socket_while_locked(namespace_lock, CANONICAL_NAME, self.identity)?;
			self.authority.verify_process_owner()?;

			let binding = EndpointBinding {
				directory: self
					.directory
					.take()
					.ok_or(LocalTransportRefusal::EndpointUnavailable)?,
				identity: self.identity,
				stage_path: self.stage_path.clone(),
			};

			Ok(LocalTransportListener {
				authority: self.authority.clone(),
				listener: self.listener.take(),
				binding: Some(binding),
				namespace_lock: self.namespace_lock.take(),
			})
		}
	}

	impl Drop for PendingPublication<'_> {
		fn drop(&mut self) {
			if let (Some(directory), Some(namespace_lock)) =
				(self.directory.as_ref(), self.namespace_lock.as_ref())
			{
				let name = if self.published { CANONICAL_NAME } else { STAGE_NAME };

				directory.remove_if_identity(namespace_lock, name, self.identity);
			}

			drop(self.listener.take());
			drop(self.directory.take());
			drop(self.namespace_lock.take());
		}
	}

	pub(super) async fn connect(
		authority: &LocalTransportAuthority,
	) -> Result<LocalTransportStream, LocalTransportRefusal> {
		authority.verify_process_owner()?;

		if !endpoint_path_fits(authority.endpoint_path()) {
			return Err(LocalTransportRefusal::UnsafeEndpoint);
		}

		let directory =
			DirectoryBinding::open(authority.endpoint_path(), authority.service_owner_uid)?;
		let identity = directory.socket_identity(CANONICAL_NAME).map_err(map_initial_endpoint)?;

		if !secure_socket(identity, authority.service_owner_uid) {
			return Err(LocalTransportRefusal::UnsafeEndpoint);
		}

		directory.verify_socket(CANONICAL_NAME, identity)?;

		let stream = match UnixStream::connect(authority.endpoint_path()).await {
			Ok(stream) => stream,
			Err(_) if directory.verify_socket(CANONICAL_NAME, identity).is_err() => {
				return Err(LocalTransportRefusal::EndpointReplaced);
			},
			Err(_) => return Err(LocalTransportRefusal::EndpointUnavailable),
		};
		let peer =
			stream.peer_cred().map_err(|_| LocalTransportRefusal::PeerCredentialsUnavailable)?;

		if peer.uid() != authority.service_owner_uid {
			return Err(LocalTransportRefusal::PeerUidMismatch);
		}

		directory.verify_socket(CANONICAL_NAME, identity)?;
		authority.verify_process_owner()?;

		Ok(stream)
	}

	pub(super) fn revalidate_listener(
		authority: &LocalTransportAuthority,
		listener: &UnixListener,
		binding: &EndpointBinding,
		namespace_lock: &NamespaceLock,
	) -> Result<(), LocalTransportRefusal> {
		authority.verify_process_owner()?;
		binding.directory.verify_socket_while_locked(
			namespace_lock,
			CANONICAL_NAME,
			binding.identity,
		)?;

		let local_path = listener
			.local_addr()
			.map_err(|_| LocalTransportRefusal::EndpointUnavailable)?
			.as_pathname()
			.map(Path::to_owned);

		if local_path.as_deref() != Some(binding.stage_path.as_path()) {
			return Err(LocalTransportRefusal::EndpointReplaced);
		}
		if listener.take_error().map_err(|_| LocalTransportRefusal::EndpointUnavailable)?.is_some()
		{
			return Err(LocalTransportRefusal::EndpointUnavailable);
		}

		Ok(())
	}

	#[cfg(target_os = "macos")]
	pub(super) fn validate_installer_namespace_lock_fd(
		authority: &LocalTransportAuthority,
		raw_fd: RawFd,
	) -> Result<File, LocalTransportRefusal> {
		if raw_fd < 3 {
			return Err(LocalTransportRefusal::UnsafeEndpoint);
		}
		// SAFETY: `F_GETFD` reads descriptor flags and retains no process memory pointer.
		let inherited_flags = unsafe { libc::fcntl(raw_fd, libc::F_GETFD) };
		if inherited_flags == -1 {
			return Err(LocalTransportRefusal::UnsafeEndpoint);
		}
		// SAFETY: `F_GETFD` proved that this process owns an open descriptor. Ownership
		// transfers to the returned file and the installer retains its original duplicate.
		let file = unsafe { File::from_raw_fd(raw_fd) };
		let directory =
			DirectoryBinding::open(authority.endpoint_path(), authority.service_owner_uid)?;
		let metadata = file.metadata().map_err(|_| LocalTransportRefusal::UnsafeEndpoint)?;
		let identity = LockIdentity::from_metadata(&metadata);
		if !secure_namespace_lock_metadata(&metadata, authority.service_owner_uid) {
			return Err(LocalTransportRefusal::UnsafeEndpoint);
		}
		directory.verify_namespace_lock_file(&file, identity)?;
		// SAFETY: the owned descriptor remains open and `F_GETFD` retains no pointer.
		let descriptor_flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
		if descriptor_flags == -1 {
			return Err(LocalTransportRefusal::EndpointUnavailable);
		}
		// SAFETY: the owned descriptor remains open and the integer flags are valid.
		let close_on_exec = unsafe {
			libc::fcntl(file.as_raw_fd(), libc::F_SETFD, descriptor_flags | libc::FD_CLOEXEC)
		};
		if close_on_exec == -1 {
			return Err(LocalTransportRefusal::EndpointUnavailable);
		}
		// SAFETY: `F_GETFD` reads back the flags of the still-owned descriptor.
		let applied_flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFD) };
		if applied_flags == -1 || applied_flags & libc::FD_CLOEXEC == 0 {
			return Err(LocalTransportRefusal::EndpointUnavailable);
		}
		Ok(file)
	}

	pub(super) fn remove_publication(
		authority: &LocalTransportAuthority,
		listener: &UnixListener,
		binding: &EndpointBinding,
		namespace_lock: &NamespaceLock,
	) -> Result<(), LocalTransportRefusal> {
		revalidate_listener(authority, listener, binding, namespace_lock)?;
		binding.directory.unlink_socket_while_locked(
			namespace_lock,
			CANONICAL_NAME,
			binding.identity,
		)
	}

	pub(super) struct EndpointBinding {
		directory: DirectoryBinding,
		identity: SocketIdentity,
		stage_path: PathBuf,
	}

	pub(super) struct NamespaceLock {
		file: File,
		identity: LockIdentity,
	}

	impl NamespaceLock {
		fn acquire(directory: &DirectoryBinding) -> Result<Self, LocalTransportRefusal> {
			directory.verify()?;

			let (file, created) =
				open_namespace_lock(&directory.directory).map_err(map_lock_open)?;

			if created {
				// SAFETY: `file` owns this descriptor and `fchmod` retains no pointer.
				if unsafe { libc::fchmod(file.as_raw_fd(), PRIVATE_FILE_MODE as mode_t) } == -1 {
					return Err(LocalTransportRefusal::UnsafeEndpoint);
				}
			}

			let metadata = file.metadata().map_err(|_| LocalTransportRefusal::UnsafeEndpoint)?;

			if !secure_namespace_lock_metadata(&metadata, directory.expected_uid) {
				return Err(LocalTransportRefusal::UnsafeEndpoint);
			}

			// SAFETY: the descriptor stays open for the listener lifetime.
			let lock_result = unsafe { libc::flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) };

			if lock_result == -1 {
				return if io::Error::last_os_error().kind() == ErrorKind::WouldBlock {
					Err(LocalTransportRefusal::EndpointInUse)
				} else {
					Err(LocalTransportRefusal::EndpointUnavailable)
				};
			}

			let namespace_lock = Self { identity: LockIdentity::from_metadata(&metadata), file };

			directory.verify_namespace_lock(&namespace_lock)?;

			Ok(namespace_lock)
		}
	}

	struct DirectoryBinding {
		directory: File,
		path: PathBuf,
		identity: FileIdentity,
		expected_uid: u32,
	}

	impl DirectoryBinding {
		fn open(endpoint_path: &Path, expected_uid: u32) -> Result<Self, LocalTransportRefusal> {
			let path = endpoint_path.parent().ok_or(LocalTransportRefusal::UnsafeDirectory)?;
			let directory = open_absolute_directory(path)
				.map_err(|_| LocalTransportRefusal::UnsafeDirectory)?;
			let metadata =
				directory.metadata().map_err(|_| LocalTransportRefusal::UnsafeDirectory)?;

			if !secure_directory(&metadata, expected_uid) {
				return Err(LocalTransportRefusal::UnsafeDirectory);
			}

			Ok(Self {
				directory,
				path: path.to_owned(),
				identity: FileIdentity::from_metadata(&metadata),
				expected_uid,
			})
		}

		fn verify(&self) -> Result<File, LocalTransportRefusal> {
			let pinned =
				self.directory.metadata().map_err(|_| LocalTransportRefusal::EndpointReplaced)?;
			let current = open_absolute_directory(&self.path)
				.map_err(|_| LocalTransportRefusal::EndpointReplaced)?;
			let current_metadata =
				current.metadata().map_err(|_| LocalTransportRefusal::EndpointReplaced)?;

			if !secure_directory(&pinned, self.expected_uid)
				|| !secure_directory(&current_metadata, self.expected_uid)
				|| FileIdentity::from_metadata(&pinned) != self.identity
				|| FileIdentity::from_metadata(&current_metadata) != self.identity
			{
				return Err(LocalTransportRefusal::EndpointReplaced);
			}

			Ok(current)
		}

		fn socket_identity(&self, name: &CStr) -> io::Result<SocketIdentity> {
			socket_identity(&self.directory, name)
		}

		fn verify_absent(&self, name: &CStr) -> Result<(), LocalTransportRefusal> {
			let current = self.verify()?;
			let pinned = socket_entry_absent(&self.directory, name);
			let reopened = socket_entry_absent(&current, name);

			if pinned && reopened { Ok(()) } else { Err(LocalTransportRefusal::EndpointReplaced) }
		}

		fn verify_socket(
			&self,
			name: &CStr,
			expected: SocketIdentity,
		) -> Result<(), LocalTransportRefusal> {
			let current_directory = self.verify()?;
			let pinned = socket_identity(&self.directory, name)
				.map_err(|_| LocalTransportRefusal::EndpointReplaced)?;
			let current = socket_identity(&current_directory, name)
				.map_err(|_| LocalTransportRefusal::EndpointReplaced)?;

			if pinned != expected
				|| current != expected
				|| !secure_socket(pinned, self.expected_uid)
			{
				return Err(LocalTransportRefusal::EndpointReplaced);
			}

			Ok(())
		}

		fn verify_namespace_lock(
			&self,
			namespace_lock: &NamespaceLock,
		) -> Result<(), LocalTransportRefusal> {
			self.verify_namespace_lock_file(&namespace_lock.file, namespace_lock.identity)
		}

		fn verify_namespace_lock_file(
			&self,
			file: &File,
			expected: LockIdentity,
		) -> Result<(), LocalTransportRefusal> {
			let current_directory = self.verify()?;
			let held_metadata =
				file.metadata().map_err(|_| LocalTransportRefusal::EndpointReplaced)?;
			let held = LockIdentity::from_metadata(&held_metadata);
			let pinned = lock_identity(&self.directory, NAMESPACE_LOCK_NAME)
				.map_err(|_| LocalTransportRefusal::EndpointReplaced)?;
			let current = lock_identity(&current_directory, NAMESPACE_LOCK_NAME)
				.map_err(|_| LocalTransportRefusal::EndpointReplaced)?;

			if !secure_namespace_lock_metadata(&held_metadata, self.expected_uid)
				|| !secure_namespace_lock(pinned, self.expected_uid)
				|| !secure_namespace_lock(current, self.expected_uid)
				|| held != expected
				|| pinned != expected
				|| current != expected
			{
				return Err(LocalTransportRefusal::EndpointReplaced);
			}

			Ok(())
		}

		fn verify_socket_while_locked(
			&self,
			namespace_lock: &NamespaceLock,
			name: &CStr,
			expected: SocketIdentity,
		) -> Result<(), LocalTransportRefusal> {
			self.verify_namespace_lock(namespace_lock)?;
			self.verify_socket(name, expected)
		}

		fn unlink_socket_while_locked(
			&self,
			namespace_lock: &NamespaceLock,
			name: &CStr,
			expected: SocketIdentity,
		) -> Result<(), LocalTransportRefusal> {
			self.verify_socket_while_locked(namespace_lock, name, expected)?;

			// POSIX has no portable compare-and-unlink operation. The retained
			// namespace lock excludes cooperating daemon replacement between this
			// comparison and `unlinkat`.
			//
			// SAFETY: the directory descriptor and fixed NUL-terminated name remain valid.
			if unsafe { libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0) } == -1 {
				return Err(LocalTransportRefusal::EndpointReplaced);
			}

			self.verify_absent(name)
		}

		fn rename_stage_to_canonical(
			&self,
			namespace_lock: &NamespaceLock,
			expected: SocketIdentity,
			published: &mut bool,
		) -> Result<(), LocalTransportRefusal> {
			self.verify_socket_while_locked(namespace_lock, STAGE_NAME, expected)?;
			self.verify_absent(CANONICAL_NAME)?;

			// SAFETY: both fixed names are relative to the same retained directory.
			// `renameat` retains neither descriptor nor pointer.
			let result = unsafe {
				libc::renameat(
					self.directory.as_raw_fd(),
					STAGE_NAME.as_ptr(),
					self.directory.as_raw_fd(),
					CANONICAL_NAME.as_ptr(),
				)
			};

			if result == -1 {
				return Err(LocalTransportRefusal::EndpointReplaced);
			}

			*published = true;
			self.verify_absent(STAGE_NAME)?;
			self.verify_socket_while_locked(namespace_lock, CANONICAL_NAME, expected)
		}

		fn remove_if_identity(
			&self,
			namespace_lock: &NamespaceLock,
			name: &CStr,
			expected: SocketIdentity,
		) {
			if self.verify_socket_while_locked(namespace_lock, name, expected).is_ok() {
				// SAFETY: the retained descriptor and fixed name stay valid for this call.
				let _ = unsafe { libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0) };
			}
		}

		fn remove_if_file_identity(
			&self,
			namespace_lock: &NamespaceLock,
			name: &CStr,
			expected: SocketIdentity,
		) {
			if self.verify_namespace_lock(namespace_lock).is_ok()
				&& expected.links == 1
				&& self.socket_identity(name).is_ok_and(|identity| {
					identity.file == expected.file && identity.links == expected.links
				}) {
				// SAFETY: the retained descriptor and fixed name stay valid for this call.
				let _ = unsafe { libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0) };
			}
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
		mode: u32,
		links: u64,
	}

	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
	struct LockIdentity {
		file: FileIdentity,
		uid: u32,
		mode: u32,
		links: u64,
	}

	impl LockIdentity {
		fn from_metadata(metadata: &Metadata) -> Self {
			Self {
				file: FileIdentity::from_metadata(metadata),
				uid: metadata.uid(),
				mode: metadata.mode(),
				links: metadata.nlink(),
			}
		}
	}

	fn stage_path(canonical: &Path) -> Result<PathBuf, LocalTransportRefusal> {
		let parent = canonical.parent().ok_or(LocalTransportRefusal::UnsafeDirectory)?;

		Ok(parent.join("decodex.sock.stage"))
	}

	fn endpoint_path_fits(path: &Path) -> bool {
		let mut address = MaybeUninit::<sockaddr_un>::zeroed();
		// SAFETY: a zeroed `sockaddr_un` is valid for inspecting `sun_path`.
		let capacity = unsafe { address.assume_init_mut().sun_path.len() };

		path.as_os_str().as_bytes().len() < capacity
	}

	fn secure_directory(metadata: &Metadata, expected_uid: u32) -> bool {
		metadata.is_dir()
			&& metadata.uid() == expected_uid
			&& metadata.mode() & 0o7777 == PRIVATE_DIRECTORY_MODE
	}

	fn secure_socket(identity: SocketIdentity, expected_uid: u32) -> bool {
		identity.uid == expected_uid
			&& identity.mode & 0o7777 == PRIVATE_FILE_MODE
			&& identity.links == 1
	}

	fn secure_namespace_lock_metadata(metadata: &Metadata, expected_uid: u32) -> bool {
		metadata.is_file()
			&& metadata.uid() == expected_uid
			&& metadata.mode() & 0o7777 == PRIVATE_FILE_MODE
			&& metadata.nlink() == 1
	}

	fn secure_namespace_lock(identity: LockIdentity, expected_uid: u32) -> bool {
		identity.uid == expected_uid
			&& identity.mode & 0o7777 == PRIVATE_FILE_MODE
			&& identity.links == 1
	}

	fn socket_entry_absent(directory: &File, name: &CStr) -> bool {
		let mut metadata = MaybeUninit::<stat>::zeroed();
		// SAFETY: the descriptor and fixed name are valid, and no result bytes are read on error.
		let result = unsafe {
			libc::fstatat(
				directory.as_raw_fd(),
				name.as_ptr(),
				metadata.as_mut_ptr(),
				AT_SYMLINK_NOFOLLOW,
			)
		};

		result == -1 && io::Error::last_os_error().kind() == ErrorKind::NotFound
	}

	fn socket_identity(directory: &File, name: &CStr) -> io::Result<SocketIdentity> {
		let metadata = stat_at(directory, name)?;

		if metadata.st_mode & S_IFMT != S_IFSOCK {
			return Err(io::Error::new(ErrorKind::InvalidData, "entry is not a Unix socket"));
		}

		Ok(SocketIdentity {
			file: FileIdentity { device: metadata.st_dev as u64, inode: metadata.st_ino as u64 },
			uid: metadata.st_uid,
			mode: metadata.st_mode as u32,
			links: metadata.st_nlink as u64,
		})
	}

	fn lock_identity(directory: &File, name: &CStr) -> io::Result<LockIdentity> {
		let metadata = stat_at(directory, name)?;

		if metadata.st_mode & S_IFMT != S_IFREG {
			return Err(io::Error::new(ErrorKind::InvalidData, "entry is not a regular file"));
		}

		Ok(LockIdentity {
			file: FileIdentity { device: metadata.st_dev as u64, inode: metadata.st_ino as u64 },
			uid: metadata.st_uid,
			mode: metadata.st_mode as u32,
			links: metadata.st_nlink as u64,
		})
	}

	fn stat_at(directory: &File, name: &CStr) -> io::Result<stat> {
		let mut metadata = MaybeUninit::<stat>::zeroed();
		// SAFETY: successful `fstatat` initializes the complete `stat` value.
		let result = unsafe {
			libc::fstatat(
				directory.as_raw_fd(),
				name.as_ptr(),
				metadata.as_mut_ptr(),
				AT_SYMLINK_NOFOLLOW,
			)
		};

		if result == -1 {
			return Err(io::Error::last_os_error());
		}

		// SAFETY: successful `fstatat` initialized the value.
		Ok(unsafe { metadata.assume_init() })
	}

	fn chmod_socket(directory: &DirectoryBinding, name: &CStr, mode: u32) -> io::Result<()> {
		// Linux did not add kernel support for `AT_SYMLINK_NOFOLLOW` here until
		// 6.5. The retained owner-only directory excludes a different UID, and
		// the caller captures the socket identity before this call and requires
		// that same identity immediately afterwards. Hostile same-UID mutation
		// is outside this authority's confinement boundary.
		//
		// SAFETY: the retained directory descriptor and NUL-terminated name
		// remain valid for this descriptor-relative call.
		let result = unsafe {
			libc::fchmodat(directory.directory.as_raw_fd(), name.as_ptr(), mode as mode_t, 0)
		};

		if result == -1 { Err(io::Error::last_os_error()) } else { Ok(()) }
	}

	fn open_namespace_lock(directory: &File) -> io::Result<(File, bool)> {
		// SAFETY: a successful `openat` returns a fresh owned descriptor.
		let created = unsafe {
			libc::openat(
				directory.as_raw_fd(),
				NAMESPACE_LOCK_NAME.as_ptr(),
				O_RDWR | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
				PRIVATE_FILE_MODE as libc::c_uint,
			)
		};

		if created != -1 {
			// SAFETY: ownership of the fresh descriptor transfers to `File`.
			return Ok((unsafe { File::from_raw_fd(created) }, true));
		}

		let error = io::Error::last_os_error();

		if error.raw_os_error() != Some(libc::EEXIST) {
			return Err(error);
		}

		// SAFETY: a successful `openat` returns a fresh owned descriptor.
		let existing = unsafe {
			libc::openat(
				directory.as_raw_fd(),
				NAMESPACE_LOCK_NAME.as_ptr(),
				O_RDWR | O_NOFOLLOW | O_CLOEXEC,
			)
		};

		if existing == -1 {
			return Err(io::Error::last_os_error());
		}

		// SAFETY: ownership of the fresh descriptor transfers to `File`.
		Ok((unsafe { File::from_raw_fd(existing) }, false))
	}

	fn open_absolute_directory(path: &Path) -> io::Result<File> {
		if !path.is_absolute() {
			return Err(io::Error::new(ErrorKind::InvalidInput, "path is not absolute"));
		}

		let mut directory = open_directory(AT_FDCWD, c"/")?;

		for component in path.components() {
			match component {
				Component::RootDir => {},
				Component::Normal(name) => {
					let name = CString::new(name.as_bytes()).map_err(|_| {
						io::Error::new(ErrorKind::InvalidInput, "path contains NUL")
					})?;

					directory = open_directory(directory.as_raw_fd(), &name)?;
				},
				_ => {
					return Err(io::Error::new(ErrorKind::InvalidInput, "path is not normalized"));
				},
			}
		}

		Ok(directory)
	}

	fn open_directory(parent: RawFd, name: &CStr) -> io::Result<File> {
		// SAFETY: `parent` is a directory or `AT_FDCWD`, and `name` is NUL-terminated.
		let descriptor = unsafe {
			libc::openat(
				parent,
				name.as_ptr(),
				DIRECTORY_ACCESS | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
			)
		};

		if descriptor == -1 {
			return Err(io::Error::last_os_error());
		}

		// SAFETY: ownership of the fresh descriptor transfers to `File`.
		Ok(unsafe { File::from_raw_fd(descriptor) })
	}

	fn map_initial_endpoint(error: io::Error) -> LocalTransportRefusal {
		if error.kind() == ErrorKind::NotFound {
			LocalTransportRefusal::EndpointUnavailable
		} else {
			LocalTransportRefusal::UnsafeEndpoint
		}
	}

	fn map_lock_open(error: io::Error) -> LocalTransportRefusal {
		if error.kind() == ErrorKind::WouldBlock {
			LocalTransportRefusal::EndpointInUse
		} else {
			LocalTransportRefusal::UnsafeEndpoint
		}
	}

	pub(super) fn effective_user_id() -> u32 {
		// SAFETY: `geteuid` has no arguments or failure return.
		unsafe { libc::geteuid() }
	}
}
