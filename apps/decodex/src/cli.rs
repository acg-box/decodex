mod account_commands;
mod docs_okf_commands;
mod radar_commands;
mod recovery_commands;

use self::{
	account_commands::AccountCommand,
	docs_okf_commands::{DocsCommand, OkfCommand},
	radar_commands::RadarCommand,
	recovery_commands::RecoverCommand,
};
#[cfg(test)]
use self::{
	account_commands::{AccountSubcommand, AccountUseCommand},
	docs_okf_commands::{
		DocsGraphCommand, DocsSubcommand, OkfFindCommand, OkfFindFilters, OkfInitCommand,
		OkfInitProfileArg, OkfSubcommand,
	},
	recovery_commands::{
		GhostLaneCleanupCommand, GhostLaneDiagnoseCommand, GhostLaneRecoveryCommand,
		GhostLaneRecoverySubcommand, LegacyCloseoutRecoveryCommand, MergedCloseoutRecoveryCommand,
		RecoverSubcommand, ReviewHandoffAdoptCommand, ReviewHandoffDiagnoseCommand,
		ReviewHandoffRebindCommand, ReviewHandoffRecoveryCommand, ReviewHandoffRecoverySubcommand,
		StaleActiveDiagnoseCommand, StaleActiveRecoveryCommand, StaleActiveRecoverySubcommand,
		StaleActiveReleaseCommand,
	},
};
use std::{
	fs,
	io::{self, Read as _},
	path::{Path, PathBuf},
	time::Duration,
};

use clap::{
	Args, Parser, Subcommand, ValueEnum,
	builder::{
		Styles,
		styling::{AnsiColor, Effects},
	},
};
use serde::{Deserialize, Serialize};

use crate::{
	agent,
	archive_hygiene::{self, ArchiveHygieneRequest},
	maintenance::{self, MaintenanceMode, MaintenancePruneRequest, MaintenanceScope},
	manual::{self, ManualCommitRequest, ManualLandRequest},
	mcp::{self, McpCapabilityProfile, McpServeRequest, McpTransport},
	orchestrator::{
		self, DEFAULT_STEER_RESULT_WAIT_TIMEOUT, DiagnoseRequest, EvidenceRequest,
		IssueDispatchMode, LaneInspectRequest, LaneInterruptRequest, LaneSteerReport,
		LaneSteerRequest, RunOnceRequest, ServeRequest,
	},
	prelude::{Result, eyre},
	program_intake::{self, GoalIntakeCommandRequest, IssueBatchIntakeCommandRequest},
	research_design::{
		self, ResearchDesignCompileRequest, ResearchDesignOutcome, ResearchDesignPromoteRequest,
		ResearchDesignRunInput,
	},
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
	fn run(&self) -> Result<()> {
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
	/// current review lifecycle record.
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
	fn run(&self) -> Result<()> {
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
	fn run(&self) -> Result<()> {
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
	/// Operator UI listen address.
	#[arg(long, value_name = "ADDR", default_value_t = orchestrator::DEFAULT_OPERATOR_LISTEN_ADDRESS.to_owned())]
	listen_address: String,
	/// Start the local dev endpoint without polling or dispatching projects.
	#[arg(long, hide = true)]
	dev: bool,
}
impl ServeCommand {
	fn run(&self) -> Result<()> {
		orchestrator::run_control_plane(ServeRequest {
			config_path: self.project_config.as_path(),
			listen_address: &self.listen_address,
			dev: self.dev,
		})
	}
}

#[derive(Debug, Args)]
struct McpCommand {
	#[command(subcommand)]
	command: McpSubcommand,
}
impl McpCommand {
	fn run(&self) -> Result<()> {
		match &self.command {
			McpSubcommand::Serve(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
struct McpServeCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// MCP transport.
	#[arg(long, value_enum, default_value_t = McpTransport::Stdio)]
	transport: McpTransport,
	/// Capability profile exposed by the MCP gateway. Defaults to admin for stdio and observe for
	/// Streamable HTTP.
	#[arg(long, value_enum)]
	capability_profile: Option<McpCapabilityProfile>,
	/// Streamable HTTP listen address.
	#[arg(long, value_name = "ADDR", default_value_t = mcp::DEFAULT_MCP_HTTP_LISTEN_ADDRESS.to_owned())]
	listen_address: String,
	/// Trusted browser Origin for Streamable HTTP. Repeat for multiple origins.
	#[arg(long = "allow-origin", value_name = "ORIGIN")]
	allowed_origins: Vec<String>,
	/// Environment variable containing the Streamable HTTP bearer token.
	#[arg(long = "bearer-token-env", value_name = "ENV_VAR")]
	bearer_token_env: Option<String>,
}
impl McpServeCommand {
	fn run(&self) -> Result<()> {
		mcp::serve(McpServeRequest {
			transport: self.transport,
			config_path: self.project_config.as_path(),
			capability_profile: self.effective_capability_profile(),
			listen_address: &self.listen_address,
			allowed_origins: &self.allowed_origins,
			bearer_token_env: self.bearer_token_env.as_deref(),
		})
	}

	fn effective_capability_profile(&self) -> McpCapabilityProfile {
		self.capability_profile.unwrap_or_else(|| self.transport.default_capability_profile())
	}
}

#[derive(Debug, Args)]
struct ProjectCommand {
	#[command(subcommand)]
	command: ProjectSubcommand,
}
impl ProjectCommand {
	fn run(&self) -> Result<()> {
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
			ProjectSubcommand::Remove(args) => {
				let removed = state_store.remove_project(&args.service_id)?;

				println!(
					"removed project {} at {}",
					removed.service_id(),
					removed.config_path().display()
				);
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
struct LaneCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	#[command(subcommand)]
	command: LaneSubcommand,
}
impl LaneCommand {
	fn run(&self) -> Result<()> {
		match &self.command {
			LaneSubcommand::Inspect(args) => orchestrator::print_lane_inspect(LaneInspectRequest {
				config_path: self.project_config.as_path(),
				issue: &args.issue,
				run_id: args.run_id.as_deref(),
				json: args.json,
			}),
			LaneSubcommand::Interrupt(args) => orchestrator::interrupt_lane(LaneInterruptRequest {
				config_path: self.project_config.as_path(),
				issue: &args.issue,
				run_id: &args.run_id,
				force: args.force,
				reason: args.reason.as_deref(),
				json: args.json,
				source: "cli",
			})
			.map(|_report| ()),
			LaneSubcommand::Steer(args) => args.run(self.project_config.as_path()),
		}
	}
}

#[derive(Debug, Args)]
struct LaneInspectCommand {
	/// Issue identifier or local issue id to inspect.
	#[arg(value_name = "ISSUE")]
	issue: String,
	/// Restrict inspection to one run id.
	#[arg(long, value_name = "RUN_ID")]
	run_id: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct LaneInterruptCommand {
	/// Issue identifier or local issue id to interrupt.
	#[arg(value_name = "ISSUE")]
	issue: String,
	/// Run id for the active app-server turn to interrupt.
	#[arg(long, value_name = "RUN_ID")]
	run_id: String,
	/// Use hard process-kill fallback when soft interrupt is unavailable or fails.
	#[arg(long)]
	force: bool,
	/// Operator-visible reason retained in local private evidence.
	#[arg(long, value_name = "TEXT")]
	reason: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}

#[derive(Debug, Args)]
struct LaneSteerCommand {
	/// Issue identifier or local issue id for the current lane.
	#[arg(value_name = "ISSUE")]
	issue: String,
	/// Run id that must own the active turn.
	#[arg(long, value_name = "RUN_ID")]
	run_id: String,
	/// Current active app-server turn id precondition.
	#[arg(long, value_name = "TURN_ID")]
	expected_turn_id: String,
	/// Operator-supplied steer text to send to the active turn.
	#[arg(long, value_name = "TEXT")]
	message: String,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
	/// How long to wait for the active attempt to report delivery.
	#[arg(long, value_name = "MILLISECONDS", default_value_t = default_lane_steer_wait_timeout_ms())]
	wait_timeout_ms: u64,
}
impl LaneSteerCommand {
	fn run(&self, config_path: Option<&Path>) -> Result<()> {
		let report = orchestrator::steer_lane(LaneSteerRequest {
			config_path,
			project_id: None,
			issue: &self.issue,
			run_id: &self.run_id,
			expected_turn_id: &self.expected_turn_id,
			message: &self.message,
			source: "cli",
			wait_timeout: Duration::from_millis(self.wait_timeout_ms),
		})?;

		if self.json {
			println!("{}", serde_json::to_string_pretty(&report)?);
		} else {
			print!("{}", render_lane_steer_report(&report));
		}
		if lane_steer_report_is_failure(&report) {
			eyre::bail!(
				"lane steer {}: {}",
				report.outcome,
				report.failure_class.as_deref().unwrap_or(&report.reason)
			);
		}

		Ok(())
	}
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
	/// Refresh live tracker and pull-request observers before printing status.
	#[arg(long)]
	live: bool,
}
impl StatusCommand {
	fn run(&self) -> Result<()> {
		orchestrator::print_status(self.project_config.as_path(), self.json, self.limit, self.live)
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
	fn run(&self) -> Result<()> {
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
	/// Resolve this evidence readback through a registered Decodex project id.
	#[arg(long, value_name = "SERVICE_ID")]
	project: Option<String>,
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
	fn run(&self) -> Result<()> {
		orchestrator::print_private_evidence(EvidenceRequest {
			config_path: self.project_config.as_path(),
			project_id: self.project.as_deref(),
			issue: &self.issue,
			run_id: self.run_id.as_deref(),
			attempt_number: self.attempt,
			json: self.json,
			include_payload: self.include_payload,
		})
	}
}

#[derive(Debug, Args)]
struct IntakeCommand {
	#[command(subcommand)]
	command: IntakeSubcommand,
}
impl IntakeCommand {
	fn run(&self) -> Result<()> {
		match &self.command {
			IntakeSubcommand::Goal(args) => args.run(),
			IntakeSubcommand::Issues(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
struct IntakeGoalCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Registered Decodex service id to intake against.
	#[arg(long, value_name = "SERVICE_ID", conflicts_with = "config")]
	project: Option<String>,
	/// Read tracker state and print the deterministic goal-intake report without mutation.
	#[arg(long, conflicts_with = "apply", required_unless_present = "apply")]
	dry_run: bool,
	/// Create or update generated normal Linear issues and persist local Program Intake state.
	#[arg(long, conflicts_with = "dry_run")]
	apply: bool,
	/// Existing Linear issue whose team and startable state should anchor generated issues.
	#[arg(long, value_name = "ISSUE")]
	team_issue: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
	/// Accepted Decision Contract identifier to materialize.
	#[arg(value_name = "CONTRACT_ID")]
	contract_id: String,
}
impl IntakeGoalCommand {
	fn run(&self) -> Result<()> {
		let report = program_intake::run_goal_intake_command(GoalIntakeCommandRequest {
			config_path: self.project_config.as_path(),
			project_id: self.project.as_deref(),
			contract_id: &self.contract_id,
			team_issue_identifier: self.team_issue.as_deref(),
			dry_run: self.dry_run,
			apply: self.apply,
		})?;

		if self.json {
			println!("{}", serde_json::to_string_pretty(&report)?);
		} else {
			print!("{}", program_intake::render_goal_intake_report(&report));
		}

		Ok(())
	}
}

#[derive(Debug, Args)]
struct IntakeIssuesCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Registered Decodex service id to intake against.
	#[arg(long, value_name = "SERVICE_ID", conflicts_with = "config")]
	project: Option<String>,
	/// Read tracker state and print the deterministic intake report without local persistence.
	#[arg(long, conflicts_with = "apply", required_unless_present = "apply")]
	dry_run: bool,
	/// Persist local runtime Program Intake records for direct Program dispatch.
	#[arg(long, conflicts_with = "dry_run")]
	apply: bool,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
	/// Existing Linear issue identifiers to intake into an internal Execution Program.
	#[arg(value_name = "ISSUE")]
	#[arg(required = true)]
	issues: Vec<String>,
}
impl IntakeIssuesCommand {
	fn run(&self) -> Result<()> {
		let report =
			program_intake::run_issue_batch_intake_command(IssueBatchIntakeCommandRequest {
				config_path: self.project_config.as_path(),
				project_id: self.project.as_deref(),
				issue_identifiers: self.issues.clone(),
				dry_run: self.dry_run,
				persist: self.apply,
			})?;

		if self.json {
			println!("{}", serde_json::to_string_pretty(&report)?);
		} else {
			print!("{}", program_intake::render_issue_batch_intake_report(&report));
		}

		Ok(())
	}
}

#[derive(Debug, Args)]
struct ResearchCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	#[command(subcommand)]
	command: ResearchSubcommand,
}
impl ResearchCommand {
	fn run(&self) -> Result<()> {
		match &self.command {
			ResearchSubcommand::Compile(args) => args.run(self.project_config.as_path()),
			ResearchSubcommand::Promote(args) => args.run(self.project_config.as_path()),
		}
	}
}

#[derive(Debug, Args)]
struct ResearchCompileCommand {
	/// Structured research/design input JSON. Use `-` to read from stdin.
	#[arg(long, value_name = "JSON")]
	input: Option<PathBuf>,
	/// Natural-language research/design intent for minimal intake.
	#[arg(long, value_name = "TEXT", conflicts_with = "input")]
	intent: Option<String>,
	/// Source tracker issue identifier to link to the contract.
	#[arg(long, value_name = "ISSUE")]
	source_issue: Option<String>,
	/// Outcome for minimal natural-language intake.
	#[arg(long, value_enum, default_value = "not-decision-ready")]
	outcome: ResearchOutcomeArg,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}
impl ResearchCompileCommand {
	fn run(&self, config_path: Option<&Path>) -> Result<()> {
		let input = self.research_input()?;
		let report =
			research_design::run_compile(ResearchDesignCompileRequest { config_path, input })?;

		if self.json {
			println!("{}", serde_json::to_string_pretty(&report)?);
		} else {
			println!(
				"research compile {}: contract={} status={} ready_after_promotion={} authority={}",
				research_outcome_label(report.outcome),
				report.contract_id,
				report.contract_status.as_str(),
				report.issue_generation_ready_after_promotion,
				report.execution_authority_granted
			);
			println!("{}", report.feedback);
		}

		Ok(())
	}

	fn research_input(&self) -> Result<ResearchDesignRunInput> {
		match (&self.input, &self.intent) {
			(Some(path), None) => read_research_input(path),
			(None, Some(intent)) => Ok(ResearchDesignRunInput::from_intent(
				intent.clone(),
				self.source_issue.clone(),
				self.outcome.into(),
			)),
			(None, None) => {
				eyre::bail!("research compile requires either --input <JSON> or --intent <TEXT>.")
			},
			(Some(_), Some(_)) => {
				eyre::bail!("research compile accepts --input or --intent, not both.")
			},
		}
	}
}

#[derive(Debug, Args)]
struct ResearchPromoteCommand {
	/// Decision Contract identifier to promote.
	#[arg(value_name = "CONTRACT_ID")]
	contract_id: String,
	/// Actor accepting the contract.
	#[arg(long, value_name = "TEXT", default_value = "operator")]
	accepted_by: String,
	/// Acceptance source, usually conversation or runtime policy.
	#[arg(long, value_name = "TEXT", default_value = "conversation")]
	acceptance_source: String,
	/// RFC3339 acceptance timestamp. Defaults to current UTC time.
	#[arg(long, value_name = "RFC3339")]
	accepted_at: Option<String>,
	/// Optional acceptance reason.
	#[arg(long, value_name = "TEXT")]
	reason: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	json: bool,
}
impl ResearchPromoteCommand {
	fn run(&self, config_path: Option<&Path>) -> Result<()> {
		let report = research_design::run_promote(ResearchDesignPromoteRequest {
			config_path,
			contract_id: &self.contract_id,
			accepted_by: &self.accepted_by,
			accepted_at: self.accepted_at.as_deref(),
			acceptance_source: &self.acceptance_source,
			promotion_reason: self.reason.clone(),
		})?;

		if self.json {
			println!("{}", serde_json::to_string_pretty(&report)?);
		} else {
			println!(
				"research promote: contract={} status={} authority={} ready={}",
				report.contract_id,
				report.contract_status.as_str(),
				report.execution_authority_granted,
				report.ready_for_issue_shaping
			);
		}

		Ok(())
	}
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
	fn run(&self) -> Result<()> {
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
	fn run(&self) -> Result<()> {
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
	fn run(&self) -> Result<()> {
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

impl From<ResearchOutcomeArg> for ResearchDesignOutcome {
	fn from(value: ResearchOutcomeArg) -> Self {
		match value {
			ResearchOutcomeArg::DecisionReady => Self::DecisionReady,
			ResearchOutcomeArg::NotDecisionReady => Self::NotDecisionReady,
			ResearchOutcomeArg::Blocked => Self::Blocked,
			ResearchOutcomeArg::NeedsHumanDecision => Self::NeedsHumanDecision,
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
	/// Remove one registered project from the local registry.
	Remove(ProjectToggleCommand),
}

#[derive(Debug, Subcommand)]
enum McpSubcommand {
	/// Serve Decodex MCP protocol primitives.
	Serve(McpServeCommand),
}

#[derive(Debug, Subcommand)]
enum LaneSubcommand {
	/// Inspect one local lane by issue identifier or tracker issue id.
	Inspect(LaneInspectCommand),
	/// Soft-interrupt an active app-server turn, with optional hard fallback.
	Interrupt(LaneInterruptCommand),
	/// Send operator-supplied text to an active steerable turn.
	Steer(LaneSteerCommand),
}

#[derive(Debug, Subcommand)]
enum ResearchSubcommand {
	/// Compile bounded research/design input into a latent Decision Contract.
	Compile(ResearchCompileCommand),
	/// Promote an accepted Decision Contract into execution authority.
	Promote(ResearchPromoteCommand),
}

#[derive(Debug, Subcommand)]
enum IntakeSubcommand {
	/// Dry-run or apply a promoted Decision Contract as normal issue-backed goal intake.
	Goal(IntakeGoalCommand),
	/// Dry-run or persist existing Linear issues as an internal program intake batch.
	Issues(IntakeIssuesCommand),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
enum ResearchOutcomeArg {
	DecisionReady,
	NotDecisionReady,
	Blocked,
	NeedsHumanDecision,
}

#[derive(Debug, Subcommand)]
enum MaintenanceSubcommand {
	/// Inspect or apply local Decodex storage retention.
	Prune(MaintenancePruneCommand),
}

fn default_lane_steer_wait_timeout_ms() -> u64 {
	u64::try_from(DEFAULT_STEER_RESULT_WAIT_TIMEOUT.as_millis()).unwrap_or(10_000)
}

fn lane_steer_report_is_failure(report: &LaneSteerReport) -> bool {
	matches!(report.outcome.as_str(), "rejected" | "failed" | "timed_out" | "fallback")
}

fn research_outcome_label(outcome: ResearchDesignOutcome) -> &'static str {
	match outcome {
		ResearchDesignOutcome::DecisionReady => "decision-ready",
		ResearchDesignOutcome::NotDecisionReady => "not-decision-ready",
		ResearchDesignOutcome::Blocked => "blocked",
		ResearchDesignOutcome::NeedsHumanDecision => "needs-human-decision",
	}
}

fn render_lane_steer_report(report: &LaneSteerReport) -> String {
	format!(
		"lane steer {}: issue={} run_id={} attempt={} expected_turn_id={} current_turn_id={} response_turn_id={} failure_class={} audit_record_id={} delivery_status={}\n",
		report.outcome,
		report.issue_identifier.as_deref().unwrap_or(&report.issue_id),
		report.run_id,
		report.attempt_number,
		report.expected_turn_id,
		report.current_turn_id.as_deref().unwrap_or("none"),
		report.response_turn_id.as_deref().unwrap_or("none"),
		report.failure_class.as_deref().unwrap_or("none"),
		report.audit_record_id,
		report.delivery_status
	)
}

fn read_research_input(path: &Path) -> Result<ResearchDesignRunInput> {
	let raw = if path == Path::new("-") {
		let mut raw = String::new();

		io::stdin().read_to_string(&mut raw)?;

		raw
	} else {
		fs::read_to_string(path)?
	};

	serde_json::from_str(&raw).map_err(|error| {
		eyre::eyre!("Failed to parse research input `{}`: {error}", path.display())
	})
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

#[cfg(test)]
mod tests {
	use std::{ffi::OsString, path::Path};

	use clap::Parser;

	use crate::{
		cli::{
			AccountCommand, AccountSubcommand, AccountUseCommand, AppCommand, AttemptCommand, Cli,
			Command, CommitCommand, DiagnoseCommand, DocsCommand, DocsSubcommand, EvidenceCommand,
			GhostLaneCleanupCommand, GhostLaneDiagnoseCommand, GhostLaneRecoveryCommand,
			GhostLaneRecoverySubcommand, IntakeCommand, IntakeGoalCommand, IntakeIssuesCommand,
			IntakeSubcommand, LandCommand, LaneCommand, LaneInspectCommand, LaneInterruptCommand,
			LaneSteerCommand, LaneSubcommand, LegacyCloseoutRecoveryCommand, McpSubcommand,
			MergedCloseoutRecoveryCommand, OkfCommand, OkfInitCommand, OkfInitProfileArg,
			OkfSubcommand, ProbeCommand, ProjectCommand, ProjectConfigArgs, ProjectSubcommand,
			RecoverCommand, RecoverSubcommand, ResearchCommand, ResearchCompileCommand,
			ResearchOutcomeArg, ResearchPromoteCommand, ResearchSubcommand,
			ReviewHandoffAdoptCommand, ReviewHandoffDiagnoseCommand, ReviewHandoffRebindCommand,
			ReviewHandoffRecoveryCommand, ReviewHandoffRecoverySubcommand, RunCommand,
			ServeCommand, StaleActiveDiagnoseCommand, StaleActiveRecoveryCommand,
			StaleActiveRecoverySubcommand, StaleActiveReleaseCommand, StatusCommand,
		},
		mcp::{McpCapabilityProfile, McpTransport},
	};

	#[test]
	fn parses_app_command() {
		let cli = Cli::parse_from(["decodex", "app"]);

		assert!(matches!(cli.command, Command::App(AppCommand { bundle: None, new: false })));
	}

	#[test]
	fn parses_app_bundle_and_new_instance() {
		let cli = Cli::parse_from([
			"decodex",
			"app",
			"--bundle",
			"target/decodex-app/Decodex.app",
			"--new",
		]);

		assert!(matches!(
			cli.command,
			Command::App(AppCommand {
				bundle: Some(bundle),
				new: true,
			}) if bundle == Path::new("target/decodex-app/Decodex.app")
		));
	}

	#[test]
	fn builds_macos_open_arguments_for_decodex_app() {
		assert_eq!(
			super::decodex_app_open_args(None, false),
			vec![OsString::from("-a"), OsString::from("Decodex")]
		);
		assert_eq!(
			super::decodex_app_open_args(Some(Path::new("target/decodex-app/Decodex.app")), true),
			vec![
				OsString::from("-n"),
				Path::new("target/decodex-app/Decodex.app").as_os_str().to_owned(),
			]
		);
	}

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
	fn parses_okf_and_docs_find_graph_commands() {
		let okf_cli = Cli::parse_from([
			"decodex",
			"okf",
			"find",
			"docs",
			"--tag",
			"okf",
			"--text",
			"command design",
		]);
		let Command::Okf(OkfCommand {
			command:
				OkfSubcommand::Find(super::OkfFindCommand {
					root,
					filters: super::OkfFindFilters { tag, text: Some(text), .. },
				}),
		}) = okf_cli.command
		else {
			panic!("expected okf find command");
		};

		assert_eq!(root, Path::new("docs"));
		assert_eq!(tag, vec![String::from("okf")]);
		assert_eq!(text, "command design");

		let docs_cli = Cli::parse_from(["decodex", "docs", "graph", "--json"]);

		assert!(matches!(
			docs_cli.command,
			Command::Docs(DocsCommand {
				root,
				command: DocsSubcommand::Graph(super::DocsGraphCommand {
					json: true,
				}),
			}) if root == Path::new("docs")
		));
	}

	#[test]
	fn parses_okf_init_command() {
		let cli = Cli::parse_from(["decodex", "okf", "init", "knowledge", "--profile", "wiki"]);

		assert!(matches!(
			cli.command,
			Command::Okf(OkfCommand {
				command: OkfSubcommand::Init(OkfInitCommand {
					root,
					profile: OkfInitProfileArg::Wiki,
				}),
			}) if root == Path::new("knowledge")
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
	fn parses_manual_authority_commands() {
		enum ExpectedCommand {
			Commit,
			Land,
		}

		for (case_name, args, expected) in [
			(
				"commit manual authority",
				&["decodex", "commit", "ship hotfix", "--manual-authority"][..],
				ExpectedCommand::Commit,
			),
			(
				"land manual authority",
				&[
					"decodex",
					"land",
					"ship hotfix",
					"--manual-authority",
					"--pr",
					"https://github.com/hack-ink/decodex/pull/64",
				][..],
				ExpectedCommand::Land,
			),
		] {
			let cli = Cli::parse_from(args.iter().copied());

			match expected {
				ExpectedCommand::Commit => assert!(
					matches!(
						cli.command,
						Command::Commit(CommitCommand {
							authority: None,
							manual_authority: true,
							..
						})
					),
					"unexpected parsed command for `{case_name}`"
				),
				ExpectedCommand::Land => assert!(
					matches!(
						cli.command,
						Command::Land(LandCommand {
							authority: None,
							manual_authority: true,
							pr: Some(_),
							..
						})
					),
					"unexpected parsed command for `{case_name}`"
				),
			}
		}
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
	fn parses_run_modes() {
		for (case_name, args, expected_issue, expected_dry_run, expected_explain) in [
			(
				"positional issue dry run",
				&["decodex", "run", "issue-1", "--dry-run"][..],
				Some("issue-1"),
				true,
				false,
			),
			("default run", &["decodex", "run"][..], None, false, false),
			(
				"explain dry run",
				&["decodex", "run", "--dry-run", "--explain"][..],
				None,
				true,
				true,
			),
		] {
			let cli = Cli::parse_from(args.iter().copied());

			assert!(
				matches!(
					cli.command,
					Command::Run(RunCommand { issue, dry_run, explain, .. })
						if issue.as_deref() == expected_issue
							&& dry_run == expected_dry_run
							&& explain == expected_explain
				),
				"unexpected parsed run command for `{case_name}`"
			);
		}

		let error = Cli::try_parse_from(["decodex", "run", "--explain"])
			.expect_err("explain should require dry-run");

		assert!(error.to_string().contains("--dry-run"));

		let error = Cli::try_parse_from(["decodex", "run", "issue-1", "--dry-run", "--explain"])
			.expect_err("explain should reject positional issue");

		assert!(error.to_string().contains("--explain"));
		assert!(error.to_string().contains("[ISSUE]"));
	}

	#[test]
	fn parses_serve_modes() {
		for (case_name, args, expected_listen_address, expected_config, expected_dev) in [
			("default listen address", &["decodex", "serve"][..], "127.0.0.1:8192", None, false),
			(
				"custom listen address and project config",
				&[
					"decodex",
					"serve",
					"--config",
					"./project.toml",
					"--listen-address",
					"127.0.0.1:9000",
				][..],
				"127.0.0.1:9000",
				Some("./project.toml"),
				false,
			),
			("dev mode", &["decodex", "serve", "--dev"][..], "127.0.0.1:8192", None, true),
		] {
			let cli = Cli::parse_from(args.iter().copied());

			assert!(
				matches!(
					cli.command,
					Command::Serve(ServeCommand {
						project_config: ProjectConfigArgs { config },
						listen_address,
						dev,
					}) if listen_address == expected_listen_address
						&& config.as_deref() == expected_config.map(Path::new)
						&& dev == expected_dev
				),
				"unexpected parsed serve command for `{case_name}`"
			);
		}
	}

	#[test]
	fn parses_mcp_stdio_serve() {
		let cli = Cli::parse_from([
			"decodex",
			"mcp",
			"serve",
			"--config",
			"./project.toml",
			"--transport",
			"stdio",
		]);
		let Command::Mcp(command) = cli.command else {
			panic!("expected mcp command");
		};
		let McpSubcommand::Serve(serve) = command.command;

		assert_eq!(serve.project_config.config.as_deref(), Some(Path::new("./project.toml")));
		assert_eq!(serve.transport, McpTransport::Stdio);
		assert_eq!(serve.effective_capability_profile(), McpCapabilityProfile::Admin);
		assert_eq!(serve.listen_address, crate::mcp::DEFAULT_MCP_HTTP_LISTEN_ADDRESS);
		assert!(serve.allowed_origins.is_empty());
		assert_eq!(serve.bearer_token_env, None);
	}

	#[test]
	fn parses_mcp_streamable_http_serve_with_safe_profile_default() {
		let cli = Cli::parse_from([
			"decodex",
			"mcp",
			"serve",
			"--transport",
			"streamable-http",
			"--listen-address",
			"127.0.0.1:8194",
			"--allow-origin",
			"http://127.0.0.1:8194",
			"--bearer-token-env",
			"DECODEX_MCP_TOKEN",
		]);
		let Command::Mcp(command) = cli.command else {
			panic!("expected mcp command");
		};
		let McpSubcommand::Serve(serve) = command.command;

		assert_eq!(serve.transport, McpTransport::StreamableHttp);
		assert_eq!(serve.effective_capability_profile(), McpCapabilityProfile::Observe);
		assert_eq!(serve.listen_address, "127.0.0.1:8194");
		assert_eq!(serve.allowed_origins, vec!["http://127.0.0.1:8194"]);
		assert_eq!(serve.bearer_token_env.as_deref(), Some("DECODEX_MCP_TOKEN"));
	}

	#[test]
	fn parses_radar_command_family_after_module_split() {
		let cli = Cli::parse_from([
			"decodex",
			"radar",
			"bundle",
			"build",
			"--repo",
			"openai/codex",
			"--pr",
			"42",
			"--out",
			"bundle.json",
		]);

		assert!(matches!(cli.command, Command::Radar(_)));
	}

	#[test]
	fn rejects_serve_interval_argument() {
		let error = Cli::try_parse_from(["decodex", "serve", "--interval", "30s"])
			.expect_err("serve interval override should be removed");
		let message = error.to_string();

		assert!(message.contains("--interval"));
	}

	#[test]
	fn parses_project_subcommands() {
		enum ExpectedProjectSubcommand {
			Add,
			Enable,
			Remove,
		}

		for (case_name, args, expected) in [
			(
				"add",
				&["decodex", "project", "add", "./project.toml"][..],
				ExpectedProjectSubcommand::Add,
			),
			(
				"enable",
				&["decodex", "project", "enable", "pubfi"][..],
				ExpectedProjectSubcommand::Enable,
			),
			(
				"remove",
				&["decodex", "project", "remove", "vibe-mono"][..],
				ExpectedProjectSubcommand::Remove,
			),
		] {
			let cli = Cli::parse_from(args.iter().copied());

			match expected {
				ExpectedProjectSubcommand::Add => assert!(
					matches!(
						cli.command,
						Command::Project(ProjectCommand { command: ProjectSubcommand::Add(_) })
					),
					"unexpected parsed project subcommand for `{case_name}`"
				),
				ExpectedProjectSubcommand::Enable => assert!(
					matches!(
						cli.command,
						Command::Project(ProjectCommand { command: ProjectSubcommand::Enable(_) })
					),
					"unexpected parsed project subcommand for `{case_name}`"
				),
				ExpectedProjectSubcommand::Remove => assert!(
					matches!(
						cli.command,
						Command::Project(ProjectCommand { command: ProjectSubcommand::Remove(_) })
					),
					"unexpected parsed project subcommand for `{case_name}`"
				),
			}
		}
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
			Command::Probe(ProbeCommand { transport, .. }) if transport == "ws://127.0.0.1:9000"
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
				live: false,
			}) if config == Path::new("./project.toml")
		));
	}

	#[test]
	fn parses_lane_inspect_with_run_id_and_project_config() {
		let cli = Cli::parse_from([
			"decodex",
			"lane",
			"--config",
			"./project.toml",
			"inspect",
			"XY-703",
			"--run-id",
			"xy-703-attempt-1",
			"--json",
		]);

		assert!(matches!(
			cli.command,
			Command::Lane(LaneCommand {
				project_config: ProjectConfigArgs { config: Some(config) },
				command: LaneSubcommand::Inspect(LaneInspectCommand {
					issue,
					run_id: Some(run_id),
					json: true,
				})
			}) if config == Path::new("./project.toml")
				&& issue == "XY-703"
				&& run_id == "xy-703-attempt-1"
		));
	}

	#[test]
	fn parses_lane_interrupt_with_force_reason_and_project_config() {
		let cli = Cli::parse_from([
			"decodex",
			"lane",
			"--config",
			"./project.toml",
			"interrupt",
			"XY-703",
			"--run-id",
			"xy-703-attempt-1",
			"--force",
			"--reason",
			"operator requested",
			"--json",
		]);

		assert!(matches!(
			cli.command,
			Command::Lane(LaneCommand {
				project_config: ProjectConfigArgs { config: Some(config) },
				command: LaneSubcommand::Interrupt(LaneInterruptCommand {
					issue,
					run_id,
					force: true,
					reason: Some(reason),
					json: true,
				})
			}) if config == Path::new("./project.toml")
				&& issue == "XY-703"
				&& run_id == "xy-703-attempt-1"
				&& reason == "operator requested"
		));
	}

	#[test]
	fn parses_lane_steer_with_expected_turn_precondition() {
		let cli = Cli::parse_from([
			"decodex",
			"lane",
			"--config",
			"./project.toml",
			"steer",
			"XY-704",
			"--run-id",
			"run-1",
			"--expected-turn-id",
			"turn-1",
			"--message",
			"adjust the current implementation",
			"--json",
		]);

		assert!(matches!(
			cli.command,
			Command::Lane(LaneCommand {
				project_config: ProjectConfigArgs { config: Some(config) },
				command: LaneSubcommand::Steer(LaneSteerCommand {
					issue,
					run_id,
					expected_turn_id,
					message,
					json: true,
					..
				})
			}) if config == Path::new("./project.toml")
				&& issue == "XY-704"
				&& run_id == "run-1"
				&& expected_turn_id == "turn-1"
				&& message == "adjust the current implementation"
		));
	}

	#[test]
	fn parses_research_compile_with_intent_and_project_config() {
		let cli = Cli::parse_from([
			"decodex",
			"research",
			"--config",
			"./project.toml",
			"compile",
			"--intent",
			"research X",
			"--source-issue",
			"XY-860",
			"--outcome",
			"needs-human-decision",
			"--json",
		]);

		assert!(matches!(
			cli.command,
			Command::Research(ResearchCommand {
				project_config: ProjectConfigArgs { config: Some(config) },
				command: ResearchSubcommand::Compile(ResearchCompileCommand {
					intent: Some(intent),
					source_issue: Some(source_issue),
					outcome: ResearchOutcomeArg::NeedsHumanDecision,
					json: true,
					..
				})
			}) if config == Path::new("./project.toml")
				&& intent == "research X"
				&& source_issue == "XY-860"
		));
	}

	#[test]
	fn parses_intake_issues_dry_run_with_project() {
		let cli = Cli::parse_from([
			"decodex",
			"intake",
			"issues",
			"--project",
			"decodex",
			"XY-1",
			"XY-2",
			"--dry-run",
			"--json",
		]);

		assert!(matches!(
			cli.command,
			Command::Intake(IntakeCommand {
				command: IntakeSubcommand::Issues(IntakeIssuesCommand {
					project: Some(_),
					dry_run: true,
					apply: false,
					json: true,
					issues,
					..
				})
			}) if issues == vec![String::from("XY-1"), String::from("XY-2")]
		));
	}

	#[test]
	fn parses_intake_issues_apply_with_project() {
		let cli = Cli::parse_from([
			"decodex",
			"intake",
			"issues",
			"--project",
			"decodex",
			"XY-1",
			"XY-2",
			"--apply",
			"--json",
		]);

		assert!(matches!(
			cli.command,
			Command::Intake(IntakeCommand {
				command: IntakeSubcommand::Issues(IntakeIssuesCommand {
					project: Some(_),
					dry_run: false,
					apply: true,
					json: true,
					issues,
					..
				})
			}) if issues == vec![String::from("XY-1"), String::from("XY-2")]
		));
	}

	#[test]
	fn parses_intake_goal_apply_with_project_and_team_anchor() {
		let cli = Cli::parse_from([
			"decodex",
			"intake",
			"goal",
			"--project",
			"decodex",
			"goal-intake-contract",
			"--apply",
			"--team-issue",
			"XY-852",
			"--json",
		]);

		assert!(matches!(
			cli.command,
			Command::Intake(IntakeCommand {
				command: IntakeSubcommand::Goal(IntakeGoalCommand {
					project: Some(_),
					contract_id,
					dry_run: false,
					apply: true,
					team_issue: Some(team_issue),
					json: true,
					..
				})
			}) if contract_id == "goal-intake-contract" && team_issue == "XY-852"
		));
	}

	#[test]
	fn rejects_intake_issues_without_explicit_mode() {
		let error = Cli::try_parse_from(["decodex", "intake", "issues", "XY-1"])
			.expect_err("intake issues requires dry-run or apply");

		assert!(error.to_string().contains("--dry-run") || error.to_string().contains("--apply"));
	}

	#[test]
	fn parses_research_promote_with_acceptance_metadata() {
		let cli = Cli::parse_from([
			"decodex",
			"research",
			"--config",
			"./project.toml",
			"promote",
			"research-design-contract",
			"--accepted-by",
			"operator",
			"--accepted-at",
			"2026-06-10T00:00:00Z",
			"--acceptance-source",
			"conversation",
			"--reason",
			"push this forward",
			"--json",
		]);

		assert!(matches!(
			cli.command,
			Command::Research(ResearchCommand {
				project_config: ProjectConfigArgs { config: Some(config) },
				command: ResearchSubcommand::Promote(ResearchPromoteCommand {
					contract_id,
					accepted_by,
					accepted_at: Some(accepted_at),
					acceptance_source,
					reason: Some(reason),
					json: true,
				})
			}) if config == Path::new("./project.toml")
				&& contract_id == "research-design-contract"
				&& accepted_by == "operator"
				&& accepted_at == "2026-06-10T00:00:00Z"
				&& acceptance_source == "conversation"
				&& reason == "push this forward"
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
				project: None,
				issue,
				run_id: Some(_),
				attempt: Some(2),
				json: true,
				include_payload: true,
			}) if config == Path::new("./project.toml") && issue == "PUB-101"
		));
	}

	#[test]
	fn parses_evidence_with_registered_project_id() {
		let cli = Cli::parse_from([
			"decodex",
			"evidence",
			"--project",
			"pubfi-mono",
			"PUB-101",
			"--json",
		]);

		assert!(matches!(
			cli.command,
			Command::Evidence(EvidenceCommand {
				project_config: ProjectConfigArgs { config: None },
				project: Some(project),
				issue,
				run_id: None,
				attempt: None,
				json: true,
				include_payload: false,
			}) if project == "pubfi-mono" && issue == "PUB-101"
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
	fn parses_ghost_lane_diagnose_with_issue_and_json() {
		let cli = Cli::parse_from([
			"decodex",
			"recover",
			"--config",
			"./project.toml",
			"ghost-lane",
			"diagnose",
			"PUBFI-012",
			"--json",
		]);

		assert!(matches!(
			cli.command,
			Command::Recover(RecoverCommand {
				project_config: ProjectConfigArgs { config: Some(config) },
				command: RecoverSubcommand::GhostLane(GhostLaneRecoveryCommand {
					command: GhostLaneRecoverySubcommand::Diagnose(
						GhostLaneDiagnoseCommand { issue: Some(_), json: true }
					)
				})
			}) if config == Path::new("./project.toml")
		));
	}

	#[test]
	fn parses_ghost_lane_cleanup_dry_run() {
		let cli = Cli::parse_from([
			"decodex",
			"recover",
			"ghost-lane",
			"cleanup",
			"PUBFI-012",
			"--dry-run",
		]);

		assert!(matches!(
			cli.command,
			Command::Recover(RecoverCommand {
				command: RecoverSubcommand::GhostLane(GhostLaneRecoveryCommand {
					command: GhostLaneRecoverySubcommand::Cleanup(
						GhostLaneCleanupCommand { issue, dry_run: true }
					)
				}),
				..
			}) if issue == "PUBFI-012"
		));
	}

	#[test]
	fn parses_stale_active_diagnose_with_issue_and_json() {
		let cli = Cli::parse_from([
			"decodex",
			"recover",
			"--config",
			"./project.toml",
			"stale-active",
			"diagnose",
			"PUB-1626",
			"--json",
		]);

		assert!(matches!(
			cli.command,
			Command::Recover(RecoverCommand {
				project_config: ProjectConfigArgs { config: Some(config) },
				command: RecoverSubcommand::StaleActive(StaleActiveRecoveryCommand {
					command: StaleActiveRecoverySubcommand::Diagnose(
						StaleActiveDiagnoseCommand { issue: Some(_), json: true }
					)
				})
			}) if config == Path::new("./project.toml")
		));
	}

	#[test]
	fn parses_stale_active_release_dry_run() {
		let cli = Cli::parse_from([
			"decodex",
			"recover",
			"stale-active",
			"release",
			"PUB-1626",
			"--dry-run",
		]);

		assert!(matches!(
			cli.command,
			Command::Recover(RecoverCommand {
				command: RecoverSubcommand::StaleActive(StaleActiveRecoveryCommand {
					command: StaleActiveRecoverySubcommand::Release(
						StaleActiveReleaseCommand { issue, dry_run: true }
					)
				}),
				..
			}) if issue == "PUB-1626"
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

	#[test]
	fn parses_review_handoff_adopt_dry_run() {
		let cli = Cli::parse_from([
			"decodex",
			"recover",
			"review-handoff",
			"adopt",
			"XY-944",
			"--pr",
			"https://github.com/hack-ink/decodex/pull/344",
			"--dry-run",
		]);

		assert!(matches!(
			cli.command,
			Command::Recover(RecoverCommand {
				command: RecoverSubcommand::ReviewHandoff(ReviewHandoffRecoveryCommand {
					command: ReviewHandoffRecoverySubcommand::Adopt(
						ReviewHandoffAdoptCommand { issue, pr, dry_run: true }
					)
				}),
				..
			}) if issue == "XY-944"
				&& pr == "https://github.com/hack-ink/decodex/pull/344"
		));
	}

	#[test]
	fn parses_legacy_closeout_manual_authority() {
		let cli = Cli::parse_from([
			"decodex",
			"recover",
			"legacy-closeout",
			"PUB-718",
			"--pr",
			"https://github.com/hack-ink/pubfi-mono-v2/pull/14",
			"--manual-authority",
		]);

		assert!(matches!(
			cli.command,
			Command::Recover(RecoverCommand {
				command: RecoverSubcommand::LegacyCloseout(LegacyCloseoutRecoveryCommand {
					issue,
					pr,
					dry_run: false,
					manual_authority: true,
				}),
				..
			}) if issue == "PUB-718"
				&& pr == "https://github.com/hack-ink/pubfi-mono-v2/pull/14"
		));
	}

	#[test]
	fn parses_merged_closeout_manual_authority() {
		let cli = Cli::parse_from([
			"decodex",
			"recover",
			"merged-closeout",
			"PUB-1549",
			"--pr",
			"https://github.com/helixbox/pubfi-mono/pull/309",
			"--manual-authority",
		]);

		assert!(matches!(
			cli.command,
			Command::Recover(RecoverCommand {
				command: RecoverSubcommand::MergedCloseout(MergedCloseoutRecoveryCommand {
					issue,
					pr,
					dry_run: false,
					manual_authority: true,
				}),
				..
			}) if issue == "PUB-1549"
				&& pr == "https://github.com/helixbox/pubfi-mono/pull/309"
		));
	}
}
