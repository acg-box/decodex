use clap::{Args, Subcommand};

use crate::{
	autonomy_runtime_policy::{self, RuntimePolicyProgramIntakeState},
	cli::ProjectConfigArgs,
	prelude::{Result, eyre},
	program_intake::{self, GoalIntakeCommandRequest, IssueBatchIntakeCommandRequest},
	runtime,
	state::ProgramIntakeAttemptStatus,
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
			IntakeSubcommand::Recover(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
pub(in crate::cli) struct IntakeRecoverCommand {
	/// Registered Decodex service id whose canonical intake claim is being inspected.
	#[arg(long, value_name = "SERVICE_ID")]
	pub(in crate::cli) project: String,
	/// Accepted Decision Contract identifier whose one-shot intake claim is being recovered.
	#[arg(value_name = "CONTRACT_ID")]
	pub(in crate::cli) contract_id: String,
	/// Existing Linear issue used by the original prepared attempt as its team anchor.
	#[arg(long, value_name = "ISSUE")]
	pub(in crate::cli) team_issue: Option<String>,
	#[command(subcommand)]
	pub(in crate::cli) action: IntakeRecoverAction,
}
impl IntakeRecoverCommand {
	fn run(&self) -> Result<()> {
		let store = runtime::open_runtime_store()?;
		let status = store.program_intake_attempt_status(&self.project, &self.contract_id)?;
		let intake_state = autonomy_runtime_policy::program_intake_state_for_contract(
			&store,
			&self.project,
			&self.contract_id,
		)?;

		match self.action {
			IntakeRecoverAction::Inspect => {},
			IntakeRecoverAction::RetryPrepared => {
				if status != Some(ProgramIntakeAttemptStatus::Prepared) {
					eyre::bail!("Program Intake retry is allowed only from prepared state.");
				}

				program_intake::run_goal_intake_command(GoalIntakeCommandRequest {
					config_path: None,
					project_id: Some(&self.project),
					contract_id: &self.contract_id,
					team_issue_identifier: self.team_issue.as_deref(),
					dry_run: false,
					apply: true,
				})?;
			},
			IntakeRecoverAction::CompleteAfterReadback => {
				if status != Some(ProgramIntakeAttemptStatus::Started)
					|| intake_state != RuntimePolicyProgramIntakeState::Complete
				{
					eyre::bail!(
						"Program Intake can be reconciled complete only from started state with exact complete readback."
					);
				}

				store.complete_program_intake_attempt(&self.project, &self.contract_id)?;
			},
		}

		let final_status = store.program_intake_attempt_status(&self.project, &self.contract_id)?;
		let final_intake_state = autonomy_runtime_policy::program_intake_state_for_contract(
			&store,
			&self.project,
			&self.contract_id,
		)?;

		println!(
			"project={} contract={} attempt_status={} intake_state={}",
			self.project,
			self.contract_id,
			attempt_status_name(final_status),
			final_intake_state.as_str()
		);

		Ok(())
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

#[derive(Clone, Copy, Debug, Subcommand)]
pub(in crate::cli) enum IntakeRecoverAction {
	/// Read the canonical claim and exact Program Intake correspondence without mutation.
	Inspect,
	/// Retry a prepared, pre-mutation claim with its exact bound inputs.
	RetryPrepared,
	/// Mark a started claim complete only after exact Program Intake readback succeeds.
	CompleteAfterReadback,
}

#[derive(Debug, Subcommand)]
pub(in crate::cli) enum IntakeSubcommand {
	/// Dry-run or apply a promoted Decision Contract as normal issue-backed goal intake.
	Goal(IntakeGoalCommand),
	/// Dry-run or persist existing Linear issues as an internal program intake batch.
	Issues(IntakeIssuesCommand),
	/// Inspect or reconcile the canonical one-shot claim without raw database edits.
	Recover(IntakeRecoverCommand),
}

fn attempt_status_name(status: Option<ProgramIntakeAttemptStatus>) -> &'static str {
	match status {
		None => "absent",
		Some(ProgramIntakeAttemptStatus::Prepared) => "prepared",
		Some(ProgramIntakeAttemptStatus::Started) => "started",
		Some(ProgramIntakeAttemptStatus::Completed) => "completed",
	}
}
