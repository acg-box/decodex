//! Operator issue-batch intake for internal Execution Programs.

mod command;
mod goal;
mod goal_run;
mod issue_batch;
mod issue_batch_run;
mod model;
mod readiness;
mod render;

#[cfg(test)] pub(crate) use self::render::validate_generated_issue_text;
pub(crate) use self::{
	command::{run_goal_intake_command, run_issue_batch_intake_command},
	goal_run::run_goal_intake,
	issue_batch::{register_intake_project_config_for_persist, resolve_intake_project_config_path},
	issue_batch_run::run_issue_batch_intake,
	model::{
		GoalIntakeCommandRequest, GoalIntakeIssueAction, GoalIntakeIssueReport, GoalIntakeReport,
		GoalIntakeRunRequest, IssueBatchIntakeClassification, IssueBatchIntakeCommandRequest,
		IssueBatchIntakeCounts, IssueBatchIntakeIssueReport, IssueBatchIntakeReport,
	},
	render::{render_goal_intake_report, render_issue_batch_intake_report},
};

pub(crate) fn goal_program_id(service_id: &str, contract_id: &str) -> String {
	goal::goal_program_id(service_id, contract_id)
}

#[cfg(test)] mod tests;
