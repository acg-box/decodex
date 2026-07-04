use std::{
	fs,
	io::{self, Read as _},
	path::{Path, PathBuf},
};

use clap::{Args, Subcommand, ValueEnum};

use crate::{
	cli::ProjectConfigArgs,
	prelude::{Result, eyre},
	research_design::{
		self, ResearchDesignCompileRequest, ResearchDesignOutcome, ResearchDesignPromoteRequest,
		ResearchDesignRunInput,
	},
};

#[derive(Debug, Args)]
pub(in crate::cli) struct ResearchCommand {
	#[command(flatten)]
	pub(in crate::cli) project_config: ProjectConfigArgs,
	#[command(subcommand)]
	pub(in crate::cli) command: ResearchSubcommand,
}
impl ResearchCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
		match &self.command {
			ResearchSubcommand::Compile(args) => args.run(self.project_config.as_path()),
			ResearchSubcommand::Promote(args) => args.run(self.project_config.as_path()),
		}
	}
}

#[derive(Debug, Args)]
pub(in crate::cli) struct ResearchCompileCommand {
	/// Structured research/design input JSON. Use `-` to read from stdin.
	#[arg(long, value_name = "JSON")]
	pub(in crate::cli) input: Option<PathBuf>,
	/// Natural-language research/design intent for minimal intake.
	#[arg(long, value_name = "TEXT", conflicts_with = "input")]
	pub(in crate::cli) intent: Option<String>,
	/// Source tracker issue identifier to link to the contract.
	#[arg(long, value_name = "ISSUE")]
	pub(in crate::cli) source_issue: Option<String>,
	/// Outcome for minimal natural-language intake.
	#[arg(long, value_enum, default_value = "not-decision-ready")]
	pub(in crate::cli) outcome: ResearchOutcomeArg,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(in crate::cli) json: bool,
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
pub(in crate::cli) struct ResearchPromoteCommand {
	/// Decision Contract identifier to promote.
	#[arg(value_name = "CONTRACT_ID")]
	pub(in crate::cli) contract_id: String,
	/// Actor accepting the contract.
	#[arg(long, value_name = "TEXT", default_value = "operator")]
	pub(in crate::cli) accepted_by: String,
	/// Acceptance source, usually conversation or runtime policy.
	#[arg(long, value_name = "TEXT", default_value = "conversation")]
	pub(in crate::cli) acceptance_source: String,
	/// RFC3339 acceptance timestamp. Defaults to current UTC time.
	#[arg(long, value_name = "RFC3339")]
	pub(in crate::cli) accepted_at: Option<String>,
	/// Optional acceptance reason.
	#[arg(long, value_name = "TEXT")]
	pub(in crate::cli) reason: Option<String>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(in crate::cli) json: bool,
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
pub(in crate::cli) enum ResearchSubcommand {
	/// Compile bounded research/design input into a latent Decision Contract.
	Compile(ResearchCompileCommand),
	/// Promote an accepted Decision Contract into execution authority.
	Promote(ResearchPromoteCommand),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub(in crate::cli) enum ResearchOutcomeArg {
	DecisionReady,
	NotDecisionReady,
	Blocked,
	NeedsHumanDecision,
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
