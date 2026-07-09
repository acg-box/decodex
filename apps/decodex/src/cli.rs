mod account_commands;
mod app_command;
mod attempt_command;
mod control_commands;
mod docs_commands;
mod git_hook_commands;
mod manual_commands;
mod probe_command;
mod recovery_commands;
mod research_intake_commands;
mod verify_commands;

pub(crate) use self::attempt_command::AttemptRequest;

#[cfg(test)] use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::{
	Args, Parser, Subcommand,
	builder::{
		Styles,
		styling::{AnsiColor, Effects},
	},
};

use self::{
	account_commands::AccountCommand,
	app_command::AppCommand,
	attempt_command::AttemptCommand,
	control_commands::{
		DiagnoseCommand, EvidenceCommand, LaneCommand, McpCommand, ProjectCommand, RunCommand,
		ServeCommand, StatusCommand,
	},
	docs_commands::DocsCommand,
	git_hook_commands::GitHookCommand,
	manual_commands::{CommitCommand, LandCommand},
	probe_command::ProbeCommand,
	recovery_commands::RecoverCommand,
	research_intake_commands::{ArchiveLinearCommand, IntakeCommand, MaintenanceCommand},
	verify_commands::VerifyCommand,
};
use crate::prelude::Result;

/// Root CLI parser for the Decodex control plane.
#[derive(Debug, Parser)]
#[command(
	about = "Repo-native orchestration for autonomous coding agents.",
	version = concat!(
		env!("CARGO_PKG_VERSION"),
		"-",
		env!("VERGEN_GIT_SHA"),
		"-",
		env!("VERGEN_CARGO_TARGET_TRIPLE"),
	),
	arg_required_else_help = true,
	rename_all = "kebab",
	subcommand_required = true,
	styles = styles(),
)]
pub(crate) struct Cli {
	#[command(subcommand)]
	command: Command,
}
impl Cli {
	pub(crate) fn run(&self) -> Result<()> {
		match &self.command {
			Command::App(args) => args.run(),
			Command::Commit(args) => args.run(),
			Command::GitHook(args) => args.run(),
			Command::Land(args) => args.run(),
			Command::Run(args) => args.run(),
			Command::Serve(args) => args.run(),
			Command::Mcp(args) => args.run(),
			Command::Project(args) => args.run(),
			Command::Lane(args) => args.run(),
			Command::Status(args) => args.run(),
			Command::Diagnose(args) => args.run(),
			Command::Docs(args) => args.run(),
			Command::Evidence(args) => args.run(),
			Command::Intake(args) => args.run(),
			Command::Recover(args) => args.run(),
			Command::ArchiveLinear(args) => args.run(),
			Command::Maintenance(args) => args.run(),
			Command::Account(args) => args.run(),
			Command::Probe(args) => args.run(),
			Command::Verify(args) => args.run(),
			Command::Attempt(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
struct ProjectConfigArgs {
	/// Use this Decodex project directory or `project.toml` instead of resolving from the
	/// registered current checkout.
	#[arg(short = 'c', long, value_name = "PROJECT_DIR")]
	config: Option<PathBuf>,
}
impl ProjectConfigArgs {
	fn as_path(&self) -> Option<&Path> {
		self.config.as_deref()
	}
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
enum Command {
	/// Open the native Decodex App.
	App(AppCommand),
	/// Create a signed local commit with a `decodex/commit/2` message.
	Commit(CommitCommand),
	/// Validate Git hook inputs with Decodex-owned policy.
	GitHook(GitHookCommand),
	/// Land the current reviewed lane with a GitHub admin merge commit.
	Land(LandCommand),
	/// Run one orchestration pass.
	Run(RunCommand),
	/// Run the local multi-project Decodex control plane.
	Serve(ServeCommand),
	/// Serve the Decodex MCP gateway.
	Mcp(McpCommand),
	/// Manage the local Decodex project registry.
	Project(ProjectCommand),
	/// Inspect or influence a local lane.
	Lane(LaneCommand),
	/// Inspect the current local runtime state for one configured project.
	Status(StatusCommand),
	/// Write and print the agent-readable local evidence index.
	Diagnose(DiagnoseCommand),
	/// Check repository documentation readiness.
	Docs(DocsCommand),
	/// Inspect local-only private execution evidence for one issue or run.
	Evidence(EvidenceCommand),
	/// Operator issue-batch intake into internal Execution Programs, not a graph editor.
	Intake(IntakeCommand),
	/// Diagnose or explicitly repair supported retained-lane recovery cases.
	Recover(RecoverCommand),
	/// Dry-run or archive old terminal Linear issues by repo label.
	ArchiveLinear(ArchiveLinearCommand),
	/// Maintain local Decodex logs, evidence, backups, and runtime storage.
	Maintenance(MaintenanceCommand),
	/// Manage the user-local Decodex Codex account pool.
	Account(AccountCommand),
	/// Validate the local app-server integration boundary.
	Probe(ProbeCommand),
	/// Publish or inspect Decodex validation evidence.
	Verify(VerifyCommand),
	/// Run one daemon-planned attempt from a structured request.
	#[command(name = "_attempt", hide = true)]
	Attempt(AttemptCommand),
}

fn styles() -> Styles {
	Styles::styled()
		.header(AnsiColor::Red.on_default() | Effects::BOLD)
		.usage(AnsiColor::Red.on_default() | Effects::BOLD)
		.literal(AnsiColor::Blue.on_default() | Effects::BOLD)
		.placeholder(AnsiColor::Green.on_default())
}

#[cfg(test)]
fn decodex_app_open_args(bundle: Option<&Path>, new: bool) -> Vec<OsString> {
	app_command::decodex_app_open_args(bundle, new)
}

#[cfg(test)] mod tests;
