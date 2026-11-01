//! Immutable outbound text, transferred by Rust without a control-model turn.

use std::{
	borrow::Cow,
	ffi::OsStr,
	fs,
	io::Write as _,
	path::{Path, PathBuf},
};

use serde::Serialize;
use sha2::{Digest as _, Sha256};
use tempfile::NamedTempFile;

use crate::{
	Request, Result,
	store::{Record, Store},
};

/// A transport preference, independent of tokens and the provider's message limits.
pub(crate) const INLINE_BYTES: usize = 128 * 1_024;

impl Request {
	pub(crate) fn inline_text(&self) -> String {
		match &self.context {
			Some(context) => format!("{}\n\n[Selected context]\n{context}", self.prompt),
			None => self.prompt.clone(),
		}
	}

	pub(crate) fn attachment_text(&self, includes_question: bool) -> Cow<'_, str> {
		if includes_question {
			match &self.context {
				Some(_) => Cow::Owned(self.inline_text()),
				None => Cow::Borrowed(&self.prompt),
			}
		} else {
			Cow::Borrowed(self.context.as_deref().unwrap_or_default())
		}
	}
}

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

		self.save_material(&format!("{}.prompt.txt", record.request.id), &text)
	}

	pub fn attachment(&self, record: &Record) -> Result<Option<Material>> {
		let Some(file) = &record.receipt.attachment else { return Ok(None) };
		let text = record.request.attachment_text(file.includes_question);

		if text.len() != file.bytes || digest(text.as_bytes()) != file.sha256 {
			return Err(
				"The saved source no longer matches its attachment manifest. No message was sent."
					.into(),
			);
		}

		self.save_material(&file.name, &text).map(Some)
	}

	fn save_material(&self, name: &str, text: &str) -> Result<Material> {
		if Path::new(name).file_name() != Some(OsStr::new(name)) {
			return Err("Saved request material must stay inside its component directory.".into());
		}

		let path = self.root.join(name);

		if path.try_exists().map_err(|e| e.to_string())? {
			if !fs::symlink_metadata(&path).map_err(|e| e.to_string())?.is_file()
				|| fs::read(&path).map_err(|e| e.to_string())? != text.as_bytes()
			{
				return Err("The saved request material changed. No message was sent.".into());
			}
		} else {
			let mut file = NamedTempFile::new_in(&self.root).map_err(|e| e.to_string())?;

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
