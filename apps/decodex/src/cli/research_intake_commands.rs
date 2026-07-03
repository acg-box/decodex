//! Research, intake, archive, and maintenance CLI command definitions.

use std::{
	fs,
	io::{self, Read as _},
	path::{Path, PathBuf},
};

use clap::{Args, Subcommand, ValueEnum};

use crate::{
	archive_hygiene::{self, ArchiveHygieneRequest},
	cli::ProjectConfigArgs,
	maintenance::{self, MaintenanceMode, MaintenancePruneRequest, MaintenanceScope},
	prelude::{Result, eyre},
	program_intake::{self, GoalIntakeCommandRequest, IssueBatchIntakeCommandRequest},
	research_design::{
		self, ResearchDesignCompileRequest, ResearchDesignOutcome, ResearchDesignPromoteRequest,
		ResearchDesignRunInput,
	},
};

#[derive(Debug, Args)]
pub(super) struct IntakeCommand {
	#[command(subcommand)]
	pub(super) command: IntakeSubcommand,
}
impl IntakeCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			IntakeSubcommand::Goal(args) => args.run(),
			IntakeSubcommand::Issues(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
pub(super) struct IntakeGoalCommand {
	#[command(flatten)]
	pub(super) project_config: ProjectConfigArgs,
	/// Registered Decodex service id to intake against.
	#[arg(long, value_name = "SERVICE_ID", conflicts_with = "config")]
	pub(super) project: Option<String>,
	/// Read tracker state and print the deterministic goal-intake report without mutation.
	#[arg(long, conflicts_with = "apply", required_unless_present = "apply")]
	pub(super) dry_run: bool,
	/// Create or update generated normal Linear issues and persist local Program Intake state.
	#[arg(long, conflicts_with = "dry_run")]
	pub(super) apply: bool,
	/// Existing Linear issue whose team and startable state should anchor generated issues.
	#[arg(long, value_name = "ISSUE")]
	pub(super) team_issue: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
	/// Accepted Decision Contract identifier to materialize.
	#[arg(value_name = "CONTRACT_ID")]
	pub(super) contract_id: String,
}
impl IntakeGoalCommand {
	pub(super) fn run(&self) -> Result<()> {
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
pub(super) struct IntakeIssuesCommand {
	#[command(flatten)]
	pub(super) project_config: ProjectConfigArgs,
	/// Registered Decodex service id to intake against.
	#[arg(long, value_name = "SERVICE_ID", conflicts_with = "config")]
	pub(super) project: Option<String>,
	/// Read tracker state and print the deterministic intake report without local persistence.
	#[arg(long, conflicts_with = "apply", required_unless_present = "apply")]
	pub(super) dry_run: bool,
	/// Persist local runtime Program Intake records for direct Program dispatch.
	#[arg(long, conflicts_with = "dry_run")]
	pub(super) apply: bool,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
	/// Existing Linear issue identifiers to intake into an internal Execution Program.
	#[arg(value_name = "ISSUE")]
	#[arg(required = true)]
	pub(super) issues: Vec<String>,
}
impl IntakeIssuesCommand {
	pub(super) fn run(&self) -> Result<()> {
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
pub(super) struct ResearchCommand {
	#[command(flatten)]
	pub(super) project_config: ProjectConfigArgs,
	#[command(subcommand)]
	pub(super) command: ResearchSubcommand,
}
impl ResearchCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			ResearchSubcommand::Compile(args) => args.run(self.project_config.as_path()),
			ResearchSubcommand::Promote(args) => args.run(self.project_config.as_path()),
		}
	}
}

#[derive(Debug, Args)]
pub(super) struct ResearchCompileCommand {
	/// Structured research/design input JSON. Use `-` to read from stdin.
	#[arg(long, value_name = "JSON")]
	pub(super) input: Option<PathBuf>,
	/// Natural-language research/design intent for minimal intake.
	#[arg(long, value_name = "TEXT", conflicts_with = "input")]
	pub(super) intent: Option<String>,
	/// Source tracker issue identifier to link to the contract.
	#[arg(long, value_name = "ISSUE")]
	pub(super) source_issue: Option<String>,
	/// Outcome for minimal natural-language intake.
	#[arg(long, value_enum, default_value = "not-decision-ready")]
	pub(super) outcome: ResearchOutcomeArg,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
}
impl ResearchCompileCommand {
	pub(super) fn run(&self, config_path: Option<&Path>) -> Result<()> {
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
pub(super) struct ResearchPromoteCommand {
	/// Decision Contract identifier to promote.
	#[arg(value_name = "CONTRACT_ID")]
	pub(super) contract_id: String,
	/// Actor accepting the contract.
	#[arg(long, value_name = "TEXT", default_value = "operator")]
	pub(super) accepted_by: String,
	/// Acceptance source, usually conversation or runtime policy.
	#[arg(long, value_name = "TEXT", default_value = "conversation")]
	pub(super) acceptance_source: String,
	/// RFC3339 acceptance timestamp. Defaults to current UTC time.
	#[arg(long, value_name = "RFC3339")]
	pub(super) accepted_at: Option<String>,
	/// Optional acceptance reason.
	#[arg(long, value_name = "TEXT")]
	pub(super) reason: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(super) json: bool,
}
impl ResearchPromoteCommand {
	pub(super) fn run(&self, config_path: Option<&Path>) -> Result<()> {
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
pub(super) struct ArchiveLinearCommand {
	#[command(flatten)]
	pub(super) project_config: ProjectConfigArgs,
	/// Repo label scope to inspect, for example `repo:decodex`.
	#[arg(long = "repo-label", value_name = "LABEL", required = true)]
	pub(super) repo_labels: Vec<String>,
	/// Archive only issues last updated more than this many days ago.
	#[arg(long, value_name = "DAYS", default_value_t = 30)]
	pub(super) older_than_days: u32,
	/// Perform the archive mutation. Omit this flag for the dry-run candidate report.
	#[arg(long)]
	pub(super) execute: bool,
}
impl ArchiveLinearCommand {
	pub(super) fn run(&self) -> Result<()> {
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
pub(super) struct MaintenanceCommand {
	#[command(subcommand)]
	pub(super) command: MaintenanceSubcommand,
}
impl MaintenanceCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			MaintenanceSubcommand::Prune(args) => args.run(),
		}
	}
}

#[derive(Debug, Args)]
pub(super) struct MaintenancePruneCommand {
	/// Report candidates without applying retention changes. This is the default mode.
	#[arg(long, conflicts_with = "apply")]
	pub(super) dry_run: bool,
	/// Apply safe file retention, state-aware runtime compaction, and WAL checkpointing.
	#[arg(long, conflicts_with = "dry_run")]
	pub(super) apply: bool,
	/// Emit the maintenance report as JSON.
	#[arg(long)]
	pub(super) json: bool,
}
impl MaintenancePruneCommand {
	pub(super) fn run(&self) -> Result<()> {
		let mode = if self.apply { MaintenanceMode::Apply } else { MaintenanceMode::DryRun };

		maintenance::run_prune_command(MaintenancePruneRequest {
			mode,
			scope: MaintenanceScope::Full,
			json: self.json,
		})
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

#[derive(Debug, Subcommand)]
pub(super) enum ResearchSubcommand {
	/// Compile bounded research/design input into a latent Decision Contract.
	Compile(ResearchCompileCommand),
	/// Promote an accepted Decision Contract into execution authority.
	Promote(ResearchPromoteCommand),
}

#[derive(Debug, Subcommand)]
pub(super) enum IntakeSubcommand {
	/// Dry-run or apply a promoted Decision Contract as normal issue-backed goal intake.
	Goal(IntakeGoalCommand),
	/// Dry-run or persist existing Linear issues as an internal program intake batch.
	Issues(IntakeIssuesCommand),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(super) enum ResearchOutcomeArg {
	DecisionReady,
	NotDecisionReady,
	Blocked,
	NeedsHumanDecision,
}

#[derive(Debug, Subcommand)]
pub(super) enum MaintenanceSubcommand {
	/// Inspect or apply local Decodex storage retention.
	Prune(MaintenancePruneCommand),
}

fn research_outcome_label(outcome: ResearchDesignOutcome) -> &'static str {
	match outcome {
		ResearchDesignOutcome::DecisionReady => "decision-ready",
		ResearchDesignOutcome::NotDecisionReady => "not-decision-ready",
		ResearchDesignOutcome::Blocked => "blocked",
		ResearchDesignOutcome::NeedsHumanDecision => "needs-human-decision",
	}
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
