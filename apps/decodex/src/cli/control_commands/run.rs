use clap::Args;

use crate::{
	cli::ProjectConfigArgs,
	orchestrator::{self, RunOnceRequest},
	prelude::Result,
};

#[derive(Debug, Args)]
pub(in crate::cli) struct RunCommand {
	#[command(flatten)]
	pub(in crate::cli) project_config: ProjectConfigArgs,
	/// Run a specific leased or queued issue by Linear identifier or tracker issue id.
	#[arg(value_name = "ISSUE")]
	pub(in crate::cli) issue: Option<String>,
	/// Validate project loading, queue eligibility, and lane planning without tracker mutation.
	#[arg(long)]
	pub(in crate::cli) dry_run: bool,
	/// Explain current queued candidates without preparing or dispatching a lane.
	#[arg(long, requires = "dry_run", conflicts_with = "issue")]
	pub(in crate::cli) explain: bool,
}
impl RunCommand {
	pub(in crate::cli) fn run(&self) -> Result<()> {
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
