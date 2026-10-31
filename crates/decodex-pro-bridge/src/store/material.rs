//! Immutable outbound text. Native control receives this descriptor, not the text.

use std::{fs, io::Write as _, path::PathBuf};

use serde::Serialize;
use sha2::{Digest as _, Sha256};

use super::{Record, Store};
use crate::Result;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Material {
	pub path: PathBuf,
	pub bytes: usize,
	pub sha256: String,
	pub normalized_sha256: String,
}

impl Store {
	pub fn material(&self, record: &Record) -> Result<Material> {
		let text = record.wire_prompt();
		let path = self.root.join(format!("{}.prompt.txt", record.request.id));
		if path.try_exists().map_err(|e| e.to_string())? {
			if !fs::symlink_metadata(&path).map_err(|e| e.to_string())?.is_file()
				|| fs::read(&path).map_err(|e| e.to_string())? != text.as_bytes()
			{
				return Err("The saved request material changed. No message was sent.".into());
			}
		} else {
			let mut file =
				tempfile::NamedTempFile::new_in(&self.root).map_err(|e| e.to_string())?;
			file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
			file.as_file().sync_all().map_err(|e| e.to_string())?;
			file.persist_noclobber(&path).map_err(|e| e.to_string())?;
		}
		Ok(Material {
			path,
			bytes: text.len(),
			sha256: digest(text.as_bytes()),
			normalized_sha256: digest(text.trim().as_bytes()),
		})
	}
}

pub(crate) fn digest(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}
