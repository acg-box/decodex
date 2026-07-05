use std::{fs::File, io::Error, os::fd::AsRawFd};

use libc::{F_GETFD, F_SETFD, FD_CLOEXEC};

use crate::prelude::Result;

pub(in crate::state) fn clear_close_on_exec(file: &File) -> Result<()> {
	let fd = file.as_raw_fd();
	let existing_flags = unsafe { libc::fcntl(fd, F_GETFD) };

	if existing_flags == -1 {
		return Err(Error::last_os_error().into());
	}

	let new_flags = existing_flags & !FD_CLOEXEC;

	if new_flags != existing_flags {
		let result = unsafe { libc::fcntl(fd, F_SETFD, new_flags) };

		if result == -1 {
			return Err(Error::last_os_error().into());
		}
	}

	Ok(())
}

pub(in crate::state) fn set_close_on_exec(file: &File) -> Result<()> {
	let fd = file.as_raw_fd();
	let existing_flags = unsafe { libc::fcntl(fd, F_GETFD) };

	if existing_flags == -1 {
		return Err(Error::last_os_error().into());
	}

	let new_flags = existing_flags | FD_CLOEXEC;

	if new_flags != existing_flags {
		let result = unsafe { libc::fcntl(fd, F_SETFD, new_flags) };

		if result == -1 {
			return Err(Error::last_os_error().into());
		}
	}

	Ok(())
}
