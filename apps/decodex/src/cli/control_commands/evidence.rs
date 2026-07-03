use clap::Args;

use crate::{
	cli::ProjectConfigArgs,
	orchestrator::{self, EvidenceRequest},
	prelude::Result,
};

#[derive(Debug, Args)]
pub(in crate::cli) struct EvidenceCommand {
	#[command(flatten)]
	pub(in crate::cli) project_config: ProjectConfigArgs,
	/// Resolve this evidence readback through a registered Decodex project id.
	#[arg(long, value_name = "SERVICE_ID")]
	pub(in crate::cli) project: Option<String>,
	/// Issue identifier or local issue id to inspect.
	pub(in crate::cli) issue: String,
	/// Restrict readback to one run id. Defaults to the latest local run for the issue.
	#[arg(long, value_name = "RUN_ID")]
	pub(in crate::cli) run_id: Option<String>,
	/// Restrict readback to one attempt number. Defaults to the selected run attempt.
	#[arg(long, value_name = "NUMBER")]
	pub(in crate::cli) attempt: Option<i64>,
	/// Emit structured JSON instead of human-readable text.
	#[arg(long)]
	pub(in crate::cli) json: bool,
	/// Include full structured payload values instead of compact payload summaries only.
	#[arg(long)]
	pub(in crate::cli) include_payload: bool,
}
impl EvidenceCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
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
