mod account_commands;
mod control_commands;
mod docs_okf_commands;
mod manual_commands;
mod radar_commands;
mod recovery_commands;
mod research_intake_commands;

use self::{
	account_commands::AccountCommand,
	control_commands::{
		DiagnoseCommand, EvidenceCommand, LaneCommand, McpCommand, ProjectCommand, RunCommand,
		ServeCommand, StatusCommand,
	},
	docs_okf_commands::{DocsCommand, OkfCommand},
	manual_commands::{CommitCommand, LandCommand},
	radar_commands::RadarCommand,
	recovery_commands::RecoverCommand,
	research_intake_commands::{
		ArchiveLinearCommand, IntakeCommand, MaintenanceCommand, ResearchCommand,
	},
};
use std::{
	fs,
	io::{self, Read as _},
	path::{Path, PathBuf},
};

use clap::{
	Args, Parser, Subcommand,
	builder::{
		Styles,
		styling::{AnsiColor, Effects},
	},
};
use serde::{Deserialize, Serialize};

use crate::{
	agent,
	orchestrator::{self, IssueDispatchMode, RunOnceRequest},
	prelude::{Result, eyre},
};

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
			Command::Land(args) => args.run(),
			Command::Run(args) => args.run(),
			Command::Serve(args) => args.run(),
			Command::Mcp(args) => args.run(),
			Command::Project(args) => args.run(),
			Command::Lane(args) => args.run(),
			Command::Status(args) => args.run(),
			Command::Diagnose(args) => args.run(),
			Command::Evidence(args) => args.run(),
			Command::Docs(args) => args.run(),
			Command::Okf(args) => args.run(),
			Command::Research(args) => args.run(),
			Command::Radar(args) => args.run(),
			Command::Intake(args) => args.run(),
			Command::Recover(args) => args.run(),
			Command::ArchiveLinear(args) => args.run(),
			Command::Maintenance(args) => args.run(),
			Command::Account(args) => args.run(),
			Command::Probe(args) => args.run(),
			Command::Attempt(args) => args.run(),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct AttemptRequest {
	#[serde(default)]
	pub(crate) dry_run: bool,
	pub(crate) issue_id: String,
	pub(crate) issue_state: String,
	pub(crate) initial_issue_state: Option<String>,
	#[serde(default)]
	pub(crate) lease_preacquired: bool,
	pub(crate) issue_claim_fd: Option<i32>,
	pub(crate) dispatch_slot_fd: Option<i32>,
	pub(crate) dispatch_slot_index: Option<usize>,
	pub(crate) dispatch_mode: AttemptDispatchMode,
	pub(crate) run_id: String,
	pub(crate) attempt_number: i64,
	pub(crate) retry_budget_base: i64,
	pub(crate) workflow_snapshot: String,
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

#[derive(Debug, Args)]
struct AppCommand {
	/// Open this Decodex app bundle instead of the installed `Decodex` app.
	#[arg(long, value_name = "APP_BUNDLE")]
	bundle: Option<PathBuf>,
	/// Ask LaunchServices to open a new app instance.
	#[arg(short = 'n', long)]
	new: bool,
}
impl AppCommand {
	fn run(&self) -> Result<()> {
		open_decodex_app(self.bundle.as_deref(), self.new)
	}
}

#[derive(Debug, Args)]
struct ProbeCommand {
	/// Override the expected app-server transport during probing.
	#[arg(value_name = "TRANSPORT", default_value = "stdio://")]
	transport: String,
}
impl ProbeCommand {
	fn run(&self) -> Result<()> {
		let report = agent::probe_app_server(&self.transport)?;

		println!(
			"probe ok: preflight_checks={} thread={} turn={} events={} output={}",
			report.capability_preflight.check_count(),
			report.thread_id,
			report.turn_id,
			report.event_count,
			report.final_output
		);

		tracing::info!(
			user_agent = %report.user_agent,
			thread_id = %report.thread_id,
			turn_id = %report.turn_id,
			event_count = report.event_count,
			"Completed probe."
		);

		Ok(())
	}
}

#[derive(Debug, Args)]
struct AttemptCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Structured request file path, or `-` to read the request from stdin.
	#[arg(value_name = "REQUEST", default_value = "-")]
	request: String,
}
impl AttemptCommand {
	fn run(&self) -> Result<()> {
		let request = read_attempt_request(&self.request)?;

		orchestrator::run_once(RunOnceRequest {
			config_path: self.project_config.as_path(),
			dry_run: request.dry_run,
			explain_queue: false,
			preferred_issue_id: Some(request.issue_id.as_str()),
			preferred_issue_state: Some(request.issue_state.as_str()),
			preferred_initial_issue_state: request.initial_issue_state.as_deref(),
			preferred_lease_acquired: request.lease_preacquired,
			preferred_issue_claim_fd: request.issue_claim_fd,
			preferred_dispatch_slot_fd: request.dispatch_slot_fd,
			preferred_dispatch_slot_index: request.dispatch_slot_index,
			preferred_dispatch_mode: Some(request.dispatch_mode.into()),
			preferred_run_id: Some(request.run_id.as_str()),
			preferred_attempt_number: Some(request.attempt_number),
			preferred_retry_budget_base: Some(request.retry_budget_base),
			preferred_workflow_snapshot: Some(request.workflow_snapshot.as_str()),
		})
	}
}

impl From<AttemptDispatchMode> for IssueDispatchMode {
	fn from(value: AttemptDispatchMode) -> Self {
		match value {
			AttemptDispatchMode::Normal => Self::Normal,
			AttemptDispatchMode::Program => Self::Program,
			AttemptDispatchMode::Retry => Self::Retry,
			AttemptDispatchMode::ReviewRepair => Self::ReviewRepair,
			AttemptDispatchMode::Closeout => Self::Closeout,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AttemptDispatchMode {
	Normal,
	Program,
	Retry,
	ReviewRepair,
	Closeout,
}
impl From<IssueDispatchMode> for AttemptDispatchMode {
	fn from(value: IssueDispatchMode) -> Self {
		match value {
			IssueDispatchMode::Normal => Self::Normal,
			IssueDispatchMode::Program => Self::Program,
			IssueDispatchMode::Retry => Self::Retry,
			IssueDispatchMode::ReviewRepair => Self::ReviewRepair,
			IssueDispatchMode::Closeout => Self::Closeout,
		}
	}
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
enum Command {
	/// Open the native Decodex App.
	App(AppCommand),
	/// Create a signed local commit with a `decodex/commit/1` message.
	Commit(CommitCommand),
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
	/// Inspect local-only private execution evidence for one issue or run.
	Evidence(EvidenceCommand),
	/// Validate the repo docs as a Markdown-only OKF knowledge bundle.
	Docs(DocsCommand),
	/// Inspect portable OKF bundles.
	Okf(OkfCommand),
	/// Compile or promote Decodex-native research/design contracts.
	Research(ResearchCommand),
	/// Run Decodex Radar automation commands.
	Radar(RadarCommand),
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
	/// Run one daemon-planned attempt from a structured request.
	#[command(name = "_attempt", hide = true)]
	Attempt(AttemptCommand),
}

fn read_attempt_request(request: &str) -> Result<AttemptRequest> {
	let raw = if request == "-" {
		let mut raw = String::new();

		io::stdin().read_to_string(&mut raw)?;

		raw
	} else {
		fs::read_to_string(request)?
	};

	serde_json::from_str(&raw).map_err(|error| {
		eyre::eyre!("Failed to parse `_attempt` request from `{}`: {error}", request)
	})
}

fn styles() -> Styles {
	Styles::styled()
		.header(AnsiColor::Red.on_default() | Effects::BOLD)
		.usage(AnsiColor::Red.on_default() | Effects::BOLD)
		.literal(AnsiColor::Blue.on_default() | Effects::BOLD)
		.placeholder(AnsiColor::Green.on_default())
}

#[cfg(any(target_os = "macos", test))]
fn decodex_app_open_args(bundle: Option<&Path>, new: bool) -> Vec<std::ffi::OsString> {
	let mut args = Vec::new();

	if new {
		args.push(std::ffi::OsString::from("-n"));
	}

	if let Some(bundle) = bundle {
		args.push(bundle.as_os_str().to_owned());
	} else {
		args.push(std::ffi::OsString::from("-a"));
		args.push(std::ffi::OsString::from("Decodex"));
	}

	args
}

#[cfg(target_os = "macos")]
fn open_decodex_app(bundle: Option<&Path>, new: bool) -> Result<()> {
	let args = decodex_app_open_args(bundle, new);
	let status = std::process::Command::new("/usr/bin/open")
		.args(args)
		.status()
		.map_err(|error| eyre::eyre!("Failed to start `open` for Decodex App: {error}"))?;

	if !status.success() {
		eyre::bail!("Failed to open Decodex App: `open` exited with {status}");
	}

	println!("Opened Decodex App.");

	Ok(())
}

#[cfg(not(target_os = "macos"))]
fn open_decodex_app(_bundle: Option<&Path>, _new: bool) -> Result<()> {
	eyre::bail!("`decodex app` is only supported on macOS");
}

#[cfg(test)] mod tests;
