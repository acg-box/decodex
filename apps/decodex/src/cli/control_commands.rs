//! Runtime control-plane CLI command definitions.

use std::{
	path::{Path, PathBuf},
	time::Duration,
};

use clap::{Args, Subcommand};

use crate::{
	cli::ProjectConfigArgs,
	mcp::{self, McpCapabilityProfile, McpServeRequest, McpTransport},
	orchestrator::{
		self, DEFAULT_STEER_RESULT_WAIT_TIMEOUT, DiagnoseRequest, EvidenceRequest,
		LaneInspectRequest, LaneInterruptRequest, LaneSteerReport, LaneSteerRequest,
		RunOnceRequest, ServeRequest,
	},
	prelude::{Result, eyre},
	runtime,
};

#[derive(Debug, Args)]
pub(super) struct RunCommand {
	#[command(flatten)]
	pub(super) project_config: ProjectConfigArgs,
	/// Run a specific leased or queued issue by Linear identifier or tracker issue id.
	#[arg(value_name = "ISSUE")]
	pub(super) issue: Option<String>,
	/// Validate project loading, queue eligibility, and lane planning without tracker mutation.
	#[arg(long)]
	pub(super) dry_run: bool,
	/// Explain current queued candidates without preparing or dispatching a lane.
	#[arg(long, requires = "dry_run", conflicts_with = "issue")]
	pub(super) explain: bool,
}
impl RunCommand {
	pub(super) fn run(&self) -> Result<()> {
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
pub(super) struct ServeCommand {
	#[command(flatten)]
	pub(super) project_config: ProjectConfigArgs,
	/// Operator UI listen address.
	#[arg(long, value_name = "ADDR", default_value_t = orchestrator::DEFAULT_OPERATOR_LISTEN_ADDRESS.to_owned())]
	pub(super) listen_address: String,
	/// Start the local dev endpoint without polling or dispatching projects.
	#[arg(long, hide = true)]
	pub(super) dev: bool,
}
impl ServeCommand {
	pub(super) fn run(&self) -> Result<()> {
		orchestrator::run_control_plane(ServeRequest {
			config_path: self.project_config.as_path(),
			listen_address: &self.listen_address,
			dev: self.dev,
		})
	}
}

#[derive(Debug, Args)]
pub(super) struct McpCommand {
	#[command(subcommand)]
	pub(super) command: McpSubcommand,
}
impl McpCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			McpSubcommand::Serve(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
pub(super) struct McpServeCommand {
	#[command(flatten)]
	pub(super) project_config: ProjectConfigArgs,
	/// MCP transport.
	#[arg(long, value_enum, default_value_t = McpTransport::Stdio)]
	pub(super) transport: McpTransport,
	/// Capability profile exposed by the MCP gateway. Defaults to admin for stdio and observe for
	/// Streamable HTTP.
	#[arg(long, value_enum)]
	pub(super) capability_profile: Option<McpCapabilityProfile>,
	/// Streamable HTTP listen address.
	#[arg(long, value_name = "ADDR", default_value_t = mcp::DEFAULT_MCP_HTTP_LISTEN_ADDRESS.to_owned())]
	pub(super) listen_address: String,
	/// Trusted browser Origin for Streamable HTTP. Repeat for multiple origins.
	#[arg(long = "allow-origin", value_name = "ORIGIN")]
	pub(super) allowed_origins: Vec<String>,
	/// Environment variable containing the Streamable HTTP bearer token.
	#[arg(long = "bearer-token-env", value_name = "ENV_VAR")]
	pub(super) bearer_token_env: Option<String>,
}
impl McpServeCommand {
	pub(super) fn run(&self) -> Result<()> {
		mcp::serve(McpServeRequest {
			transport: self.transport,
			config_path: self.project_config.as_path(),
			capability_profile: self.effective_capability_profile(),
			listen_address: &self.listen_address,
			allowed_origins: &self.allowed_origins,
			bearer_token_env: self.bearer_token_env.as_deref(),
		})
	}

	pub(super) fn effective_capability_profile(&self) -> McpCapabilityProfile {
		self.capability_profile.unwrap_or_else(|| self.transport.default_capability_profile())
	}
}

#[derive(Debug, Args)]
pub(super) struct ProjectCommand {
	#[command(subcommand)]
	pub(super) command: ProjectSubcommand,
}
impl ProjectCommand {
	pub(super) fn run(&self) -> Result<()> {
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
pub(super) struct ProjectAddCommand {
	/// Path to a Decodex project directory containing `project.toml` and `WORKFLOW.md`.
	#[arg(value_name = "PROJECT_DIR")]
	pub(super) config: PathBuf,
}

#[derive(Debug, Args)]
pub(super) struct ProjectToggleCommand {
	/// Project service id from the registered Decodex config.
	#[arg(value_name = "SERVICE_ID")]
	pub(super) service_id: String,
}

#[derive(Debug, Args)]
pub(super) struct LaneCommand {
	#[command(flatten)]
	pub(super) project_config: ProjectConfigArgs,
	#[command(subcommand)]
	pub(super) command: LaneSubcommand,
}
impl LaneCommand {
	pub(super) fn run(&self) -> Result<()> {
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
pub(super) struct LaneInspectCommand {
	/// Issue identifier or local issue id to inspect.
	#[arg(value_name = "ISSUE")]
	pub(super) issue: String,
	/// Restrict inspection to one run id.
	#[arg(long, value_name = "RUN_ID")]
	pub(super) run_id: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
}

#[derive(Debug, Args)]
pub(super) struct LaneInterruptCommand {
	/// Issue identifier or local issue id to interrupt.
	#[arg(value_name = "ISSUE")]
	pub(super) issue: String,
	/// Run id for the active app-server turn to interrupt.
	#[arg(long, value_name = "RUN_ID")]
	pub(super) run_id: String,
	/// Use hard process-kill fallback when soft interrupt is unavailable or fails.
	#[arg(long)]
	pub(super) force: bool,
	/// Operator-visible reason retained in local private evidence.
	#[arg(long, value_name = "TEXT")]
	pub(super) reason: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
}

#[derive(Debug, Args)]
pub(super) struct LaneSteerCommand {
	/// Issue identifier or local issue id for the current lane.
	#[arg(value_name = "ISSUE")]
	pub(super) issue: String,
	/// Run id that must own the active turn.
	#[arg(long, value_name = "RUN_ID")]
	pub(super) run_id: String,
	/// Current active app-server turn id precondition.
	#[arg(long, value_name = "TURN_ID")]
	pub(super) expected_turn_id: String,
	/// Operator-supplied steer text to send to the active turn.
	#[arg(long, value_name = "TEXT")]
	pub(super) message: String,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
	/// How long to wait for the active attempt to report delivery.
	#[arg(long, value_name = "MILLISECONDS", default_value_t = default_lane_steer_wait_timeout_ms())]
	pub(super) wait_timeout_ms: u64,
}
impl LaneSteerCommand {
	pub(super) fn run(&self, config_path: Option<&Path>) -> Result<()> {
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
pub(super) struct StatusCommand {
	#[command(flatten)]
	pub(super) project_config: ProjectConfigArgs,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
	/// Maximum number of recent runs to display.
	#[arg(long, value_name = "COUNT", default_value_t = orchestrator::DEFAULT_STATUS_RUN_LIMIT)]
	pub(super) limit: usize,
	/// Refresh live tracker and pull-request observers before printing status.
	#[arg(long)]
	pub(super) live: bool,
}
impl StatusCommand {
	pub(super) fn run(&self) -> Result<()> {
		orchestrator::print_status(self.project_config.as_path(), self.json, self.limit, self.live)
	}
}

#[derive(Debug, Args)]
pub(super) struct DiagnoseCommand {
	#[command(flatten)]
	pub(super) project_config: ProjectConfigArgs,
	/// Emit the agent handoff index JSON instead of a one-line path summary.
	#[arg(long)]
	pub(super) json: bool,
	/// Maximum number of recent runs to include while generating evidence.
	#[arg(long, value_name = "COUNT", default_value_t = orchestrator::DEFAULT_STATUS_RUN_LIMIT)]
	pub(super) limit: usize,
}
impl DiagnoseCommand {
	pub(super) fn run(&self) -> Result<()> {
		orchestrator::run_diagnose(DiagnoseRequest {
			config_path: self.project_config.as_path(),
			json: self.json,
			limit: self.limit,
		})
	}
}

#[derive(Debug, Args)]
pub(super) struct EvidenceCommand {
	#[command(flatten)]
	pub(super) project_config: ProjectConfigArgs,
	/// Resolve this evidence readback through a registered Decodex project id.
	#[arg(long, value_name = "SERVICE_ID")]
	pub(super) project: Option<String>,
	/// Issue identifier or local issue id to inspect.
	pub(super) issue: String,
	/// Restrict readback to one run id. Defaults to the latest local run for the issue.
	#[arg(long, value_name = "RUN_ID")]
	pub(super) run_id: Option<String>,
	/// Restrict readback to one attempt number. Defaults to the selected run attempt.
	#[arg(long, value_name = "NUMBER")]
	pub(super) attempt: Option<i64>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
	/// Include full structured payload values instead of compact payload summaries only.
	#[arg(long)]
	pub(super) include_payload: bool,
}
impl EvidenceCommand {
	pub(super) fn run(&self) -> Result<()> {
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

#[derive(Debug, Subcommand)]
pub(super) enum ProjectSubcommand {
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
pub(super) enum McpSubcommand {
	/// Serve Decodex MCP protocol primitives.
	Serve(McpServeCommand),
}

#[derive(Debug, Subcommand)]
pub(super) enum LaneSubcommand {
	/// Inspect one local lane by issue identifier or tracker issue id.
	Inspect(LaneInspectCommand),
	/// Soft-interrupt an active app-server turn, with optional hard fallback.
	Interrupt(LaneInterruptCommand),
	/// Send operator-supplied text to an active steerable turn.
	Steer(LaneSteerCommand),
}

fn default_lane_steer_wait_timeout_ms() -> u64 {
	u64::try_from(DEFAULT_STEER_RESULT_WAIT_TIMEOUT.as_millis()).unwrap_or(10_000)
}

fn lane_steer_report_is_failure(report: &LaneSteerReport) -> bool {
	matches!(report.outcome.as_str(), "rejected" | "failed" | "timed_out" | "fallback")
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
