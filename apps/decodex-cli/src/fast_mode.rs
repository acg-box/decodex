//! Local Codex Fast mode configuration with bounded typed output.

#[cfg(unix)] use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::{
	env,
	ffi::OsString,
	fmt::{Display, Formatter},
	fs::{self, DirBuilder, File},
	io::{ErrorKind, Write as _},
	path::{Path, PathBuf},
	process,
};

use clap::{Args, Subcommand};
use serde::Serialize;
use toml_edit::{DocumentMut, Item, Table};

use crate::{CommandOutput, OutputFormat};

const FAST_MODE_OUTPUT_SCHEMA: &str = "decodex/fast-mode-cli/1";
const MAX_CONFIG_BYTES: u64 = 1_048_576;
const TEMP_FILE_ATTEMPTS: u8 = 16;

/// Local Codex Fast mode operations.
#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum FastModeCommand {
	/// Read `[features].fast_mode` from the current user's Codex configuration.
	Status,
	/// Set `[features].fast_mode` without changing unrelated Codex configuration.
	Set(SetArgs),
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct SetArgs {
	/// Enable or disable Codex Fast mode.
	#[arg(long, action = clap::ArgAction::Set)]
	enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum FastModeFailure {
	HomeUnavailable,
	UnsafeConfigPath,
	ConfigUnavailable,
	ConfigTooLarge,
	ConfigInvalid,
	FeaturesNotTable,
	FastModeNotBoolean,
	WriteFailed,
}
impl Display for FastModeFailure {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		formatter.write_str(match self {
			Self::HomeUnavailable => "the current user home is unavailable",
			Self::UnsafeConfigPath => "the Codex config path is unsafe",
			Self::ConfigUnavailable => "the Codex config is unavailable",
			Self::ConfigTooLarge => "the Codex config exceeds the supported size",
			Self::ConfigInvalid => "the Codex config is invalid TOML",
			Self::FeaturesNotTable => "`features` is not a TOML table",
			Self::FastModeNotBoolean => "`features.fast_mode` is not a Boolean",
			Self::WriteFailed => "the Codex config could not be updated",
		})
	}
}

#[derive(Serialize)]
struct SuccessDocument {
	schema: &'static str,
	command: &'static str,
	outcome: &'static str,
	enabled: bool,
}

#[derive(Serialize)]
struct FailureDocument {
	schema: &'static str,
	command: &'static str,
	outcome: &'static str,
	error: FastModeFailure,
}

pub(crate) fn execute(command: FastModeCommand, format: OutputFormat) -> CommandOutput {
	let command_name = command.name();
	let path = match codex_config_path(env::var_os("HOME")) {
		Ok(path) => path,
		Err(error) => return render_failure(command_name, format, error),
	};
	let result = match command {
		FastModeCommand::Status => status_at_path(&path),
		FastModeCommand::Set(args) => set_at_path(&path, args.enabled),
	};

	match result {
		Ok(enabled) => render_success(command_name, format, enabled),
		Err(error) => render_failure(command_name, format, error),
	}
}

impl FastModeCommand {
	const fn name(&self) -> &'static str {
		match self {
			Self::Status => "status",
			Self::Set(_) => "set",
		}
	}
}

fn codex_config_path(home: Option<OsString>) -> Result<PathBuf, FastModeFailure> {
	let home = home.filter(|value| !value.is_empty()).ok_or(FastModeFailure::HomeUnavailable)?;

	Ok(PathBuf::from(home).join(".codex").join("config.toml"))
}

fn status_at_path(path: &Path) -> Result<bool, FastModeFailure> {
	let document = read_document(path)?;

	read_fast_mode(&document)
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
	let metadata = match fs::symlink_metadata(path) {
		Ok(metadata) => {
			if metadata.file_type().is_symlink() || !metadata.is_file() {
				return Err(FastModeFailure::UnsafeConfigPath);
			}
			Some(metadata)
		},
		Err(error) if error.kind() == ErrorKind::NotFound => None,
		Err(_) => return Err(FastModeFailure::ConfigUnavailable),
	};
	if metadata.as_ref().is_some_and(|metadata| metadata.len() > MAX_CONFIG_BYTES) {
		return Err(FastModeFailure::ConfigTooLarge);
	}

	let input = match fs::read_to_string(path) {
		Ok(input) => input,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(DocumentMut::new()),
		Err(_) => return Err(FastModeFailure::ConfigUnavailable),
	};
	if input.len() as u64 > MAX_CONFIG_BYTES {
		return Err(FastModeFailure::ConfigTooLarge);
	}
	if input.trim().is_empty() {
		return Ok(DocumentMut::new());
	}

	input.parse().map_err(|_| FastModeFailure::ConfigInvalid)
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
			secure_file(path)?;
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
		let metadata = fs::metadata(parent).map_err(|_| FastModeFailure::ConfigUnavailable)?;
		if metadata.permissions().mode() & 0o022 != 0 {
			return Err(FastModeFailure::UnsafeConfigPath);
		}
	}

	Ok(())
}

fn create_private_file(path: &Path) -> Result<Option<File>, FastModeFailure> {
	let mut options = File::options();

	options.write(true).create_new(true);

	match options.open(path) {
		Ok(file) => {
			if let Err(error) = secure_file(path) {
				drop(file);
				let _ = fs::remove_file(path);

				return Err(error);
			}

			Ok(Some(file))
		},
		Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(None),
		Err(_) => Err(FastModeFailure::WriteFailed),
	}
}

fn secure_file(path: &Path) -> Result<(), FastModeFailure> {
	#[cfg(unix)]
	{
		let mut permissions =
			fs::metadata(path).map_err(|_| FastModeFailure::WriteFailed)?.permissions();

		permissions.set_mode(0o600);
		fs::set_permissions(path, permissions).map_err(|_| FastModeFailure::WriteFailed)?;
	}

	Ok(())
}

fn sync_directory(path: &Path) -> Result<(), FastModeFailure> {
	File::open(path)
		.and_then(|directory| directory.sync_all())
		.map_err(|_| FastModeFailure::WriteFailed)
}

fn render_success(command: &'static str, format: OutputFormat, enabled: bool) -> CommandOutput {
	let text = match format {
		OutputFormat::Human =>
			format!("Codex Fast mode: {}", if enabled { "enabled" } else { "disabled" }),
		OutputFormat::Json => serde_json::to_string(&SuccessDocument {
			schema: FAST_MODE_OUTPUT_SCHEMA,
			command,
			outcome: "success",
			enabled,
		})
		.expect("bounded Fast mode success serialization cannot fail"),
	};

	CommandOutput { text, exit_code: 0, error_stream: false }
}

fn render_failure(
	command: &'static str,
	format: OutputFormat,
	error: FastModeFailure,
) -> CommandOutput {
	let (text, error_stream) = match format {
		OutputFormat::Human => (format!("decodex fast-mode {command} failed: {error}"), true),
		OutputFormat::Json => (
			serde_json::to_string(&FailureDocument {
				schema: FAST_MODE_OUTPUT_SCHEMA,
				command,
				outcome: "failure",
				error,
			})
			.expect("bounded Fast mode failure serialization cannot fail"),
			false,
		),
	};

	CommandOutput { text, exit_code: 2, error_stream }
}

#[cfg(test)]
mod tests {
	#[cfg(unix)] use std::os::unix::fs::{PermissionsExt as _, symlink};
	use std::{fs, path::Path};

	use crate::OutputFormat;

	fn config_path(temp: &tempfile::TempDir) -> std::path::PathBuf {
		temp.path().join(".codex").join("config.toml")
	}

	fn json(output: &crate::CommandOutput) -> serde_json::Value {
		serde_json::from_str(output.text()).expect("Fast mode output must be valid JSON")
	}

	#[test]
	fn command_surface_requires_an_explicit_set_boolean() {
		use clap::Parser as _;

		let status =
			crate::Cli::try_parse_from(["decodex", "fast-mode", "status", "--output", "json"])
				.expect("status command must parse");
		let enabled =
			crate::Cli::try_parse_from(["decodex", "fast-mode", "set", "--enabled", "true"])
				.expect("set command must parse");

		assert!(matches!(status.command, crate::Command::FastMode(super::FastModeCommand::Status)));
		assert_eq!(status.output, OutputFormat::Json);
		assert!(matches!(
			enabled.command,
			crate::Command::FastMode(super::FastModeCommand::Set(super::SetArgs { enabled: true }))
		));
		assert!(crate::Cli::try_parse_from(["decodex", "fast-mode", "set"]).is_err());
		assert!(crate::Cli::try_parse_from(["decodex", "fast-mode", "set", "--enabled"]).is_err());
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
	fn output_is_stable_bounded_and_path_free() {
		let enabled = super::render_success("set", OutputFormat::Json, true);
		let failure = super::render_failure(
			"status",
			OutputFormat::Json,
			super::FastModeFailure::ConfigInvalid,
		);

		assert_eq!(
			enabled.text(),
			r#"{"schema":"decodex/fast-mode-cli/1","command":"set","outcome":"success","enabled":true}"#
		);
		assert_eq!(
			failure.text(),
			r#"{"schema":"decodex/fast-mode-cli/1","command":"status","outcome":"failure","error":"config_invalid"}"#
		);
		assert_eq!(enabled.exit_code(), 0);
		assert_eq!(failure.exit_code(), 2);
		assert!(!enabled.is_error_stream());
		assert!(!failure.is_error_stream());
		assert!(enabled.text().len() < 128);
		assert!(failure.text().len() < 160);
		assert!(
			!json(&enabled).as_object().expect("document must be an object").contains_key("path")
		);
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

	use std::ffi::OsString;
}
