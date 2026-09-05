//! Optional inherited parent-lifetime channel for the bundled desktop service.

use std::{
	io::{self, ErrorKind},
	mem::MaybeUninit,
	os::fd::{FromRawFd as _, RawFd},
};

use tokio::io::AsyncReadExt as _;

pub(crate) struct ParentLifetime {
	channel: tokio::net::UnixStream,
}

impl ParentLifetime {
	pub(crate) fn from_inherited_fd(raw_fd: RawFd) -> io::Result<Self> {
		if raw_fd <= libc::STDERR_FILENO {
			return Err(io::Error::new(ErrorKind::InvalidInput, "parent channel fd is reserved"));
		}
		validate_socket(raw_fd)?;

		// SAFETY: validation proves that this live descriptor is an owned Unix stream socket, and
		// the hidden CLI contract transfers its sole child-process ownership to this function.
		let channel = unsafe { std::os::unix::net::UnixStream::from_raw_fd(raw_fd) };
		channel.set_nonblocking(true)?;
		Ok(Self { channel: tokio::net::UnixStream::from_std(channel)? })
	}

	pub(crate) async fn wait_for_parent_exit(&mut self) -> io::Result<()> {
		let mut unexpected = [0_u8; 1];
		match self.channel.read(&mut unexpected).await? {
			0 => Ok(()),
			_ => Err(io::Error::new(
				ErrorKind::InvalidData,
				"parent lifetime channel carried unexpected data",
			)),
		}
	}
}

fn validate_socket(raw_fd: RawFd) -> io::Result<()> {
	// SAFETY: `fcntl` only inspects the caller-supplied descriptor.
	if unsafe { libc::fcntl(raw_fd, libc::F_GETFD) } < 0 {
		return Err(io::Error::last_os_error());
	}

	let mut socket_type = 0_i32;
	let mut socket_type_len = libc::socklen_t::try_from(std::mem::size_of::<i32>())
		.map_err(|_| io::Error::new(ErrorKind::InvalidInput, "socket type is not representable"))?;
	// SAFETY: both output pointers reference initialized writable storage of the declared length.
	if unsafe {
		libc::getsockopt(
			raw_fd,
			libc::SOL_SOCKET,
			libc::SO_TYPE,
			std::ptr::from_mut(&mut socket_type).cast(),
			&mut socket_type_len,
		)
	} != 0 || socket_type != libc::SOCK_STREAM
	{
		return Err(io::Error::new(ErrorKind::InvalidInput, "parent channel is not a stream"));
	}

	let mut status = MaybeUninit::<libc::stat>::uninit();
	// SAFETY: `status` provides writable storage for one `stat` result.
	if unsafe { libc::fstat(raw_fd, status.as_mut_ptr()) } != 0 {
		return Err(io::Error::last_os_error());
	}
	// SAFETY: successful `fstat` initialized the complete value.
	let status = unsafe { status.assume_init() };
	if status.st_mode & libc::S_IFMT != libc::S_IFSOCK || status.st_uid != effective_user_id() {
		return Err(io::Error::new(
			ErrorKind::PermissionDenied,
			"parent channel ownership or type is invalid",
		));
	}

	Ok(())
}

fn effective_user_id() -> libc::uid_t {
	// SAFETY: `geteuid` has no preconditions.
	unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests {
	use std::os::fd::IntoRawFd as _;

	use super::*;

	#[tokio::test]
	async fn inherited_socket_eof_reports_parent_exit() {
		let (parent, child) = std::os::unix::net::UnixStream::pair().expect("create socket pair");
		let mut lifetime = ParentLifetime::from_inherited_fd(child.into_raw_fd())
			.expect("accept inherited child endpoint");
		drop(parent);
		lifetime.wait_for_parent_exit().await.expect("observe parent EOF");
	}

	#[tokio::test]
	async fn inherited_socket_rejects_data_as_a_closed_protocol() {
		use std::io::Write as _;

		let (mut parent, child) =
			std::os::unix::net::UnixStream::pair().expect("create socket pair");
		let mut lifetime = ParentLifetime::from_inherited_fd(child.into_raw_fd())
			.expect("accept inherited child endpoint");
		parent.write_all(&[1]).expect("write unexpected byte");
		assert_eq!(
			lifetime.wait_for_parent_exit().await.expect_err("reject channel data").kind(),
			ErrorKind::InvalidData,
		);
	}
}
