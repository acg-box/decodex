use serde_json::{self, Value};

use crate::{
	program_intake::{self, GoalIntakeIssueReport, GoalIntakeReport},
	research_design::ResearchDesignRunReport,
};

pub(in crate::mcp) fn research_compile_result(
	report: &ResearchDesignRunReport,
	persisted: bool,
	mode: &str,
) -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.research_compile_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"contract_id": report.contract_id,
		"contract_status": report.contract_status.as_str(),
		"ready_for_issue_shaping": report.ready_for_issue_shaping,
		"issue_generation_ready_after_promotion": report.issue_generation_ready_after_promotion,
		"execution_authority_granted": report.execution_authority_granted,
		"proposed_issue_count": report.proposed_issues.len(),
		"promotion_targets": report.promotion_targets,
		"conflict_domains": report.conflict_domains,
		"next_action": if persisted {
			"Promote the Decision Contract only after explicit acceptance."
		} else {
			"Re-run with mode=apply and explicit authority to persist a latent Decision Contract."
		}
	})
}

pub(in crate::mcp) fn research_promote_readiness_result(
	contract_id: &str,
	contract_status: &str,
	ready_for_issue_shaping: bool,
	persisted: bool,
	mode: &str,
) -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.research_promote_result/1",
		"status": "ok",
		"mode": mode,
		"persisted": persisted,
		"contract_id": contract_id,
		"contract_status": contract_status,
		"execution_authority_granted": persisted && contract_status == "accepted_promoted",
		"ready_for_issue_shaping": ready_for_issue_shaping,
		"next_action": if persisted {
			"Use intake_goal dry_run to inspect issue shaping before apply."
		} else {
			"Re-run with mode=apply and explicit acceptance authority to promote."
		}
	})
}

pub(in crate::mcp) fn intake_goal_result(report: &GoalIntakeReport, mode: &str) -> Value {
	let issues = report.issues.iter().map(intake_goal_issue_result).collect::<Vec<_>>();

	serde_json::json!({
		"schema": "decodex.mcp.intake_goal_result/1",
		"status": "ok",
		"mode": mode,
		"service_id": report.service_id,
		"contract_id": report.contract_id,
		"dry_run": report.dry_run,
		"applied": report.applied,
		"persisted": report.persisted,
		"issue_count": issues.len(),
		"issues": issues,
		"next_action": if report.persisted {
			"Let the Program scheduler dispatch ready mapped issues; do not add queue labels manually."
		} else {
			"Review the public issue split, then re-run with mode=apply and explicit authority if accepted."
		}
	})
}

fn intake_goal_issue_result(row: &GoalIntakeIssueReport) -> Value {
	serde_json::json!({
		"title": row.title,
		"objective": row.objective,
		"issue_identifier": row.issue_identifier,
		"action": goal_intake_action_name(row.action),
		"dependencies": row.dependencies,
		"conflict_domains": row.conflict_domains,
		"acceptance": row.acceptance,
		"validation": row.validation,
		"reasons": row.reasons
	})
}

fn goal_intake_action_name(action: program_intake::GoalIntakeIssueAction) -> &'static str {
	match action {
		program_intake::GoalIntakeIssueAction::WouldCreate => "would_create",
		program_intake::GoalIntakeIssueAction::WouldUpdate => "would_update",
		program_intake::GoalIntakeIssueAction::Created => "created",
		program_intake::GoalIntakeIssueAction::Updated => "updated",
	}
}
