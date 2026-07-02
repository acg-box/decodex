mod authority;
mod linking;
mod parsing;
mod planning;
mod program;
mod reporting;

pub(super) use self::{
	authority::ensure_goal_intake_authority,
	linking::{apply_goal_issues_and_link_contract, goal_intake_anchor, linked_goal_issues},
	parsing::{
		conflict_domain_labels, goal_node_id, goal_objective_lineage, goal_program_id,
		goal_proposed_issue_conflict_domains, parse_goal_queue_intent, parse_goal_stage,
	},
	planning::goal_issue_plans,
	program::goal_execution_program,
	reporting::{applied_goal_issue_rows, dry_run_goal_issue_rows},
};
