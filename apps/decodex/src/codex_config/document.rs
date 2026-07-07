#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::{fs, io::ErrorKind, path::Path, process};

use toml_edit::DocumentMut;

use crate::prelude::{Result, eyre};

pub(in crate::codex_config) fn load_codex_config_document(path: &Path) -> Result<DocumentMut> {
	let input = match fs::read_to_string(path) {
		Ok(input) => input,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(DocumentMut::new()),
		Err(error) => {
			eyre::bail!("Failed to read Codex config `{}`: {error}", path.display());
		},
	};

	if input.trim().is_empty() {
		return Ok(DocumentMut::new());
	}

	input
		.parse::<DocumentMut>()
		.map_err(|error| eyre::eyre!("Codex config `{}` is invalid TOML: {error}", path.display()))
}

pub(in crate::codex_config) fn write_codex_config_document(
	path: &Path,
	document: &DocumentMut,
) -> Result<()> {
	let parent = path.parent().ok_or_else(|| {
		eyre::eyre!("Codex config path `{}` must have a parent directory.", path.display())
	})?;
	let file_name = path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("Codex config path must end in a valid file name."))?;
	let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
	let mut output = document.to_string();

	if !output.ends_with('\n') {
		output.push('\n');
	}

	fs::create_dir_all(parent)?;
	fs::write(&temp_path, output)?;

	secure_config_file(&temp_path)?;

	fs::rename(temp_path, path)?;

	secure_config_file(path)?;

	Ok(())
}

fn secure_config_file(path: &Path) -> Result<()> {
	#[cfg(unix)]
	{
		let mut permissions = fs::metadata(path)?.permissions();

		permissions.set_mode(0o600);

		fs::set_permissions(path, permissions)?;
	}

	Ok(())
}
