use std::{path::Path, time::Duration};

use clap::{Args, Subcommand};

use crate::{
	cli::ProjectConfigArgs,
	orchestrator::{
		self, DEFAULT_STEER_RESULT_WAIT_TIMEOUT, LaneInspectRequest, LaneInterruptRequest,
		LaneSteerReport, LaneSteerRequest,
	},
	prelude::{Result, eyre},
};

#[derive(Debug, Args)]
pub(in crate::cli) struct LaneCommand {
	#[command(flatten)]
	pub(in crate::cli) project_config: ProjectConfigArgs,
	#[command(subcommand)]
	pub(in crate::cli) command: LaneSubcommand,
}
impl LaneCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
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
pub(in crate::cli) struct LaneInspectCommand {
	/// Issue identifier or local issue id to inspect.
	#[arg(value_name = "ISSUE")]
	pub(in crate::cli) issue: String,
	/// Restrict inspection to one run id.
	#[arg(long, value_name = "RUN_ID")]
	pub(in crate::cli) run_id: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(in crate::cli) json: bool,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct LaneInterruptCommand {
	/// Issue identifier or local issue id to interrupt.
	#[arg(value_name = "ISSUE")]
	pub(in crate::cli) issue: String,
	/// Run id for the active app-server turn to interrupt.
	#[arg(long, value_name = "RUN_ID")]
	pub(in crate::cli) run_id: String,
	/// Use hard process-kill fallback when soft interrupt is unavailable or fails.
	#[arg(long)]
	pub(in crate::cli) force: bool,
	/// Operator-visible reason retained in local private evidence.
	#[arg(long, value_name = "TEXT")]
	pub(in crate::cli) reason: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(in crate::cli) json: bool,
}

#[derive(Debug, Args)]
pub(in crate::cli) struct LaneSteerCommand {
	/// Issue identifier or local issue id for the current lane.
	#[arg(value_name = "ISSUE")]
	pub(in crate::cli) issue: String,
	/// Run id that must own the active turn.
	#[arg(long, value_name = "RUN_ID")]
	pub(in crate::cli) run_id: String,
	/// Current active app-server turn id precondition.
	#[arg(long, value_name = "TURN_ID")]
	pub(in crate::cli) expected_turn_id: String,
	/// Operator-supplied steer text to send to the active turn.
	#[arg(long, value_name = "TEXT")]
	pub(in crate::cli) message: String,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(in crate::cli) json: bool,
	/// How long to wait for the active attempt to report delivery.
	#[arg(long, value_name = "MILLISECONDS", default_value_t = default_lane_steer_wait_timeout_ms())]
	pub(in crate::cli) wait_timeout_ms: u64,
}
impl LaneSteerCommand {
	pub(in crate::cli) fn run(&self, config_path: Option<&Path>) -> Result<()> {
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

#[derive(Debug, Subcommand)]
pub(in crate::cli) enum LaneSubcommand {
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
