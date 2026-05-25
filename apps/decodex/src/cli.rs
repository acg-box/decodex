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
	accounts::{self, AccountImportRequest, AccountLoginRequest, AccountUseRequest},
	agent,
	archive_hygiene::{self, ArchiveHygieneRequest},
	maintenance::{self, MaintenanceMode, MaintenancePruneRequest, MaintenanceScope},
	manual::{self, ManualCommitRequest, ManualLandRequest},
	orchestrator::{
		self, DiagnoseRequest, EvidenceRequest, IssueDispatchMode, RunOnceRequest, ServeRequest,
	},
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
	#[command(subcommand)]
	command: Command,
}
impl Cli {
	pub(crate) fn run(&self) -> crate::prelude::Result<()> {
		match &self.command {
			Command::Commit(args) => args.run(),
			Command::Land(args) => args.run(),
			Command::Run(args) => args.run(),
			Command::Serve(args) => args.run(),
			Command::Project(args) => args.run(),
			Command::Status(args) => args.run(),
			Command::Diagnose(args) => args.run(),
			Command::Evidence(args) => args.run(),
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
struct AccountCommand {
	#[command(subcommand)]
	command: AccountSubcommand,
}
impl AccountCommand {
	fn run(&self) -> crate::prelude::Result<()> {
		match &self.command {
			AccountSubcommand::List(args) => accounts::run_account_list(args.json),
			AccountSubcommand::Select(args) =>
				accounts::run_account_select(&args.selector, args.json),
			AccountSubcommand::Clear(args) => accounts::run_account_clear(args.json),
			AccountSubcommand::Logout(args) =>
				accounts::run_account_logout(&args.selector, args.json),
			AccountSubcommand::ImportAuth(args) =>
				accounts::run_account_import(&AccountImportRequest {
					auth_json_path: args.auth_json.clone(),
					json: args.json,
				}),
			AccountSubcommand::Use(args) => accounts::run_account_use(&AccountUseRequest {
				selector: args.selector.clone(),
				auth_json_path: args.auth_json.clone(),
				json: args.json,
			}),
			AccountSubcommand::Login(args) => accounts::run_account_login(&AccountLoginRequest {
				codex_bin: args.codex_bin.clone(),
				keep_temp_home: args.keep_temp_home,
			}),
		}
	}
}

#[derive(Debug, Args)]
struct AccountListCommand {
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct AccountSelectCommand {
	/// Email, full account id, or redacted fingerprint to pin.
	selector: String,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct AccountClearCommand {
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct AccountLogoutCommand {
	/// Email, full account id, or redacted fingerprint to remove.
	selector: String,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct AccountImportCommand {
	/// Path to a Codex `auth.json` file to import.
	#[arg(value_name = "AUTH_JSON")]
	auth_json: PathBuf,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct AccountUseCommand {
	/// Email, full account id, or redacted fingerprint to write into Codex `auth.json`.
	selector: String,
	/// Override the Codex `auth.json` destination. Defaults to `$CODEX_HOME/auth.json`
	/// or `~/.codex/auth.json`.
	#[arg(long, value_name = "AUTH_JSON")]
	auth_json: Option<PathBuf>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct AccountLoginCommand {
	/// Codex CLI binary used for isolated device login.
	#[arg(long, default_value = "codex")]
	codex_bin: String,
	/// Keep the temporary Codex home after login for manual inspection.
	#[arg(long, hide = true)]
	keep_temp_home: bool,
}

#[derive(Debug, Args)]
struct CommitCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
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
	fn run(&self) -> crate::prelude::Result<()> {
		manual::run_commit(
			self.project_config.as_path(),
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
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Tree-change summary for the landed change record.
	#[arg(value_name = "SUMMARY")]
	summary: String,
	/// Primary issue that authorizes the merged change. Defaults to the current issue worktree
	/// name.
	#[arg(long, value_name = "ISSUE", conflicts_with = "manual_authority")]
	authority: Option<String>,
	/// Use reserved authority `manual` instead of a Linear issue; requires `--pr`.
	#[arg(long, conflicts_with = "authority", requires = "pr")]
	manual_authority: bool,
	/// Pull request URL to land. Required with `--manual-authority`; otherwise defaults to the
	/// current review handoff marker.
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
	fn run(&self) -> crate::prelude::Result<()> {
		manual::run_land(
			self.project_config.as_path(),
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
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Run a specific leased or queued issue by Linear identifier or tracker issue id.
	#[arg(value_name = "ISSUE")]
	issue: Option<String>,
	/// Validate project loading, queue eligibility, and lane planning without tracker mutation.
	#[arg(long)]
	dry_run: bool,
	/// Explain current queued candidates without preparing or dispatching a lane.
	#[arg(long, requires = "dry_run", conflicts_with = "issue")]
	explain: bool,
}
impl RunCommand {
	fn run(&self) -> crate::prelude::Result<()> {
		orchestrator::run_once(RunOnceRequest {
			config_path: self.project_config.as_path(),
			dry_run: self.dry_run,
			explain_queue: self.explain,
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
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Poll interval between control-plane ticks, for example `60s` or `5m`.
	#[arg(long, value_name = "INTERVAL", value_parser = parse_duration_arg)]
	interval: Option<Duration>,
	/// Operator UI listen address.
	#[arg(long, value_name = "ADDR", default_value = "127.0.0.1:8912")]
	listen_address: String,
	/// Serve only local operator HTTP/API endpoints without polling or dispatching projects.
	#[arg(long, hide = true)]
	api_only: bool,
}
impl ServeCommand {
	fn run(&self) -> crate::prelude::Result<()> {
		if self.api_only && self.interval.is_some() {
			eyre::bail!(
				"serve --api-only does not accept --interval because API-only mode does not poll projects."
			);
		}

		orchestrator::run_control_plane(ServeRequest {
			config_path: self.project_config.as_path(),
			poll_interval: if self.api_only {
				None
			} else {
				Some(self.interval.unwrap_or_else(|| Duration::from_secs(60)))
			},
			listen_address: &self.listen_address,
			api_only: self.api_only,
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

				if !registration.enabled() {
					state_store.set_project_enabled(registration.service_id(), true)?;
				}

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
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
	/// Maximum number of recent runs to display.
	#[arg(long, value_name = "COUNT", default_value_t = orchestrator::DEFAULT_STATUS_RUN_LIMIT)]
	limit: usize,
}
impl StatusCommand {
	fn run(&self) -> crate::prelude::Result<()> {
		orchestrator::print_status(self.project_config.as_path(), self.json, self.limit)
	}
}

#[derive(Debug, Args)]
struct DiagnoseCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Emit the agent handoff index JSON instead of a one-line path summary.
	#[arg(long)]
	json: bool,
	/// Maximum number of recent runs to include while generating evidence.
	#[arg(long, value_name = "COUNT", default_value_t = orchestrator::DEFAULT_STATUS_RUN_LIMIT)]
	limit: usize,
}
impl DiagnoseCommand {
	fn run(&self) -> crate::prelude::Result<()> {
		orchestrator::run_diagnose(DiagnoseRequest {
			config_path: self.project_config.as_path(),
			json: self.json,
			limit: self.limit,
		})
	}
}

#[derive(Debug, Args)]
struct EvidenceCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Issue identifier or local issue id to inspect.
	issue: String,
	/// Restrict readback to one run id. Defaults to the latest local run for the issue.
	#[arg(long, value_name = "RUN_ID")]
	run_id: Option<String>,
	/// Restrict readback to one attempt number. Defaults to the selected run attempt.
	#[arg(long, value_name = "NUMBER")]
	attempt: Option<i64>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
	/// Include full structured payload values instead of compact payload summaries only.
	#[arg(long)]
	include_payload: bool,
}
impl EvidenceCommand {
	fn run(&self) -> crate::prelude::Result<()> {
		orchestrator::print_private_evidence(EvidenceRequest {
			config_path: self.project_config.as_path(),
			issue: &self.issue,
			run_id: self.run_id.as_deref(),
			attempt_number: self.attempt,
			json: self.json,
			include_payload: self.include_payload,
		})
	}
}

#[derive(Debug, Args)]
struct RecoverCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	#[command(subcommand)]
	command: RecoverSubcommand,
}
impl RecoverCommand {
	fn run(&self) -> crate::prelude::Result<()> {
		match &self.command {
			RecoverSubcommand::ReviewHandoff(args) => args.run(self.project_config.as_path()),
		}
	}
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
	#[command(flatten)]
	project_config: ProjectConfigArgs,
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
	fn run(&self) -> crate::prelude::Result<()> {
		archive_hygiene::run(
			self.project_config.as_path(),
			&ArchiveHygieneRequest {
				repo_labels: self.repo_labels.clone(),
				older_than_days: self.older_than_days,
				execute: self.execute,
			},
		)
	}
}

#[derive(Debug, Args)]
struct MaintenanceCommand {
	#[command(subcommand)]
	command: MaintenanceSubcommand,
}
impl MaintenanceCommand {
	fn run(&self) -> crate::prelude::Result<()> {
		match &self.command {
			MaintenanceSubcommand::Prune(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
struct MaintenancePruneCommand {
	/// Report candidates without applying retention changes. This is the default mode.
	#[arg(long, conflicts_with = "apply")]
	dry_run: bool,
	/// Apply safe file retention, state-aware runtime compaction, and WAL checkpointing.
	#[arg(long, conflicts_with = "dry_run")]
	apply: bool,
	/// Emit the maintenance report as JSON.
	#[arg(long)]
	json: bool,
}
impl MaintenancePruneCommand {
	fn run(&self) -> crate::prelude::Result<()> {
		let mode = if self.apply { MaintenanceMode::Apply } else { MaintenanceMode::DryRun };

		maintenance::run_prune_command(MaintenancePruneRequest {
			mode,
			scope: MaintenanceScope::Full,
			json: self.json,
		})
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
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Structured request file path, or `-` to read the request from stdin.
	#[arg(value_name = "REQUEST", default_value = "-")]
	request: String,
}
impl AttemptCommand {
	fn run(&self) -> crate::prelude::Result<()> {
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
	/// Write and print the agent-readable local evidence index.
	Diagnose(DiagnoseCommand),
	/// Inspect local-only private execution evidence for one issue or run.
	Evidence(EvidenceCommand),
	/// Diagnose or explicitly repair supported retained-lane recovery cases.
	Recover(RecoverCommand),
	/// Dry-run or archive old terminal Linear issues by repo label.
	ArchiveLinear(ArchiveLinearCommand),
	/// Maintain local Decodex logs, evidence, backups, and runtime storage.
	Maintenance(MaintenanceCommand),
	/// Manage the global Decodex Codex account pool.
	Account(AccountCommand),
	/// Validate the local app-server integration boundary.
	Probe(ProbeCommand),
	/// Run one daemon-planned attempt from a structured request.
	#[command(name = "_attempt", hide = true)]
	Attempt(AttemptCommand),
}

#[derive(Debug, Subcommand)]
enum AccountSubcommand {
	/// List configured Codex accounts without printing token material.
	List(AccountListCommand),
	/// Pin new Decodex runs to one account.
	Select(AccountSelectCommand),
	/// Return new Decodex runs to balanced account selection.
	Clear(AccountClearCommand),
	/// Remove one account from the Decodex account pool.
	Logout(AccountLogoutCommand),
	/// Import an existing Codex `auth.json` into the Decodex account pool.
	ImportAuth(AccountImportCommand),
	/// Force Codex to use one stored account by overwriting its `auth.json`.
	Use(AccountUseCommand),
	/// Run Codex device login in an isolated temporary home, then import it.
	Login(AccountLoginCommand),
}

#[derive(Debug, Subcommand)]
enum ProjectSubcommand {
	/// Register or refresh one Decodex project config and enable it.
	Add(ProjectAddCommand),
	/// List registered local projects.
	List,
	/// Enable one registered project for `decodex serve`.
	Enable(ProjectToggleCommand),
	/// Disable one registered project for `decodex serve`.
	Disable(ProjectToggleCommand),
}

#[derive(Debug, Subcommand)]
enum RecoverSubcommand {
	/// Recover retained review lanes whose handoff marker is missing.
	ReviewHandoff(ReviewHandoffRecoveryCommand),
}

#[derive(Debug, Subcommand)]
enum ReviewHandoffRecoverySubcommand {
	/// Read-only diagnosis for orphaned retained review lanes.
	Diagnose(ReviewHandoffDiagnoseCommand),
	/// Explicitly bind a validated PR URL to one retained review lane.
	Rebind(ReviewHandoffRebindCommand),
}

#[derive(Debug, Subcommand)]
enum MaintenanceSubcommand {
	/// Inspect or apply local Decodex storage retention.
	Prune(MaintenancePruneCommand),
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
	use std::{path::Path, time::Duration};

	use clap::Parser;

	use crate::cli::{
		AccountCommand, AccountSubcommand, AccountUseCommand, AttemptCommand, Cli, Command,
		CommitCommand, DiagnoseCommand, EvidenceCommand, LandCommand, ProbeCommand, ProjectCommand,
		ProjectConfigArgs, ProjectSubcommand, RecoverCommand, RecoverSubcommand,
		ReviewHandoffDiagnoseCommand, ReviewHandoffRebindCommand, ReviewHandoffRecoveryCommand,
		ReviewHandoffRecoverySubcommand, RunCommand, ServeCommand, StatusCommand,
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
	fn land_manual_authority_requires_pr() {
		let error = Cli::try_parse_from(["decodex", "land", "ship hotfix", "--manual-authority"])
			.expect_err("manual authority land should require an explicit PR");

		assert!(error.to_string().contains("--manual-authority"));
		assert!(error.to_string().contains("--pr"));
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

		assert!(matches!(
			cli.command,
			Command::Run(RunCommand { issue: Some(_), dry_run: true, explain: false, .. })
		));
	}

	#[test]
	fn parses_run_without_issue() {
		let cli = Cli::parse_from(["decodex", "run"]);

		assert!(matches!(
			cli.command,
			Command::Run(RunCommand { issue: None, dry_run: false, explain: false, .. })
		));
	}

	#[test]
	fn parses_run_dry_run_explain() {
		let cli = Cli::parse_from(["decodex", "run", "--dry-run", "--explain"]);

		assert!(matches!(
			cli.command,
			Command::Run(RunCommand { issue: None, dry_run: true, explain: true, .. })
		));

		let error = Cli::try_parse_from(["decodex", "run", "--explain"])
			.expect_err("explain should require dry-run");

		assert!(error.to_string().contains("--dry-run"));

		let error = Cli::try_parse_from(["decodex", "run", "issue-1", "--dry-run", "--explain"])
			.expect_err("explain should reject positional issue");

		assert!(error.to_string().contains("--explain"));
		assert!(error.to_string().contains("[ISSUE]"));
	}

	#[test]
	fn parses_serve_with_interval_listen_address_and_project_config() {
		let cli = Cli::parse_from([
			"decodex",
			"serve",
			"--config",
			"./project.toml",
			"--interval",
			"30s",
			"--listen-address",
			"127.0.0.1:9000",
		]);

		assert!(matches!(
			cli.command,
			Command::Serve(ServeCommand {
				project_config: ProjectConfigArgs { config: Some(config) },
				interval,
				listen_address,
				api_only,
			})
				if interval == Some(Duration::from_secs(30))
					&& listen_address == "127.0.0.1:9000"
					&& !api_only
					&& config == Path::new("./project.toml")
		));
	}

	#[test]
	fn parses_serve_api_only() {
		let cli = Cli::parse_from(["decodex", "serve", "--api-only"]);

		assert!(matches!(
			cli.command,
			Command::Serve(ServeCommand { interval: None, api_only: true, .. })
		));
	}

	#[test]
	fn rejects_serve_api_only_with_interval() {
		let cli = Cli::parse_from(["decodex", "serve", "--api-only", "--interval", "30s"]);
		let Command::Serve(command) = cli.command else {
			panic!("expected serve command");
		};
		let error = command.run().expect_err("api-only serve must reject interval configuration");
		let message = error.to_string();

		assert!(message.contains("--api-only"));
		assert!(message.contains("--interval"));
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
	fn parses_account_use_with_auth_json_override() {
		let cli = Cli::parse_from([
			"decodex",
			"account",
			"use",
			"copy@example.com",
			"--auth-json",
			"./auth.json",
			"--json",
		]);

		assert!(matches!(
			cli.command,
			Command::Account(AccountCommand {
				command: AccountSubcommand::Use(AccountUseCommand {
					selector,
					auth_json: Some(_),
					json: true,
				})
			}) if selector == "copy@example.com"
		));
	}

	#[test]
	fn account_commands_reject_project_config() {
		let error =
			Cli::try_parse_from(["decodex", "account", "list", "--config", "./project.toml"])
				.expect_err("global account commands should not accept project config");

		assert!(error.to_string().contains("--config"));
	}

	#[test]
	fn project_config_must_belong_to_project_scoped_command() {
		let error = Cli::try_parse_from(["decodex", "--config", "./project.toml", "status"])
			.expect_err("project config should not be accepted at root");

		assert!(error.to_string().contains("--config"));
	}

	#[test]
	fn parses_hidden_attempt_with_stdin_request() {
		let cli = Cli::parse_from(["decodex", "_attempt", "--config", "./project.toml", "-"]);

		assert!(matches!(
			cli.command,
			Command::Attempt(AttemptCommand {
				project_config: ProjectConfigArgs { config: Some(config) },
				request,
			}) if request == "-" && config == Path::new("./project.toml")
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
	fn parses_status_with_json_limit_and_project_config() {
		let cli = Cli::parse_from([
			"decodex",
			"status",
			"--config",
			"./project.toml",
			"--json",
			"--limit",
			"5",
		]);

		assert!(matches!(
			cli.command,
			Command::Status(StatusCommand {
				project_config: ProjectConfigArgs { config: Some(config) },
				json: true,
				limit: 5,
			}) if config == Path::new("./project.toml")
		));
	}

	#[test]
	fn parses_diagnose_with_json_limit_and_project_config() {
		let cli = Cli::parse_from([
			"decodex",
			"diagnose",
			"--config",
			"./project.toml",
			"--json",
			"--limit",
			"5",
		]);

		assert!(matches!(
			cli.command,
			Command::Diagnose(DiagnoseCommand {
				project_config: ProjectConfigArgs { config: Some(config) },
				json: true,
				limit: 5,
			}) if config == Path::new("./project.toml")
		));
	}

	#[test]
	fn parses_evidence_with_issue_run_attempt_json_payload_and_project_config() {
		let cli = Cli::parse_from([
			"decodex",
			"evidence",
			"--config",
			"./project.toml",
			"PUB-101",
			"--run-id",
			"run-1",
			"--attempt",
			"2",
			"--json",
			"--include-payload",
		]);

		assert!(matches!(
			cli.command,
			Command::Evidence(EvidenceCommand {
				project_config: ProjectConfigArgs { config: Some(config) },
				issue,
				run_id: Some(_),
				attempt: Some(2),
				json: true,
				include_payload: true,
			}) if config == Path::new("./project.toml") && issue == "PUB-101"
		));
	}

	#[test]
	fn parses_review_handoff_diagnose_with_issue_and_json() {
		let cli = Cli::parse_from([
			"decodex",
			"recover",
			"--config",
			"./project.toml",
			"review-handoff",
			"diagnose",
			"PUB-718",
			"--json",
		]);

		assert!(matches!(
			cli.command,
			Command::Recover(RecoverCommand {
				project_config: ProjectConfigArgs { config: Some(config) },
				command: RecoverSubcommand::ReviewHandoff(ReviewHandoffRecoveryCommand {
					command: ReviewHandoffRecoverySubcommand::Diagnose(
						ReviewHandoffDiagnoseCommand { issue: Some(_), json: true }
					)
				})
			}) if config == Path::new("./project.toml")
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
				}),
				..
			}) if issue == "PUB-718"
				&& pr == "https://github.com/hack-ink/pubfi-mono-v2/pull/14"
		));
	}
}
