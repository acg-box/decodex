//! In-process Codex Fast mode configuration.

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::{
	env,
	ffi::OsString,
	fs::{self, DirBuilder, File, OpenOptions},
	io::{Error, ErrorKind, Read as _, Write as _},
	path::{Path, PathBuf},
	process,
};

use serde::Serialize;
use toml_edit::{DocumentMut, Item, Table};

const MAX_CONFIG_BYTES: u64 = 1_048_576;
const TEMP_FILE_ATTEMPTS: u8 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FastModeFailure {
	HomeUnavailable,
	UnsafeConfigPath,
	ConfigUnavailable,
	ConfigTooLarge,
	ConfigInvalid,
	FeaturesNotTable,
	FastModeNotBoolean,
	WriteFailed,
}

pub(crate) fn status() -> Result<bool, FastModeFailure> {
	status_at_path(&codex_config_path(env::var_os("HOME"))?)
}

pub(crate) fn set_enabled(enabled: bool) -> Result<bool, FastModeFailure> {
	set_at_path(&codex_config_path(env::var_os("HOME"))?, enabled)
}

fn codex_config_path(home: Option<OsString>) -> Result<PathBuf, FastModeFailure> {
	let home = home.filter(|value| !value.is_empty()).ok_or(FastModeFailure::HomeUnavailable)?;

	Ok(PathBuf::from(home).join(".codex").join("config.toml"))
}

fn status_at_path(path: &Path) -> Result<bool, FastModeFailure> {
	read_fast_mode(&read_document(path)?)
}

fn set_at_path(path: &Path, enabled: bool) -> Result<bool, FastModeFailure> {
	let mut document = read_document(path)?;
	let root = document.as_table_mut();

	if !root.contains_key("features") {
		root.insert("features", Item::Table(Table::new()));
	}

	let features = root
		.get_mut("features")
		.and_then(Item::as_table_like_mut)
		.ok_or(FastModeFailure::FeaturesNotTable)?;
	if let Some(value) = features.get("fast_mode")
		&& !value.is_none()
		&& value.as_bool().is_none()
	{
		return Err(FastModeFailure::FastModeNotBoolean);
	}

	features.insert("fast_mode", toml_edit::value(enabled));
	write_document(path, &document)?;

	Ok(enabled)
}

fn read_document(path: &Path) -> Result<DocumentMut, FastModeFailure> {
	match fs::symlink_metadata(path) {
		Ok(metadata) => {
			if metadata.file_type().is_symlink() || !metadata.is_file() {
				return Err(FastModeFailure::UnsafeConfigPath);
			}
			if metadata.len() > MAX_CONFIG_BYTES {
				return Err(FastModeFailure::ConfigTooLarge);
			}
		},
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(DocumentMut::new()),
		Err(_) => return Err(FastModeFailure::ConfigUnavailable),
	}

	let mut file = open_config_no_follow(path).map_err(map_config_open_error)?;
	let metadata = file.metadata().map_err(|_| FastModeFailure::ConfigUnavailable)?;
	if !metadata.is_file() {
		return Err(FastModeFailure::UnsafeConfigPath);
	}
	if metadata.len() > MAX_CONFIG_BYTES {
		return Err(FastModeFailure::ConfigTooLarge);
	}
	let mut bytes = Vec::new();
	std::io::Read::by_ref(&mut file)
		.take(MAX_CONFIG_BYTES + 1)
		.read_to_end(&mut bytes)
		.map_err(|_| FastModeFailure::ConfigUnavailable)?;
	if bytes.len() as u64 > MAX_CONFIG_BYTES {
		return Err(FastModeFailure::ConfigTooLarge);
	}
	let input = String::from_utf8(bytes).map_err(|_| FastModeFailure::ConfigUnavailable)?;
	if input.trim().is_empty() {
		return Ok(DocumentMut::new());
	}

	input.parse().map_err(|_| FastModeFailure::ConfigInvalid)
}

fn open_config_no_follow(path: &Path) -> Result<File, Error> {
	let mut options = OpenOptions::new();

	options.read(true);
	#[cfg(unix)]
	options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);

	options.open(path)
}

fn map_config_open_error(error: Error) -> FastModeFailure {
	if error.kind() == ErrorKind::NotFound {
		FastModeFailure::ConfigUnavailable
	} else if is_symlink_open_error(&error) {
		FastModeFailure::UnsafeConfigPath
	} else {
		FastModeFailure::ConfigUnavailable
	}
}

fn is_symlink_open_error(error: &Error) -> bool {
	#[cfg(unix)]
	{
		error.raw_os_error() == Some(libc::ELOOP)
	}
	#[cfg(not(unix))]
	{
		let _ = error;
		false
	}
}

fn read_fast_mode(document: &DocumentMut) -> Result<bool, FastModeFailure> {
	let Some(features) = document.get("features") else {
		return Ok(false);
	};
	let features = features.as_table_like().ok_or(FastModeFailure::FeaturesNotTable)?;
	let Some(value) = features.get("fast_mode") else {
		return Ok(false);
	};

	value.as_bool().ok_or(FastModeFailure::FastModeNotBoolean)
}

fn write_document(path: &Path, document: &DocumentMut) -> Result<(), FastModeFailure> {
	let parent = path.parent().ok_or(FastModeFailure::UnsafeConfigPath)?;

	prepare_parent(parent)?;

	let file_name =
		path.file_name().and_then(|name| name.to_str()).ok_or(FastModeFailure::UnsafeConfigPath)?;
	let mut output = document.to_string();
	if !output.ends_with('\n') {
		output.push('\n');
	}
	if output.len() as u64 > MAX_CONFIG_BYTES {
		return Err(FastModeFailure::ConfigTooLarge);
	}

	for attempt in 0..TEMP_FILE_ATTEMPTS {
		let temporary = parent.join(format!(".{file_name}.decodex-{}-{attempt}", process::id()));
		let Some(mut file) = create_private_file(&temporary)? else {
			continue;
		};
		let write_result = (|| {
			file.write_all(output.as_bytes()).map_err(|_| FastModeFailure::WriteFailed)?;
			file.sync_all().map_err(|_| FastModeFailure::WriteFailed)?;
			drop(file);
			fs::rename(&temporary, path).map_err(|_| FastModeFailure::WriteFailed)?;
			sync_directory(parent)
		})();

		if write_result.is_err() {
			let _ = fs::remove_file(&temporary);
		}

		return write_result;
	}

	Err(FastModeFailure::WriteFailed)
}

fn prepare_parent(parent: &Path) -> Result<(), FastModeFailure> {
	match fs::symlink_metadata(parent) {
		Ok(metadata) =>
			if metadata.file_type().is_symlink() || !metadata.is_dir() {
				return Err(FastModeFailure::UnsafeConfigPath);
			},
		Err(error) if error.kind() == ErrorKind::NotFound => {
			let mut builder = DirBuilder::new();

			#[cfg(unix)]
			builder.mode(0o700);

			builder.create(parent).map_err(|_| FastModeFailure::WriteFailed)?;
		},
		Err(_) => return Err(FastModeFailure::ConfigUnavailable),
	}

	#[cfg(unix)]
	{
		let metadata =
			fs::symlink_metadata(parent).map_err(|_| FastModeFailure::ConfigUnavailable)?;
		if metadata.file_type().is_symlink()
			|| !metadata.is_dir()
			|| metadata.permissions().mode() & 0o022 != 0
		{
			return Err(FastModeFailure::UnsafeConfigPath);
		}
	}

	Ok(())
}

fn create_private_file(path: &Path) -> Result<Option<File>, FastModeFailure> {
	let mut options = OpenOptions::new();

	options.write(true).create_new(true);
	#[cfg(unix)]
	options.mode(0o600).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);

	match options.open(path) {
		Ok(file) => Ok(Some(file)),
		Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(None),
		Err(_) => Err(FastModeFailure::WriteFailed),
	}
}

fn sync_directory(path: &Path) -> Result<(), FastModeFailure> {
	let mut options = OpenOptions::new();

	options.read(true);
	#[cfg(unix)]
	options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY);

	options
		.open(path)
		.and_then(|directory| directory.sync_all())
		.map_err(|_| FastModeFailure::WriteFailed)
}

#[cfg(test)]
mod tests {
	#[cfg(unix)] use std::os::unix::fs::{PermissionsExt as _, symlink};
	use std::{
		ffi::OsString,
		fs,
		path::{Path, PathBuf},
	};

	fn config_path(temp: &tempfile::TempDir) -> PathBuf {
		temp.path().join(".codex").join("config.toml")
	}

	#[test]
	fn missing_config_reports_disabled_without_creating_files() {
		let temp = tempfile::tempdir().expect("temporary directory must be created");
		let path = config_path(&temp);

		assert!(!super::status_at_path(&path).expect("missing config must be readable"));
		assert!(!path.exists());
	}

	#[test]
	fn set_preserves_unrelated_config_and_comments() {
		let temp = tempfile::tempdir().expect("temporary directory must be created");
		let path = config_path(&temp);

		fs::create_dir(path.parent().expect("config must have a parent"))
			.expect("Codex directory must be created");
		fs::write(
			&path,
			"# local Codex settings\nmodel = \"gpt-5.6\"\n\n[features]\nplugins = true\nfast_mode = false\n",
		)
		.expect("seed config must be written");

		assert!(super::set_at_path(&path, true).expect("Fast mode must be enabled"));

		let output = fs::read_to_string(&path).expect("config must be readable");

		assert!(output.contains("# local Codex settings"));
		assert!(output.contains("model = \"gpt-5.6\""));
		assert!(output.contains("plugins = true"));
		assert!(output.contains("fast_mode = true"));
		assert!(super::status_at_path(&path).expect("status must be readable"));
	}

	#[test]
	fn set_creates_private_codex_directory_and_config() {
		let temp = tempfile::tempdir().expect("temporary directory must be created");
		let path = config_path(&temp);

		super::set_at_path(&path, false).expect("Fast mode must be disabled");

		assert_eq!(
			fs::read_to_string(&path).expect("config must be readable"),
			"[features]\nfast_mode = false\n"
		);

		#[cfg(unix)]
		{
			assert_eq!(
				fs::metadata(path.parent().expect("config must have a parent"))
					.expect("Codex directory metadata must be readable")
					.permissions()
					.mode() & 0o777,
				0o700
			);
			assert_eq!(
				fs::metadata(&path).expect("config metadata must be readable").permissions().mode()
					& 0o777,
				0o600
			);
		}
	}

	#[test]
	fn invalid_shapes_fail_without_changing_the_file() {
		for (input, expected) in [
			("features = true\n", super::FastModeFailure::FeaturesNotTable),
			("[features]\nfast_mode = \"yes\"\n", super::FastModeFailure::FastModeNotBoolean),
			("[features", super::FastModeFailure::ConfigInvalid),
		] {
			let temp = tempfile::tempdir().expect("temporary directory must be created");
			let path = config_path(&temp);

			fs::create_dir(path.parent().expect("config must have a parent"))
				.expect("Codex directory must be created");
			fs::write(&path, input).expect("seed config must be written");

			assert_eq!(super::set_at_path(&path, true), Err(expected));
			assert_eq!(fs::read_to_string(&path).expect("config must be readable"), input);
		}
	}

	#[test]
	fn oversized_input_is_rejected_before_parsing() {
		let temp = tempfile::tempdir().expect("temporary directory must be created");
		let path = config_path(&temp);

		fs::create_dir(path.parent().expect("config must have a parent"))
			.expect("Codex directory must be created");
		fs::write(&path, vec![b' '; super::MAX_CONFIG_BYTES as usize + 1])
			.expect("large config must be written");

		assert_eq!(super::status_at_path(&path), Err(super::FastModeFailure::ConfigTooLarge));
	}

	#[cfg(unix)]
	#[test]
	fn symlinked_config_and_writable_parent_are_rejected() {
		let temp = tempfile::tempdir().expect("temporary directory must be created");
		let target = temp.path().join("target.toml");
		let path = config_path(&temp);

		fs::create_dir(path.parent().expect("config must have a parent"))
			.expect("Codex directory must be created");
		fs::write(&target, "[features]\nfast_mode = false\n")
			.expect("target config must be written");
		symlink(&target, &path).expect("config symlink must be created");

		assert_eq!(super::status_at_path(&path), Err(super::FastModeFailure::UnsafeConfigPath));
		fs::remove_file(&path).expect("config symlink must be removed");

		let mut permissions = fs::metadata(path.parent().expect("config must have a parent"))
			.expect("Codex directory metadata must be readable")
			.permissions();
		permissions.set_mode(0o777);
		fs::set_permissions(path.parent().expect("config must have a parent"), permissions)
			.expect("Codex directory permissions must change");

		assert_eq!(super::set_at_path(&path, true), Err(super::FastModeFailure::UnsafeConfigPath));
	}

	#[test]
	fn home_path_is_exact_and_empty_home_is_rejected() {
		assert_eq!(
			super::codex_config_path(Some(OsString::from("/Users/example")))
				.expect("home must resolve"),
			Path::new("/Users/example/.codex/config.toml")
		);
		assert_eq!(
			super::codex_config_path(Some(OsString::new())),
			Err(super::FastModeFailure::HomeUnavailable)
		);
		assert_eq!(super::codex_config_path(None), Err(super::FastModeFailure::HomeUnavailable));
	}
}
