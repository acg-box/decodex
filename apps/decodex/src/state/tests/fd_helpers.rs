#[cfg(unix)] use libc::{F_GETFD, FD_CLOEXEC};

#[cfg(unix)]
pub(crate) fn fd_has_close_on_exec(fd: i32) -> bool {
	let flags = unsafe { libc::fcntl(fd, F_GETFD) };

	assert_ne!(flags, -1, "fcntl(F_GETFD) should succeed for test fd {fd}");

	flags & FD_CLOEXEC != 0
}
