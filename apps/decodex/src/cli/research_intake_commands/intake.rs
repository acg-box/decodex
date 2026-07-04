use clap::{Args, Subcommand};

use crate::{
	cli::ProjectConfigArgs,
	prelude::Result,
	program_intake::{self, GoalIntakeCommandRequest, IssueBatchIntakeCommandRequest},
};

#[derive(Debug, Args)]
pub(in crate::cli) struct IntakeCommand {
	#[command(subcommand)]
	pub(in crate::cli) command: IntakeSubcommand,
}
impl IntakeCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
		match &self.command {
			IntakeSubcommand::Goal(args) => args.run(),
			IntakeSubcommand::Issues(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
pub(in crate::cli) struct IntakeGoalCommand {
	#[command(flatten)]
	pub(in crate::cli) project_config: ProjectConfigArgs,
	/// Registered Decodex service id to intake against.
	#[arg(long, value_name = "SERVICE_ID", conflicts_with = "config")]
	pub(in crate::cli) project: Option<String>,
	/// Read tracker state and print the deterministic goal-intake report without mutation.
	#[arg(long, conflicts_with = "apply", required_unless_present = "apply")]
	pub(in crate::cli) dry_run: bool,
	/// Create or update generated normal Linear issues and persist local Program Intake state.
	#[arg(long, conflicts_with = "dry_run")]
	pub(in crate::cli) apply: bool,
	/// Existing Linear issue whose team and startable state should anchor generated issues.
	#[arg(long, value_name = "ISSUE")]
	pub(in crate::cli) team_issue: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(in crate::cli) json: bool,
	/// Accepted Decision Contract identifier to materialize.
	#[arg(value_name = "CONTRACT_ID")]
	pub(in crate::cli) contract_id: String,
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
pub(in crate::cli) struct IntakeIssuesCommand {
	#[command(flatten)]
	pub(in crate::cli) project_config: ProjectConfigArgs,
	/// Registered Decodex service id to intake against.
	#[arg(long, value_name = "SERVICE_ID", conflicts_with = "config")]
	pub(in crate::cli) project: Option<String>,
	/// Read tracker state and print the deterministic intake report without local persistence.
	#[arg(long, conflicts_with = "apply", required_unless_present = "apply")]
	pub(in crate::cli) dry_run: bool,
	/// Persist local runtime Program Intake records for direct Program dispatch.
	#[arg(long, conflicts_with = "dry_run")]
	pub(in crate::cli) apply: bool,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(in crate::cli) json: bool,
	/// Existing Linear issue identifiers to intake into an internal Execution Program.
	#[arg(value_name = "ISSUE")]
	#[arg(required = true)]
	pub(in crate::cli) issues: Vec<String>,
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

#[derive(Debug, Subcommand)]
pub(in crate::cli) enum IntakeSubcommand {
	/// Dry-run or apply a promoted Decision Contract as normal issue-backed goal intake.
	Goal(IntakeGoalCommand),
	/// Dry-run or persist existing Linear issues as an internal program intake batch.
	Issues(IntakeIssuesCommand),
}
