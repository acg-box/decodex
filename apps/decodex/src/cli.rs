use std::{
	fs,
	io::{self, Read as _},
	path::{Path, PathBuf},
	time::Duration,
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
	archive_hygiene::{self, ArchiveHygieneRequest},
	manual::{self, ManualCommitRequest, ManualLandRequest},
	orchestrator::{self, IssueDispatchMode, RunOnceRequest, ServeRequest},
	prelude::eyre,
	recovery::{self, ReviewHandoffDiagnoseRequest, ReviewHandoffRebindRequest},
	runtime,
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
	/// Override the Decodex project directory or its `project.toml` path.
	#[arg(short = 'c', long, global = true, value_name = "PROJECT_DIR")]
	config: Option<PathBuf>,
	#[command(subcommand)]
	command: Command,
}
impl Cli {
	pub(crate) fn run(&self) -> crate::prelude::Result<()> {
		let config_path = self.config.as_deref();

		match &self.command {
			Command::Commit(args) => args.run(config_path),
			Command::Land(args) => args.run(config_path),
			Command::Run(args) => args.run(config_path),
			Command::Serve(args) => args.run(config_path),
			Command::Project(args) => args.run(),
			Command::Status(args) => args.run(config_path),
			Command::Recover(args) => args.run(config_path),
			Command::ArchiveLinear(args) => args.run(config_path),
			Command::Probe(args) => args.run(),
			Command::Attempt(args) => args.run(config_path),
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum AttemptDispatchMode {
	Normal,
	Retry,
	ReviewRepair,
	Closeout,
}
impl From<IssueDispatchMode> for AttemptDispatchMode {
	fn from(value: IssueDispatchMode) -> Self {
		match value {
			IssueDispatchMode::Normal => Self::Normal,
			IssueDispatchMode::Retry => Self::Retry,
			IssueDispatchMode::ReviewRepair => Self::ReviewRepair,
			IssueDispatchMode::Closeout => Self::Closeout,
		}
	}
}

impl From<AttemptDispatchMode> for IssueDispatchMode {
	fn from(value: AttemptDispatchMode) -> Self {
		match value {
			AttemptDispatchMode::Normal => Self::Normal,
			AttemptDispatchMode::Retry => Self::Retry,
			AttemptDispatchMode::ReviewRepair => Self::ReviewRepair,
			AttemptDispatchMode::Closeout => Self::Closeout,
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

#[derive(Debug, Subcommand)]
enum Command {
	/// Create a signed local commit with a `decodex/commit/1` message.
	Commit(CommitCommand),
	/// Land the current reviewed lane with a GitHub admin merge commit.
	Land(LandCommand),
	/// Run one orchestration pass.
	Run(RunCommand),
	/// Run the local multi-project Decodex control plane.
	Serve(ServeCommand),
	/// Manage the local Decodex project registry.
	Project(ProjectCommand),
	/// Inspect the current local runtime state for one configured project.
	Status(StatusCommand),
	/// Diagnose or explicitly repair supported retained-lane recovery cases.
	Recover(RecoverCommand),
	/// Dry-run or archive old terminal Linear issues by repo label.
	ArchiveLinear(ArchiveLinearCommand),
	/// Validate the local app-server integration boundary.
	Probe(ProbeCommand),
	/// Run one daemon-planned attempt from a structured request.
	#[command(name = "_attempt", hide = true)]
	Attempt(AttemptCommand),
}

#[derive(Debug, Args)]
struct CommitCommand {
	/// Tree-change summary for the new commit message.
	#[arg(value_name = "SUMMARY")]
	summary: String,
	/// Primary issue that authorizes the change. Defaults to the current issue worktree name.
	#[arg(long, value_name = "ISSUE", conflicts_with = "manual_authority")]
	authority: Option<String>,
	/// Use reserved authority `manual` instead of a Linear issue.
	#[arg(long, conflicts_with = "authority")]
	manual_authority: bool,
	/// Additional related issues for the commit message.
	#[arg(long, value_name = "ISSUE")]
	related: Vec<String>,
	/// Mark the change as breaking.
	#[arg(long)]
	breaking: bool,
}
impl CommitCommand {
	fn run(&self, config_path: Option<&Path>) -> crate::prelude::Result<()> {
		manual::run_commit(
			config_path,
			&ManualCommitRequest {
				summary: self.summary.clone(),
				authority: self.authority.clone(),
				manual_authority: self.manual_authority,
				related: self.related.clone(),
				breaking: self.breaking,
			},
		)
	}
}

#[derive(Debug, Args)]
struct LandCommand {
	/// Tree-change summary for the landed change record.
	#[arg(value_name = "SUMMARY")]
	summary: String,
	/// Primary issue that authorizes the merged change. Defaults to the current issue worktree
	/// name.
	#[arg(long, value_name = "ISSUE", conflicts_with = "manual_authority")]
	authority: Option<String>,
	/// Use reserved authority `manual` instead of a Linear issue.
	#[arg(long, conflicts_with = "authority")]
	manual_authority: bool,
	/// Pull request URL to land. Defaults to the current review handoff marker.
	#[arg(long, value_name = "URL")]
	pr: Option<String>,
	/// Additional related issues for the landed change record.
	#[arg(long, value_name = "ISSUE")]
	related: Vec<String>,
	/// Mark the landed change record as breaking.
	#[arg(long)]
	breaking: bool,
}
impl LandCommand {
	fn run(&self, config_path: Option<&Path>) -> crate::prelude::Result<()> {
		manual::run_land(
			config_path,
			&ManualLandRequest {
				summary: self.summary.clone(),
				authority: self.authority.clone(),
				manual_authority: self.manual_authority,
				pr_url: self.pr.clone(),
				related: self.related.clone(),
				breaking: self.breaking,
			},
		)
	}
}

#[derive(Debug, Args)]
struct RunCommand {
	/// Run a specific leased or queued issue by Linear identifier or tracker issue id.
	#[arg(value_name = "ISSUE")]
	issue: Option<String>,
	/// Skip external side effects where the later implementation allows it.
	#[arg(long)]
	dry_run: bool,
}
impl RunCommand {
	fn run(&self, config_path: Option<&Path>) -> crate::prelude::Result<()> {
		orchestrator::run_once(RunOnceRequest {
			config_path,
			dry_run: self.dry_run,
			preferred_issue_id: self.issue.as_deref(),
			preferred_issue_state: None,
			preferred_initial_issue_state: None,
			preferred_lease_acquired: false,
			preferred_issue_claim_fd: None,
			preferred_dispatch_slot_fd: None,
			preferred_dispatch_slot_index: None,
			preferred_dispatch_mode: None,
			preferred_run_id: None,
			preferred_attempt_number: None,
			preferred_retry_budget_base: None,
			preferred_workflow_snapshot: None,
		})
	}
}

#[derive(Debug, Args)]
struct ServeCommand {
	/// Poll interval between control-plane ticks, for example `60s` or `5m`.
	#[arg(long, value_name = "INTERVAL", default_value = "60s", value_parser = parse_duration_arg)]
	interval: Duration,
	/// Operator UI listen address.
	#[arg(long, value_name = "ADDR", default_value = "127.0.0.1:8912")]
	listen_address: String,
}
impl ServeCommand {
	fn run(&self, config_path: Option<&Path>) -> crate::prelude::Result<()> {
		orchestrator::run_control_plane(ServeRequest {
			config_path,
			poll_interval: self.interval,
			listen_address: &self.listen_address,
		})
	}
}

#[derive(Debug, Args)]
struct ProjectCommand {
	#[command(subcommand)]
	command: ProjectSubcommand,
}
impl ProjectCommand {
	fn run(&self) -> crate::prelude::Result<()> {
		let state_store = runtime::open_runtime_store()?;

		match &self.command {
			ProjectSubcommand::Add(args) => {
				let registration =
					runtime::register_project_config(&state_store, &args.config, true)?;

				println!(
					"registered project {} at {}",
					registration.service_id(),
					registration.config_path().display()
				);
			},
			ProjectSubcommand::List => {
				let projects = state_store.list_projects()?;

				if projects.is_empty() {
					println!("No registered projects.");
				} else {
					for project in projects {
						let status = if project.enabled() { "enabled" } else { "disabled" };

						println!(
							"{}\t{}\t{}",
							project.service_id(),
							status,
							project.config_path().display()
						);
					}
				}
			},
			ProjectSubcommand::Enable(args) => {
				state_store.set_project_enabled(&args.service_id, true)?;

				println!("enabled project {}", args.service_id);
			},
			ProjectSubcommand::Disable(args) => {
				state_store.set_project_enabled(&args.service_id, false)?;

				println!("disabled project {}", args.service_id);
			},
		}

		Ok(())
	}
}

#[derive(Debug, Subcommand)]
enum ProjectSubcommand {
	/// Register or refresh one Decodex project config.
	Add(ProjectAddCommand),
	/// List registered local projects.
	List,
	/// Enable one registered project for `decodex serve`.
	Enable(ProjectToggleCommand),
	/// Disable one registered project for `decodex serve`.
	Disable(ProjectToggleCommand),
}

#[derive(Debug, Args)]
struct ProjectAddCommand {
	/// Path to a Decodex project directory containing `project.toml` and `WORKFLOW.md`.
	#[arg(value_name = "PROJECT_DIR")]
	config: PathBuf,
}

#[derive(Debug, Args)]
struct ProjectToggleCommand {
	/// Project service id from the registered Decodex config.
	#[arg(value_name = "SERVICE_ID")]
	service_id: String,
}

#[derive(Debug, Args)]
struct StatusCommand {
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
	/// Maximum number of recent runs to display.
	#[arg(long, value_name = "COUNT", default_value_t = orchestrator::DEFAULT_STATUS_RUN_LIMIT)]
	limit: usize,
}
impl StatusCommand {
	fn run(&self, config_path: Option<&Path>) -> crate::prelude::Result<()> {
		orchestrator::print_status(config_path, self.json, self.limit)
	}
}

#[derive(Debug, Args)]
struct RecoverCommand {
	#[command(subcommand)]
	command: RecoverSubcommand,
}
impl RecoverCommand {
	fn run(&self, config_path: Option<&Path>) -> crate::prelude::Result<()> {
		match &self.command {
			RecoverSubcommand::ReviewHandoff(args) => args.run(config_path),
		}
	}
}

#[derive(Debug, Subcommand)]
enum RecoverSubcommand {
	/// Recover retained review lanes whose handoff marker is missing.
	ReviewHandoff(ReviewHandoffRecoveryCommand),
}

#[derive(Debug, Args)]
struct ReviewHandoffRecoveryCommand {
	#[command(subcommand)]
	command: ReviewHandoffRecoverySubcommand,
}
impl ReviewHandoffRecoveryCommand {
	fn run(&self, config_path: Option<&Path>) -> crate::prelude::Result<()> {
		match &self.command {
			ReviewHandoffRecoverySubcommand::Diagnose(args) =>
				recovery::run_review_handoff_diagnose(
					config_path,
					&ReviewHandoffDiagnoseRequest { issue: args.issue.clone(), json: args.json },
				),
			ReviewHandoffRecoverySubcommand::Rebind(args) => recovery::run_review_handoff_rebind(
				config_path,
				&ReviewHandoffRebindRequest {
					issue: args.issue.clone(),
					pr_url: args.pr.clone(),
					dry_run: args.dry_run,
				},
			),
		}
	}
}

#[derive(Debug, Subcommand)]
enum ReviewHandoffRecoverySubcommand {
	/// Read-only diagnosis for orphaned retained review lanes.
	Diagnose(ReviewHandoffDiagnoseCommand),
	/// Explicitly bind a validated PR URL to one retained review lane.
	Rebind(ReviewHandoffRebindCommand),
}

#[derive(Debug, Args)]
struct ReviewHandoffDiagnoseCommand {
	/// Issue identifier to inspect. Omit to inspect all retained review worktrees.
	#[arg(value_name = "ISSUE")]
	issue: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct ReviewHandoffRebindCommand {
	/// Issue identifier for the retained review lane.
	#[arg(value_name = "ISSUE")]
	issue: String,
	/// Pull request URL to bind after validation.
	#[arg(long, value_name = "URL")]
	pr: String,
	/// Validate only; do not write runtime markers or tracker audit comments.
	#[arg(long)]
	dry_run: bool,
}

#[derive(Debug, Args)]
struct ArchiveLinearCommand {
	/// Repo label scope to inspect, for example `repo:decodex`.
	#[arg(long = "repo-label", value_name = "LABEL", required = true)]
	repo_labels: Vec<String>,
	/// Archive only issues last updated more than this many days ago.
	#[arg(long, value_name = "DAYS", default_value_t = 30)]
	older_than_days: u32,
	/// Perform the archive mutation. Omit this flag for the dry-run candidate report.
	#[arg(long)]
	execute: bool,
}
impl ArchiveLinearCommand {
	fn run(&self, config_path: Option<&Path>) -> crate::prelude::Result<()> {
		archive_hygiene::run(
			config_path,
			&ArchiveHygieneRequest {
				repo_labels: self.repo_labels.clone(),
				older_than_days: self.older_than_days,
				execute: self.execute,
			},
		)
	}
}

#[derive(Debug, Args)]
struct ProbeCommand {
	/// Override the expected app-server transport during probing.
	#[arg(value_name = "TRANSPORT", default_value = "stdio://")]
	transport: String,
}
impl ProbeCommand {
	fn run(&self) -> crate::prelude::Result<()> {
		let report = agent::probe_app_server(&self.transport)?;

		println!(
			"probe ok: thread={} turn={} events={} output={}",
			report.thread_id, report.turn_id, report.event_count, report.final_output
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
	/// Structured request file path, or `-` to read the request from stdin.
	#[arg(value_name = "REQUEST", default_value = "-")]
	request: String,
}
impl AttemptCommand {
	fn run(&self, config_path: Option<&Path>) -> crate::prelude::Result<()> {
		let request = read_attempt_request(&self.request)?;

		orchestrator::run_once(RunOnceRequest {
			config_path,
			dry_run: request.dry_run,
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

fn parse_duration_arg(raw: &str) -> std::result::Result<Duration, String> {
	let (number, unit) = raw
		.strip_suffix('s')
		.map(|value| (value, "s"))
		.or_else(|| raw.strip_suffix('m').map(|value| (value, "m")))
		.or_else(|| raw.strip_suffix('h').map(|value| (value, "h")))
		.unwrap_or((raw, "s"));
	let value = number.parse::<u64>().map_err(|_| {
		format!("invalid duration `{raw}`; expected `<n>`, `<n>s`, `<n>m`, or `<n>h`")
	})?;

	if value == 0 {
		return Err(String::from("duration must be greater than zero"));
	}

	match unit {
		"s" => Ok(Duration::from_secs(value)),
		"m" => value
			.checked_mul(60)
			.map(Duration::from_secs)
			.ok_or_else(|| format!("duration `{raw}` is too large")),
		"h" => value
			.checked_mul(60)
			.and_then(|minutes| minutes.checked_mul(60))
			.map(Duration::from_secs)
			.ok_or_else(|| format!("duration `{raw}` is too large")),
		_ => Err(format!("unsupported duration unit in `{raw}`")),
	}
}

fn read_attempt_request(request: &str) -> crate::prelude::Result<AttemptRequest> {
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

#[cfg(test)]
mod tests {
	use std::{path::PathBuf, time::Duration};

	use clap::Parser;

	use crate::cli::{
		AttemptCommand, Cli, Command, CommitCommand, LandCommand, ProbeCommand, ProjectCommand,
		ProjectSubcommand, RecoverCommand, RecoverSubcommand, ReviewHandoffDiagnoseCommand,
		ReviewHandoffRebindCommand, ReviewHandoffRecoveryCommand, ReviewHandoffRecoverySubcommand,
		RunCommand, ServeCommand, StatusCommand,
	};

	#[test]
	fn parses_commit_with_authority_related_and_breaking() {
		let cli = Cli::parse_from([
			"decodex",
			"commit",
			"redesign decodex cli",
			"--authority",
			"XY-225",
			"--related",
			"XY-201",
			"--related",
			"XY-202",
			"--breaking",
		]);

		assert!(matches!(
			cli.command,
			Command::Commit(CommitCommand {
				authority: Some(_),
				manual_authority: false,
				breaking: true,
				..
			})
		));
	}

	#[test]
	fn parses_land_with_pr_override() {
		let cli = Cli::parse_from([
			"decodex",
			"land",
			"redesign decodex cli",
			"--pr",
			"https://github.com/hack-ink/decodex/pull/64",
		]);

		assert!(matches!(cli.command, Command::Land(LandCommand { pr: Some(_), .. })));
	}

	#[test]
	fn parses_commit_with_manual_authority() {
		let cli = Cli::parse_from(["decodex", "commit", "ship hotfix", "--manual-authority"]);

		assert!(matches!(
			cli.command,
			Command::Commit(CommitCommand { authority: None, manual_authority: true, .. })
		));
	}

	#[test]
	fn parses_land_with_manual_authority() {
		let cli = Cli::parse_from([
			"decodex",
			"land",
			"ship hotfix",
			"--manual-authority",
			"--pr",
			"https://github.com/hack-ink/decodex/pull/64",
		]);

		assert!(matches!(
			cli.command,
			Command::Land(LandCommand { authority: None, manual_authority: true, pr: Some(_), .. })
		));
	}

	#[test]
	fn commit_rejects_authority_and_manual_authority_together() {
		let error = Cli::try_parse_from([
			"decodex",
			"commit",
			"ship hotfix",
			"--authority",
			"XY-225",
			"--manual-authority",
		])
		.expect_err("authority and manual-authority should conflict");

		assert!(error.to_string().contains("--authority"));
		assert!(error.to_string().contains("--manual-authority"));
	}

	#[test]
	fn parses_run_with_positional_issue_and_dry_run() {
		let cli = Cli::parse_from(["decodex", "run", "issue-1", "--dry-run"]);

		assert!(matches!(cli.command, Command::Run(RunCommand { issue: Some(_), dry_run: true })));
	}

	#[test]
	fn parses_run_without_issue() {
		let cli = Cli::parse_from(["decodex", "run"]);

		assert!(matches!(cli.command, Command::Run(RunCommand { issue: None, dry_run: false })));
	}

	#[test]
	fn parses_serve_with_interval_listen_address_and_global_config() {
		let cli = Cli::parse_from([
			"decodex",
			"--config",
			"./project.toml",
			"serve",
			"--interval",
			"30s",
			"--listen-address",
			"127.0.0.1:9000",
		]);

		assert_eq!(cli.config, Some(PathBuf::from("./project.toml")));
		assert!(matches!(
			cli.command,
			Command::Serve(ServeCommand { interval, listen_address })
				if interval == Duration::from_secs(30) && listen_address == "127.0.0.1:9000"
		));
	}

	#[test]
	fn parses_project_add() {
		let cli = Cli::parse_from(["decodex", "project", "add", "./project.toml"]);

		assert!(matches!(
			cli.command,
			Command::Project(ProjectCommand { command: ProjectSubcommand::Add(_) })
		));
	}

	#[test]
	fn parses_project_enable() {
		let cli = Cli::parse_from(["decodex", "project", "enable", "pubfi"]);

		assert!(matches!(
			cli.command,
			Command::Project(ProjectCommand { command: ProjectSubcommand::Enable(_) })
		));
	}

	#[test]
	fn parses_hidden_attempt_with_stdin_request() {
		let cli = Cli::parse_from(["decodex", "--config", "./project.toml", "_attempt", "-"]);

		assert_eq!(cli.config, Some(PathBuf::from("./project.toml")));
		assert!(matches!(
			cli.command,
			Command::Attempt(AttemptCommand { request }) if request == "-"
		));
	}

	#[test]
	fn parses_probe_with_custom_transport() {
		let cli = Cli::parse_from(["decodex", "probe", "ws://127.0.0.1:9000"]);

		assert!(matches!(
			cli.command,
			Command::Probe(ProbeCommand { transport }) if transport == "ws://127.0.0.1:9000"
		));
	}

	#[test]
	fn parses_status_with_json_limit_and_global_config() {
		let cli = Cli::parse_from([
			"decodex",
			"--config",
			"./project.toml",
			"status",
			"--json",
			"--limit",
			"5",
		]);

		assert_eq!(cli.config, Some(PathBuf::from("./project.toml")));
		assert!(matches!(cli.command, Command::Status(StatusCommand { json: true, limit: 5 })));
	}

	#[test]
	fn parses_review_handoff_diagnose_with_issue_and_json() {
		let cli = Cli::parse_from([
			"decodex",
			"--config",
			"./project.toml",
			"recover",
			"review-handoff",
			"diagnose",
			"PUB-718",
			"--json",
		]);

		assert_eq!(cli.config, Some(PathBuf::from("./project.toml")));
		assert!(matches!(
			cli.command,
			Command::Recover(RecoverCommand {
				command: RecoverSubcommand::ReviewHandoff(ReviewHandoffRecoveryCommand {
					command: ReviewHandoffRecoverySubcommand::Diagnose(
						ReviewHandoffDiagnoseCommand { issue: Some(_), json: true }
					)
				})
			})
		));
	}

	#[test]
	fn parses_review_handoff_rebind_dry_run() {
		let cli = Cli::parse_from([
			"decodex",
			"recover",
			"review-handoff",
			"rebind",
			"PUB-718",
			"--pr",
			"https://github.com/hack-ink/pubfi-mono-v2/pull/14",
			"--dry-run",
		]);

		assert!(matches!(
			cli.command,
			Command::Recover(RecoverCommand {
				command: RecoverSubcommand::ReviewHandoff(ReviewHandoffRecoveryCommand {
					command: ReviewHandoffRecoverySubcommand::Rebind(
						ReviewHandoffRebindCommand { issue, pr, dry_run: true }
					)
				})
			}) if issue == "PUB-718"
				&& pr == "https://github.com/hack-ink/pubfi-mono-v2/pull/14"
		));
	}
}
