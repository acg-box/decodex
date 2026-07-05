use std::{fs, io::ErrorKind, path::Path};

use crate::prelude::Result;

pub(crate) fn remove_interrupt_request(path: &Path) -> Result<()> {
	remove_request(path)
}

pub(crate) fn remove_steer_request(path: &Path) -> Result<()> {
	remove_request(path)
}

fn remove_request(path: &Path) -> Result<()> {
	match fs::remove_file(path) {
		Ok(()) => Ok(()),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
		Err(error) => Err(error.into()),
	}
}
