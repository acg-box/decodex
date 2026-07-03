use crate::{OpenOptions, Path, Value, Write as _, eyre, fs, prelude::Result, process, serde_json};

pub(crate) fn load_json(path: &Path) -> Result<Value> {
	let raw = fs::read_to_string(path)?;

	serde_json::from_str(&raw)
		.map_err(|error| eyre::eyre!("Failed to parse JSON from {}: {error}", path.display()))
}

pub(crate) fn write_json(path: &Path, payload: &Value) -> Result<()> {
	if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
		fs::create_dir_all(parent)?;
	}

	let mut output = serde_json::to_string_pretty(payload)?;

	output.push('\n');

	let parent = path.parent().unwrap_or_else(|| Path::new("."));
	let file_name = path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("JSON output path must end in a valid file name"))?;
	let temp_path = parent.join(format!(".{file_name}.tmp-{}", process::id()));
	let write_result = (|| -> Result<()> {
		let mut file = OpenOptions::new().write(true).create_new(true).open(&temp_path)?;

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
