//! Shared Codex user configuration. Thread overrides have a separate owner.

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::{
	env,
	ffi::OsString,
	fmt::{Display, Formatter},
	fs::{self, DirBuilder, File, OpenOptions},
	io::{Error, ErrorKind, Read, Write as _},
	path::{Path, PathBuf},
	process,
};

use serde::Serialize;
use toml_edit::{DocumentMut, Item, Table};

use crate::ServiceTier;

const MAX_CONFIG_BYTES: u64 = 1_048_576;
const TEMP_FILE_ATTEMPTS: u8 = 16;

/// Closed configuration failures without file paths or parser excerpts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexConfigError {
	/// The user home is absent.
	HomeUnavailable,
	/// The configuration path is not safe to use.
	UnsafeConfigPath,
	/// The configuration cannot be read.
	ConfigUnavailable,
	/// The configuration exceeds the byte limit.
	ConfigTooLarge,
	/// The configuration is not valid TOML.
	ConfigInvalid,
	/// The features value is not a table.
	FeaturesNotTable,
	/// The Fast feature value is not a Boolean.
	FastModeNotBoolean,
	/// The shared service tier is not a valid string identifier.
	ServiceTierInvalid,
	/// The configuration update did not complete.
	WriteFailed,
}
impl Display for CodexConfigError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::HomeUnavailable => "the current user home is unavailable",
			Self::UnsafeConfigPath => "the Codex config path is unsafe",
			Self::ConfigUnavailable => "the Codex config is unavailable",
			Self::ConfigTooLarge => "the Codex config exceeds the supported size",
			Self::ConfigInvalid => "the Codex config is invalid TOML",
			Self::FeaturesNotTable => "`features` is not a TOML table",
			Self::FastModeNotBoolean => "`features.fast_mode` is not a Boolean",
			Self::ServiceTierInvalid => "`service_tier` is not a valid speed",
			Self::WriteFailed => "the Codex config could not be updated",
		})
	}
}

/// Read the saved speed from the same user configuration as the native runtime.
/// An absent preference uses standard speed. This does not select a thread override.
pub fn codex_default_service_tier() -> Result<ServiceTier, CodexConfigError> {
	service_tier_at_path(&codex_config_path(env::var_os("HOME"))?)
}

/// Save the shared speed without changing models, features, profiles or comments.
/// Native project and managed configuration precedence still applies to new conversations.
pub fn set_codex_default_service_tier(tier: &ServiceTier) -> Result<(), CodexConfigError> {
	set_service_tier_at_path(&codex_config_path(env::var_os("HOME"))?, tier)
}

fn service_tier_at_path(path: &Path) -> Result<ServiceTier, CodexConfigError> {
	let document = read_document(path)?;
	let Some(value) = document.get("service_tier") else { return Ok(ServiceTier::standard()) };
	let value = value.as_str().ok_or(CodexConfigError::ServiceTierInvalid)?;
	ServiceTier::new(if value == "fast" { "priority" } else { value })
		.map_err(|_| CodexConfigError::ServiceTierInvalid)
}

fn set_service_tier_at_path(path: &Path, tier: &ServiceTier) -> Result<(), CodexConfigError> {
	let mut document = read_document(path)?;
	// Match upstream ConfigEdit::SetServiceTier: config spells priority as fast.
	let value = if tier.as_str() == "priority" { "fast" } else { tier.as_str() };
	let mut replacement = toml_edit::Value::from(value);
	if let Some(current) = document.get("service_tier").and_then(Item::as_value) {
		*replacement.decor_mut() = current.decor().clone();
	}
	document["service_tier"] = Item::Value(replacement);
	write_document(path, &document)
}

/// Read the global Fast feature gate from the normal user configuration.
/// An absent value inherits the native default, which enables the feature.
pub fn global_fast_mode_enabled() -> Result<bool, CodexConfigError> {
	status_at_path(&codex_config_path(env::var_os("HOME"))?)
}

/// Set the global Fast feature gate while preserving unrelated settings and comments.
/// This does not select a service tier for any thread.
pub fn set_global_fast_mode_enabled(enabled: bool) -> Result<bool, CodexConfigError> {
	set_at_path(&codex_config_path(env::var_os("HOME"))?, enabled)
}

fn codex_config_path(home: Option<OsString>) -> Result<PathBuf, CodexConfigError> {
	let home = home.filter(|value| !value.is_empty()).ok_or(CodexConfigError::HomeUnavailable)?;

	Ok(PathBuf::from(home).join(".codex").join("config.toml"))
}

fn status_at_path(path: &Path) -> Result<bool, CodexConfigError> {
	read_fast_mode(&read_document(path)?)
}

fn set_at_path(path: &Path, enabled: bool) -> Result<bool, CodexConfigError> {
	let mut document = read_document(path)?;
	let root = document.as_table_mut();

	if !root.contains_key("features") {
		root.insert("features", Item::Table(Table::new()));
	}

	let features = root
		.get_mut("features")
		.and_then(Item::as_table_like_mut)
		.ok_or(CodexConfigError::FeaturesNotTable)?;

	if let Some(value) = features.get("fast_mode")
		&& !value.is_none()
		&& value.as_bool().is_none()
	{
		return Err(CodexConfigError::FastModeNotBoolean);
	}

	features.insert("fast_mode", toml_edit::value(enabled));

	write_document(path, &document)?;

	Ok(enabled)
}

fn read_document(path: &Path) -> Result<DocumentMut, CodexConfigError> {
	match fs::symlink_metadata(path) {
		Ok(metadata) => {
			if metadata.file_type().is_symlink() || !metadata.is_file() {
				return Err(CodexConfigError::UnsafeConfigPath);
			}
			if metadata.len() > MAX_CONFIG_BYTES {
				return Err(CodexConfigError::ConfigTooLarge);
			}
		},
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(DocumentMut::new()),
		Err(_) => return Err(CodexConfigError::ConfigUnavailable),
	}

	let mut file = open_config_no_follow(path).map_err(map_config_open_error)?;
	let metadata = file.metadata().map_err(|_| CodexConfigError::ConfigUnavailable)?;

	if !metadata.is_file() {
		return Err(CodexConfigError::UnsafeConfigPath);
	}
	if metadata.len() > MAX_CONFIG_BYTES {
		return Err(CodexConfigError::ConfigTooLarge);
	}

	let mut bytes = Vec::new();

	Read::by_ref(&mut file)
		.take(MAX_CONFIG_BYTES + 1)
		.read_to_end(&mut bytes)
		.map_err(|_| CodexConfigError::ConfigUnavailable)?;

	if bytes.len() as u64 > MAX_CONFIG_BYTES {
		return Err(CodexConfigError::ConfigTooLarge);
	}

	let input = String::from_utf8(bytes).map_err(|_| CodexConfigError::ConfigUnavailable)?;

	if input.trim().is_empty() {
		return Ok(DocumentMut::new());
	}

	input.parse().map_err(|_| CodexConfigError::ConfigInvalid)
}

fn open_config_no_follow(path: &Path) -> Result<File, Error> {
	let mut options = OpenOptions::new();

	options.read(true);
	#[cfg(unix)]
	options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);

	options.open(path)
}

fn map_config_open_error(error: Error) -> CodexConfigError {
	if error.kind() == ErrorKind::NotFound {
		CodexConfigError::ConfigUnavailable
	} else if is_symlink_open_error(&error) {
		CodexConfigError::UnsafeConfigPath
	} else {
		CodexConfigError::ConfigUnavailable
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

fn read_fast_mode(document: &DocumentMut) -> Result<bool, CodexConfigError> {
	let Some(features) = document.get("features") else {
		return Ok(true);
	};
	let features = features.as_table_like().ok_or(CodexConfigError::FeaturesNotTable)?;
	let Some(value) = features.get("fast_mode") else {
		return Ok(true);
	};

	value.as_bool().ok_or(CodexConfigError::FastModeNotBoolean)
}

fn write_document(path: &Path, document: &DocumentMut) -> Result<(), CodexConfigError> {
	let parent = path.parent().ok_or(CodexConfigError::UnsafeConfigPath)?;

	prepare_parent(parent)?;

	let file_name = path
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or(CodexConfigError::UnsafeConfigPath)?;
	let mut output = document.to_string();

	if !output.ends_with('\n') {
		output.push('\n');
	}
	if output.len() as u64 > MAX_CONFIG_BYTES {
		return Err(CodexConfigError::ConfigTooLarge);
	}

	for attempt in 0..TEMP_FILE_ATTEMPTS {
		let temporary = parent.join(format!(".{file_name}.decodex-{}-{attempt}", process::id()));
		let Some(mut file) = create_private_file(&temporary)? else {
			continue;
		};
		let write_result = (|| {
			file.write_all(output.as_bytes()).map_err(|_| CodexConfigError::WriteFailed)?;
			file.sync_all().map_err(|_| CodexConfigError::WriteFailed)?;

			drop(file);

			fs::rename(&temporary, path).map_err(|_| CodexConfigError::WriteFailed)?;

			sync_directory(parent)
		})();

		if write_result.is_err() {
			let _ = fs::remove_file(&temporary);
		}

		return write_result;
	}

	Err(CodexConfigError::WriteFailed)
}

fn prepare_parent(parent: &Path) -> Result<(), CodexConfigError> {
	match fs::symlink_metadata(parent) {
		Ok(metadata) =>
			if metadata.file_type().is_symlink() || !metadata.is_dir() {
				return Err(CodexConfigError::UnsafeConfigPath);
			},
		Err(error) if error.kind() == ErrorKind::NotFound => {
			let mut builder = DirBuilder::new();

			#[cfg(unix)]
			builder.mode(0o700);
			builder.create(parent).map_err(|_| CodexConfigError::WriteFailed)?;
		},
		Err(_) => return Err(CodexConfigError::ConfigUnavailable),
	}

	#[cfg(unix)]
	{
		let metadata =
			fs::symlink_metadata(parent).map_err(|_| CodexConfigError::ConfigUnavailable)?;

		if metadata.file_type().is_symlink()
			|| !metadata.is_dir()
			|| metadata.permissions().mode() & 0o022 != 0
		{
			return Err(CodexConfigError::UnsafeConfigPath);
		}
	}

	Ok(())
}

fn create_private_file(path: &Path) -> Result<Option<File>, CodexConfigError> {
	let mut options = OpenOptions::new();

	options.write(true).create_new(true);
	#[cfg(unix)]
	options.mode(0o600).custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);

	match options.open(path) {
		Ok(file) => Ok(Some(file)),
		Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(None),
		Err(_) => Err(CodexConfigError::WriteFailed),
	}
}

fn sync_directory(path: &Path) -> Result<(), CodexConfigError> {
	let mut options = OpenOptions::new();

	options.read(true);
	#[cfg(unix)]
	options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY);

	options
		.open(path)
		.and_then(|directory| directory.sync_all())
		.map_err(|_| CodexConfigError::WriteFailed)
}

#[cfg(test)]
mod tests {
	#[cfg(unix)] use std::os::unix::fs::PermissionsExt as _;
	use std::{
		ffi::OsString,
		fs,
		path::{Path, PathBuf},
	};

	fn config_path(temp: &tempfile::TempDir) -> PathBuf {
		temp.path().join(".codex").join("config.toml")
	}

	#[test]
	fn shared_speed_edits_preserve_native_spelling_and_other_settings() {
		let temp = tempfile::tempdir().unwrap();
		let path = config_path(&temp);
		fs::create_dir(path.parent().unwrap()).unwrap();
		let input = "# shared defaults\nmodel = \"chosen-model\"\nservice_tier = \"flex\" # speed\n\n[features]\nasync_compaction = true\nfast_mode = true\n\n[profiles.work]\nmodel = \"project-model\"\n";
		for (request, config) in [
			("default", "default"),
			("priority", "fast"),
			("ultrafast", "ultrafast"),
			("flex", "flex"),
		] {
			fs::write(&path, input).unwrap();
			let tier = crate::ServiceTier::new(request).unwrap();
			super::set_service_tier_at_path(&path, &tier).unwrap();
			assert_eq!(super::service_tier_at_path(&path).unwrap(), tier);
			assert_eq!(
				fs::read_to_string(&path).unwrap(),
				input.replacen(
					"service_tier = \"flex\"",
					&format!("service_tier = \"{config}\""),
					1
				)
			);
		}
	}

	#[test]
	fn absent_shared_speed_is_standard_without_creating_a_config() {
		let temp = tempfile::tempdir().unwrap();
		let path = config_path(&temp);
		assert_eq!(super::service_tier_at_path(&path).unwrap(), crate::ServiceTier::standard());
		assert!(!path.exists());
	}

	#[test]
	fn invalid_speed_is_explicit_and_invalid_toml_is_never_overwritten() {
		let temp = tempfile::tempdir().unwrap();
		let path = config_path(&temp);
		fs::create_dir(path.parent().unwrap()).unwrap();
		fs::write(&path, "service_tier = 7\n").unwrap();
		assert_eq!(
			super::service_tier_at_path(&path),
			Err(super::CodexConfigError::ServiceTierInvalid)
		);
		fs::write(&path, "[broken").unwrap();
		assert_eq!(
			super::set_service_tier_at_path(&path, &crate::ServiceTier::standard()),
			Err(super::CodexConfigError::ConfigInvalid)
		);
		assert_eq!(fs::read_to_string(&path).unwrap(), "[broken");
	}

	#[test]
	fn absent_fast_mode_inherits_native_default_without_writing_config() {
		for input in
			[None, Some(""), Some("model = \"gpt-5.6\"\n"), Some("[features]\nplugins = true\n")]
		{
			let temp = tempfile::tempdir().expect("temporary directory");
			let path = config_path(&temp);

			if let Some(input) = input {
				fs::create_dir(path.parent().unwrap()).unwrap();
				fs::write(&path, input).unwrap();
			}

			assert!(super::status_at_path(&path).expect("native default is enabled"));
			assert_eq!(fs::read_to_string(&path).ok().as_deref(), input);
		}
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

		assert!(
			!super::status_at_path(&path).expect("explicit false overrides the native default")
		);
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
					.mode()
					& 0o777,
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
			("features = true\n", super::CodexConfigError::FeaturesNotTable),
			("[features]\nfast_mode = \"yes\"\n", super::CodexConfigError::FastModeNotBoolean),
			("[features", super::CodexConfigError::ConfigInvalid),
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

		assert_eq!(super::status_at_path(&path), Err(super::CodexConfigError::ConfigTooLarge));
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
		std::os::unix::fs::symlink(&target, &path).expect("config symlink must be created");

		assert_eq!(super::status_at_path(&path), Err(super::CodexConfigError::UnsafeConfigPath));

		fs::remove_file(&path).expect("config symlink must be removed");

		let mut permissions = fs::metadata(path.parent().expect("config must have a parent"))
			.expect("Codex directory metadata must be readable")
			.permissions();

		permissions.set_mode(0o777);

		fs::set_permissions(path.parent().expect("config must have a parent"), permissions)
			.expect("Codex directory permissions must change");

		assert_eq!(super::set_at_path(&path, true), Err(super::CodexConfigError::UnsafeConfigPath));
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
			Err(super::CodexConfigError::HomeUnavailable)
		);
		assert_eq!(super::codex_config_path(None), Err(super::CodexConfigError::HomeUnavailable));
	}
}
