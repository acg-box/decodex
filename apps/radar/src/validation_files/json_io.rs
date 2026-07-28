use std::os::unix::fs::OpenOptionsExt as _;

use crate::{OpenOptions, Path, Value, Write as _, eyre, fs, prelude::Result, process, serde_json};

pub(crate) fn load_json(path: &Path) -> Result<Value> {
	let raw = if crate::is_radar_cache_path(path) {
		String::from_utf8(crate::read_private_file(path)?)
			.map_err(|error| eyre::eyre!("Radar cache JSON is not UTF-8: {error}"))?
	} else {
		fs::read_to_string(path)?
	};

	serde_json::from_str(&raw)
		.map_err(|error| eyre::eyre!("Failed to parse JSON from {}: {error}", path.display()))
}

pub(crate) fn write_json(path: &Path, payload: &Value) -> Result<()> {
	let mut output = serde_json::to_string_pretty(payload)?;

	output.push('\n');

	if crate::is_radar_cache_path(path) {
		return crate::write_private_file_atomic(path, output.as_bytes());
	}

	if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
		fs::create_dir_all(parent)?;
	}

	let parent = path.parent().unwrap_or_else(|| Path::new("."));
	let file_name = path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("JSON output path must end in a valid file name"))?;
	let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
	let write_result = (|| -> Result<()> {
		let mut file =
			OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp_path)?;

		file.write_all(output.as_bytes())?;
		file.sync_all()?;

		fs::rename(&temp_path, path)?;

		Ok(())
	})();

	if write_result.is_err() {
		let _ = fs::remove_file(&temp_path);
	}

	write_result?;

	Ok(())
}
