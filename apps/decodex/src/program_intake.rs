//! Operator issue-batch intake for internal Execution Programs.

mod command;
mod goal;
mod goal_run;
mod issue_batch;
mod issue_batch_run;
mod model;
mod render;

pub(crate) use self::command::{run_goal_intake_command, run_issue_batch_intake_command};
pub(crate) use self::goal_run::run_goal_intake;
pub(crate) use self::issue_batch::{
	register_intake_project_config_for_persist, resolve_intake_project_config_path,
};
pub(crate) use self::issue_batch_run::run_issue_batch_intake;
pub(crate) use self::model::{
	GoalIntakeCommandRequest, GoalIntakeIssueAction, GoalIntakeIssueReport, GoalIntakeReport,
	GoalIntakeRunRequest, IssueBatchIntakeClassification, IssueBatchIntakeCommandRequest,
	IssueBatchIntakeCounts, IssueBatchIntakeIssueReport, IssueBatchIntakeReport,
};
#[cfg(test)]
pub(crate) use self::render::validate_generated_issue_text;
pub(crate) use self::render::{render_goal_intake_report, render_issue_batch_intake_report};

#[cfg(test)]
mod tests;
