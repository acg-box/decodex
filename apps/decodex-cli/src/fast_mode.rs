//! Local Codex Fast mode commands with bounded typed output.

use clap::{Args, Subcommand};
use serde::Serialize;

use crate::{CommandOutput, OutputFormat};
use decodex_protocol::FastModeFailure;

const FAST_MODE_OUTPUT_SCHEMA: &str = "decodex/fast-mode-cli/1";

/// Local Codex Fast mode operations.
#[derive(Clone, Debug, Eq, PartialEq, Subcommand)]
pub enum FastModeCommand {
	/// Read `[features].fast_mode` from the current user's Codex configuration.
	Status,
	/// Set `[features].fast_mode` without changing unrelated Codex configuration.
	Set(SetArgs),
}
impl FastModeCommand {
	const fn name(&self) -> &'static str {
		match self {
			Self::Status => "status",
			Self::Set(_) => "set",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct SetArgs {
	/// Enable or disable Codex Fast mode.
	#[arg(long, action = clap::ArgAction::Set)]
	enabled: bool,
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
	let result = match command {
		FastModeCommand::Status => decodex_protocol::global_fast_mode_enabled(),
		FastModeCommand::Set(args) => decodex_protocol::set_global_fast_mode_enabled(args.enabled),
	};

	match result {
		Ok(enabled) => render_success(command_name, format, enabled),
		Err(error) => render_failure(command_name, format, error),
	}
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
	use clap::Parser as _;

	use crate::{
		Cli, Command, OutputFormat,
		fast_mode::{self, FastModeCommand, SetArgs},
	};
	use decodex_protocol::FastModeFailure;
	#[test]
	fn command_surface_requires_an_explicit_set_boolean() {
		let status = Cli::try_parse_from(["decodex", "fast-mode", "status", "--output", "json"])
			.expect("status command must parse");
		let enabled = Cli::try_parse_from(["decodex", "fast-mode", "set", "--enabled", "true"])
			.expect("set command must parse");

		assert!(matches!(status.command, Command::FastMode(FastModeCommand::Status)));
		assert_eq!(status.output, OutputFormat::Json);
		assert!(matches!(
			enabled.command,
			Command::FastMode(FastModeCommand::Set(SetArgs { enabled: true }))
		));
		assert!(Cli::try_parse_from(["decodex", "fast-mode", "set"]).is_err());
		assert!(Cli::try_parse_from(["decodex", "fast-mode", "set", "--enabled"]).is_err());
	}
	#[test]
	fn output_is_stable_bounded_and_path_free() {
		let enabled = fast_mode::render_success("set", OutputFormat::Json, true);
		let failure =
			fast_mode::render_failure("status", OutputFormat::Json, FastModeFailure::ConfigInvalid);

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
	}
}
