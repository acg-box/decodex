#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::{fs, io::ErrorKind, path::Path, process};

use crate::{
	agent::codex_accounts::record::{auth_json::AuthDotJson, model::AccountPoolRecord},
	prelude::{Result, eyre},
};

pub(in crate::agent::codex_accounts) fn sync_refreshed_record_to_codex_auth(
	record: &AccountPoolRecord,
	path: &Path,
) -> Result<()> {
	let input = match fs::read_to_string(path) {
		Ok(input) => input,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
		Err(_) => return Ok(()),
	};
	let auth = match serde_json::from_str::<AuthDotJson>(&input) {
		Ok(auth) => auth,
		Err(_) => return Ok(()),
	};
	let auth_account_id = auth
		.tokens
		.as_ref()
		.and_then(|tokens| tokens.account_id.as_deref())
		.filter(|account_id| !account_id.trim().is_empty());

	if auth_account_id != record.account_id() {
		return Ok(());
	}

	write_auth_json_atomically(path, &record.auth_dot_json())
}

pub(in crate::agent::codex_accounts) fn write_auth_json_atomically(
	path: &Path,
	auth: &AuthDotJson,
) -> Result<()> {
	let parent = path.parent().ok_or_else(|| {
		eyre::eyre!("Codex auth JSON path `{}` must have a parent directory.", path.display())
	})?;
	let file_name = path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("Codex auth JSON path must end in a valid file name."))?;
	let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
	let mut output = serde_json::to_string_pretty(auth)?;

	output.push('\n');

	fs::create_dir_all(parent)?;
	fs::write(&temp_path, output)?;

	secure_account_file(&temp_path)?;

	fs::rename(temp_path, path)?;

	secure_account_file(path)?;

	Ok(())
}

pub(in crate::agent::codex_accounts) fn secure_account_file(path: &Path) -> Result<()> {
	#[cfg(unix)]
	{
		let mode = if path.is_dir() { 0o700 } else { 0o600 };
		let mut permissions = fs::metadata(path)?.permissions();

		permissions.set_mode(mode);

		fs::set_permissions(path, permissions)?;
	}

	Ok(())
}
