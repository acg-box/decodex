#[cfg(unix)] use std::os::unix::ffi::OsStringExt as _;
use std::{ffi::OsString, path::PathBuf};

use crate::prelude::Result;

pub(crate) fn path_buf_from_git_line_output(output: &[u8]) -> Result<Option<PathBuf>> {
	let resolved = output.strip_suffix(b"\n").unwrap_or(output);
	let resolved = resolved.strip_suffix(b"\r").unwrap_or(resolved);

	path_buf_from_git_bytes(resolved)
}

#[cfg(unix)]
pub(crate) fn path_buf_from_git_bytes(path: &[u8]) -> Result<Option<PathBuf>> {
	if path.is_empty() {
		return Ok(None);
	}

	Ok(Some(PathBuf::from(OsString::from_vec(path.to_vec()))))
}

#[cfg(not(unix))]
pub(crate) fn path_buf_from_git_bytes(path: &[u8]) -> Result<Option<PathBuf>> {
	let resolved = String::from_utf8(path.to_vec())?;

	if resolved.is_empty() {
		return Ok(None);
	}

	Ok(Some(PathBuf::from(resolved)))
}
