//! Private atomic records and an OS-held execution lease. A crash releases the lease.

use std::{
	fs::{self, DirBuilder, File, OpenOptions},
	io::Write as _,
	path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};

use serde::{Deserialize, Serialize};

use crate::{Answer, Phase, Receipt, Request, Result};

mod material;
pub(crate) use material::{Material, digest};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Record {
	pub request: Request,
	pub receipt: Receipt,
	pub answer: Option<Answer>,
	pub parent_answer_id: Option<String>,
	/// Owned browser target saved before Send, so a missing URL can be recovered.
	pub target_id: Option<String>,
}

impl Record {
	pub fn wire_prompt(&self) -> String {
		format!("[Decodex Pro request: {}]\n\n{}", self.request.id, self.request.prompt)
	}
}

pub(crate) struct Store {
	root: PathBuf,
}

impl Store {
	pub fn open(root: &Path) -> Result<Self> {
		if !root.is_absolute() {
			return Err("The component state directory must be absolute.".into());
		}
		let mut builder = DirBuilder::new();
		builder.recursive(true);
		#[cfg(unix)]
		builder.mode(0o700);
		builder.create(root).map_err(|e| e.to_string())?;
		let metadata = fs::symlink_metadata(root).map_err(|e| e.to_string())?;
		if !metadata.is_dir() {
			return Err("The component state directory must be a real directory.".into());
		}
		#[cfg(unix)]
		if metadata.permissions().mode() & 0o077 != 0 {
			return Err("The component state directory must have mode 0700.".into());
		}
		Ok(Self { root: root.into() })
	}

	pub fn lease(&self) -> Result<File> {
		self.try_lease("records.lock")?
			.ok_or_else(|| "The research journal is being updated. Try again.".into())
	}

	pub fn worker_lease(&self, id: &str) -> Result<Option<File>> {
		validate_id(id)?;
		self.try_lease(&format!("{id}.worker.lock"))
	}

	pub fn execution_lease(&self) -> Result<Option<File>> {
		self.try_lease("execution.lock")
	}

	pub fn browser_lease(&self) -> Result<Option<File>> {
		self.try_lease("browser.lock")
	}

	fn try_lease(&self, name: &str) -> Result<Option<File>> {
		let mut options = OpenOptions::new();
		options.read(true).write(true).create(true).truncate(false);
		#[cfg(unix)]
		options.mode(0o600);
		let file = options.open(self.root.join(name)).map_err(|e| e.to_string())?;
		match file.try_lock() {
			Ok(()) => Ok(Some(file)),
			Err(std::fs::TryLockError::WouldBlock) => Ok(None),
			Err(error) => Err(error.to_string()),
		}
	}

	pub fn load(&self, id: &str) -> Result<Record> {
		validate_id(id)?;
		let bytes = fs::read(self.path(id)).map_err(|e| e.to_string())?;
		let record: Record = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
		if record.request.id != id || record.receipt.id != id {
			return Err("The request record identity changed.".into());
		}
		Ok(record)
	}

	pub fn prepare(&self, request: Request) -> Result<Record> {
		validate_id(&request.id)?;
		if request.prompt.is_empty() {
			return Err("The prompt must not be empty.".into());
		}
		if let Some(saved) = self.existing(&request)? {
			return Ok(saved);
		}
		let parent = request.previous.as_deref().map(|id| self.load(id)).transpose()?;
		if parent.as_ref().is_some_and(|r| r.receipt.phase != Phase::Complete) {
			return Err("The previous request must be complete before continuation.".into());
		}
		let record = Record {
			receipt: Receipt {
				id: request.id.clone(),
				phase: Phase::Prepared,
				conversation_url: parent.as_ref().and_then(|r| r.receipt.conversation_url.clone()),
				model: None,
				execution: None,
				user_message_id: None,
				input_binding: None,
				baseline_messages: 0,
				last_error: None,
			},
			parent_answer_id: parent
				.as_ref()
				.and_then(|r| r.answer.as_ref().map(|a| a.message_id.clone())),
			target_id: parent.and_then(|r| r.target_id),
			request,
			answer: None,
		};
		self.save(&record)?;
		Ok(record)
	}

	pub fn existing(&self, request: &Request) -> Result<Option<Record>> {
		validate_id(&request.id)?;
		if !self.path(&request.id).exists() {
			return Ok(None);
		}
		let saved = self.load(&request.id)?;
		if saved.request != *request {
			return Err("The request ID already identifies different input.".into());
		}
		Ok(Some(saved))
	}

	pub fn save(&self, record: &Record) -> Result<()> {
		validate_id(&record.request.id)?;
		let bytes = serde_json::to_vec_pretty(record).map_err(|e| e.to_string())?;
		let mut temporary =
			tempfile::NamedTempFile::new_in(&self.root).map_err(|e| e.to_string())?;
		temporary.write_all(&bytes).map_err(|e| e.to_string())?;
		temporary.as_file().sync_all().map_err(|e| e.to_string())?;
		temporary.persist(self.path(&record.request.id)).map_err(|e| e.to_string())?;
		File::open(&self.root).and_then(|f| f.sync_all()).map_err(|e| e.to_string())
	}

	fn path(&self, id: &str) -> PathBuf {
		self.root.join(format!("{id}.json"))
	}

	pub fn setup_target(&self) -> Result<Option<String>> {
		match fs::read_to_string(self.root.join("setup-target")) {
			Ok(id) => Ok(Some(id)),
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
			Err(error) => Err(error.to_string()),
		}
	}

	pub fn save_setup_target(&self, id: &str) -> Result<()> {
		fs::write(self.root.join("setup-target"), id).map_err(|e| e.to_string())
	}
}

fn validate_id(id: &str) -> Result<()> {
	if id.is_empty() || id.len() > 64 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
	{
		return Err("Request IDs must contain 1 to 64 ASCII letters, digits, or hyphens.".into());
	}
	Ok(())
}

#[cfg(test)] mod tests;
