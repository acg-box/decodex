//! Optional inherited parent-lifetime channel for the bundled desktop service.

use std::{
	io::{Error, ErrorKind},
	mem::{self, MaybeUninit},
	os::{
		fd::{FromRawFd as _, RawFd},
		unix::net,
	},
	ptr,
};

use libc::{
	F_GETFD, S_IFMT, S_IFSOCK, SO_TYPE, SOCK_STREAM, SOL_SOCKET, STDERR_FILENO, socklen_t, stat,
	uid_t,
};
use tokio::io::AsyncReadExt as _;

pub(crate) struct ParentLifetime {
	channel: tokio::net::UnixStream,
}
impl ParentLifetime {
	pub(crate) fn from_inherited_fd(raw_fd: RawFd) -> Result<Self, Error> {
		if raw_fd <= STDERR_FILENO {
			return Err(Error::new(ErrorKind::InvalidInput, "parent channel fd is reserved"));
		}

		validate_socket(raw_fd)?;

		// SAFETY: validation proves that this live descriptor is an owned Unix stream socket, and
		// the hidden CLI contract transfers its sole child-process ownership to this function.
		let channel = unsafe { net::UnixStream::from_raw_fd(raw_fd) };

		channel.set_nonblocking(true)?;

		Ok(Self { channel: tokio::net::UnixStream::from_std(channel)? })
	}

	pub(crate) async fn wait_for_parent_exit(&mut self) -> Result<(), Error> {
		let mut unexpected = [0_u8; 1];

		match self.channel.read(&mut unexpected).await? {
			0 => Ok(()),
			_ => Err(Error::new(
				ErrorKind::InvalidData,
				"parent lifetime channel carried unexpected data",
			)),
		}
	}
}

fn validate_socket(raw_fd: RawFd) -> Result<(), Error> {
	// SAFETY: `fcntl` only inspects the caller-supplied descriptor.
	if unsafe { libc::fcntl(raw_fd, F_GETFD) } < 0 {
		return Err(Error::last_os_error());
	}

	let mut socket_type = 0_i32;
	let mut socket_type_len = socklen_t::try_from(mem::size_of::<i32>())
		.map_err(|_| Error::new(ErrorKind::InvalidInput, "socket type is not representable"))?;

	// SAFETY: both output pointers reference initialized writable storage of the declared length.
	if unsafe {
		libc::getsockopt(
			raw_fd,
			SOL_SOCKET,
			SO_TYPE,
			ptr::from_mut(&mut socket_type).cast(),
			&mut socket_type_len,
		)
	} != 0
		|| socket_type != SOCK_STREAM
	{
		return Err(Error::new(ErrorKind::InvalidInput, "parent channel is not a stream"));
	}

	let mut status = MaybeUninit::<stat>::uninit();

	// SAFETY: `status` provides writable storage for one `stat` result.
	if unsafe { libc::fstat(raw_fd, status.as_mut_ptr()) } != 0 {
		return Err(Error::last_os_error());
	}

	// SAFETY: successful `fstat` initialized the complete value.
	let status = unsafe { status.assume_init() };

	if status.st_mode & S_IFMT != S_IFSOCK || status.st_uid != effective_user_id() {
		return Err(Error::new(
			ErrorKind::PermissionDenied,
			"parent channel ownership or type is invalid",
		));
	}

	Ok(())
}

fn effective_user_id() -> uid_t {
	// SAFETY: `geteuid` has no preconditions.
	unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests {
	use std::{
		io::{ErrorKind, Write as _},
		os::{fd::IntoRawFd as _, unix::net},
	};

	use crate::service::parent_lifetime::ParentLifetime;

	#[tokio::test]
	async fn inherited_socket_eof_reports_parent_exit() {
		let (parent, child) = net::UnixStream::pair().expect("create socket pair");
		let mut lifetime = ParentLifetime::from_inherited_fd(child.into_raw_fd())
			.expect("accept inherited child endpoint");

		drop(parent);

		lifetime.wait_for_parent_exit().await.expect("observe parent EOF");
	}

	#[tokio::test]
	async fn inherited_socket_rejects_data_as_a_closed_protocol() {
		let (mut parent, child) = net::UnixStream::pair().expect("create socket pair");
		let mut lifetime = ParentLifetime::from_inherited_fd(child.into_raw_fd())
			.expect("accept inherited child endpoint");

		parent.write_all(&[1]).expect("write unexpected byte");

		assert_eq!(
			lifetime.wait_for_parent_exit().await.expect_err("reject channel data").kind(),
			ErrorKind::InvalidData,
		);
	}
}
