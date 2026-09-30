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
	let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp_path)?;
	let write_result = (|| -> Result<()> {
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

#[cfg(test)]
mod tests {
	use std::{fs, process};

	use crate::validation_files::json_io;

	#[test]
	fn temporary_name_collision_preserves_both_existing_files() {
		let temp = tempfile::tempdir().expect("temporary directory");
		let path = temp.path().join("artifact.json");
		let staging = temp.path().join(format!(".artifact.json.tmp-{}", process::id()));

		fs::write(&path, b"original artifact").unwrap();
		fs::write(&staging, b"another writer's data").unwrap();

		assert!(json_io::write_json(&path, &serde_json::json!({"updated": true})).is_err());
		assert_eq!(fs::read(&path).unwrap(), b"original artifact");
		assert_eq!(fs::read(&staging).unwrap(), b"another writer's data");

		fs::remove_file(staging).unwrap();
		json_io::write_json(&path, &serde_json::json!({"updated": true}))
			.expect("write after collision");

		assert_eq!(json_io::load_json(&path).unwrap(), serde_json::json!({"updated": true}));
	}
}
