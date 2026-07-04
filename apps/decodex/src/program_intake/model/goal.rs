use crate::{
	execution_program::{ExecutionConflictDomain, ExecutionProgramNodeStage, ExecutionQueueIntent},
	loop_contract::DecisionContract,
	state::StateStore,
	tracker::{IssueTracker, TrackerIssue},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GoalIssuePlan {
	pub(crate) key: String,
	pub(crate) node_id: String,
	pub(crate) title: String,
	pub(crate) objective: String,
	pub(crate) stage: ExecutionProgramNodeStage,
	pub(crate) queue_intent: ExecutionQueueIntent,
	pub(crate) description: String,
	pub(crate) dependencies: Vec<String>,
	pub(crate) dependency_node_ids: Vec<String>,
	pub(crate) conflict_domains: Vec<ExecutionConflictDomain>,
	pub(crate) acceptance: Vec<String>,
	pub(crate) validation: Vec<String>,
	pub(crate) risk: Vec<String>,
}

pub(crate) struct GoalIntakeAnchor {
	pub(crate) team_id: String,
	pub(crate) state_id: String,
}

pub(crate) struct GoalIssueBriefInput<'a> {
	pub(crate) contract: &'a DecisionContract,
	pub(crate) objective: &'a str,
	pub(crate) dependencies: &'a [String],
	pub(crate) conflict_domains: &'a [ExecutionConflictDomain],
	pub(crate) acceptance: &'a [String],
	pub(crate) validation: &'a [String],
	pub(crate) risk: &'a [String],
}

pub(crate) struct ApplyGoalIssuesInput<'a, T>
where
	T: IssueTracker + ?Sized,
{
	pub(crate) state_store: &'a StateStore,
	pub(crate) service_id: &'a str,
	pub(crate) source_issue_id: Option<&'a str>,
	pub(crate) tracker: &'a T,
	pub(crate) contract: &'a DecisionContract,
	pub(crate) plans: &'a [GoalIssuePlan],
	pub(crate) linked_issues: &'a [Option<TrackerIssue>],
	pub(crate) anchor: &'a GoalIntakeAnchor,
}
