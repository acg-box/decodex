use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::execution_program::{
	ExecutionNodeEvaluation, ExecutionProgramEvaluation, ExecutionProgramOperatorSummary,
};
use crate::state::ExecutionProgramRecord;

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorExecutionProgramStatus {
	pub(crate) program_id: String,
	#[serde(default = "operator_execution_program_unknown_status")]
	pub(crate) status: String,
	pub(crate) source_contract_id: Option<String>,

	pub(crate) intake_kind: Option<String>,

	pub(crate) public_summary: Option<String>,
	pub(crate) node_count: usize,
	pub(crate) planned_count: usize,
	pub(crate) mapped_count: usize,
	pub(crate) ready_count: usize,
	pub(crate) queued_count: usize,
	pub(crate) blocked_count: usize,
	pub(crate) held_count: usize,
	pub(crate) active_count: usize,
	pub(crate) needs_attention_count: usize,
	pub(crate) completed_count: usize,
	pub(crate) stale_count: usize,
	pub(crate) superseded_count: usize,
	pub(crate) dispatchable_count: usize,
	pub(crate) mapped_issue_identifiers: Vec<String>,
	#[serde(default)]
	pub(crate) node_readbacks: Vec<OperatorExecutionProgramNodeStatus>,
	pub(crate) readback_warning: Option<String>,
}
impl OperatorExecutionProgramStatus {
	pub(crate) fn from_summary(
		record: &ExecutionProgramRecord,
		summary: ExecutionProgramOperatorSummary,
		evaluation: &ExecutionProgramEvaluation,
	) -> Self {
		let program_intake_plan = record.program().program_intake_plan();

		Self {
			status: operator_execution_program_status(
				&summary,
				record.program().nodes().len(),
				None,
			),
			program_id: summary.program_id.clone(),
			source_contract_id: record.source_contract_id().map(str::to_owned),
			intake_kind: program_intake_plan.map(|plan| plan.intake_kind().as_str().to_owned()),
			public_summary: program_intake_plan.map(|plan| plan.public_summary().to_owned()),
			node_count: record.program().nodes().len(),
			planned_count: summary.planned_count,
			mapped_count: summary.mapped_count,
			ready_count: summary.ready_count,
			queued_count: summary.queued_count,
			blocked_count: summary.blocked_count,
			held_count: summary.held_count,
			active_count: summary.active_count,
			needs_attention_count: summary.needs_attention_count,
			completed_count: summary.completed_count,
			stale_count: summary.stale_count,
			superseded_count: summary.superseded_count,
			dispatchable_count: summary.dispatchable_count,
			mapped_issue_identifiers: summary.mapped_issue_identifiers,
			node_readbacks: evaluation
				.nodes()
				.iter()
				.filter(|node| operator_execution_program_node_should_render(node))
				.map(operator_execution_program_node_readback)
				.collect(),
			readback_warning: None,
		}
	}

	pub(crate) fn missing_contract(record: &ExecutionProgramRecord) -> Self {
		let node_count = record.program().nodes().len();

		Self {
			program_id: record.program_id().to_owned(),
			status: String::from("stale"),
			source_contract_id: record.source_contract_id().map(str::to_owned),
			intake_kind: record
				.program()
				.program_intake_plan()
				.map(|plan| plan.intake_kind().as_str().to_owned()),
			public_summary: record
				.program()
				.program_intake_plan()
				.map(|plan| plan.public_summary().to_owned()),
			node_count,
			planned_count: 0,
			mapped_count: 0,
			ready_count: 0,
			queued_count: 0,
			blocked_count: 0,
			held_count: 0,
			active_count: 0,
			needs_attention_count: 0,
			completed_count: 0,
			stale_count: node_count,
			superseded_count: 0,
			dispatchable_count: 0,
			mapped_issue_identifiers: operator_execution_program_mapped_issue_identifiers(record),
			node_readbacks: operator_execution_program_missing_contract_nodes(record),
			readback_warning: Some(String::from("source_decision_contract_missing")),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct OperatorExecutionProgramNodeStatus {
	#[serde(default = "operator_execution_program_unknown_status")]
	pub(crate) program_stage: String,
	pub(crate) lifecycle_state: String,
	pub(crate) readiness_state: String,
	pub(crate) issue_identifier: Option<String>,
	pub(crate) issue_state: Option<String>,
	pub(crate) dispatch_action: Option<String>,
	pub(crate) reason_codes: Vec<String>,
	pub(crate) reasons: Vec<String>,
	pub(crate) next_action: String,
}

pub(crate) fn operator_execution_program_status(
	summary: &ExecutionProgramOperatorSummary,
	node_count: usize,
	readback_warning: Option<&str>,
) -> String {
	if readback_warning.is_some() || summary.stale_count > 0 || summary.superseded_count > 0 {
		String::from("stale")
	} else if summary.needs_attention_count > 0 {
		String::from("attention")
	} else if summary.blocked_count > 0 {
		String::from("blocked")
	} else if summary.active_count > 0 {
		String::from("active")
	} else if summary.queued_count > 0 {
		String::from("queued")
	} else if summary.ready_count > 0 {
		String::from("ready")
	} else if node_count > 0 && summary.completed_count == node_count {
		String::from("completed")
	} else if summary.held_count > 0 {
		String::from("held")
	} else {
		String::from("idle")
	}
}

pub(crate) fn operator_execution_program_unknown_status() -> String {
	String::from("unknown")
}

pub(crate) fn operator_execution_program_node_should_render(
	node: &ExecutionNodeEvaluation,
) -> bool {
	node.dispatch_action().is_some()
		|| matches!(
			node.lifecycle_state(),
			crate::execution_program::ExecutionProgramNodeLifecycleState::Active
				| crate::execution_program::ExecutionProgramNodeLifecycleState::Blocked
				| crate::execution_program::ExecutionProgramNodeLifecycleState::Mapped
				| crate::execution_program::ExecutionProgramNodeLifecycleState::NeedsAttention
				| crate::execution_program::ExecutionProgramNodeLifecycleState::PostReview
				| crate::execution_program::ExecutionProgramNodeLifecycleState::Planned
				| crate::execution_program::ExecutionProgramNodeLifecycleState::Stale
				| crate::execution_program::ExecutionProgramNodeLifecycleState::Superseded
		)
}

pub(crate) fn operator_execution_program_node_readback(
	node: &ExecutionNodeEvaluation,
) -> OperatorExecutionProgramNodeStatus {
	let reason_codes = operator_execution_program_reason_codes(node.reasons());
	let reasons = node
		.reasons()
		.iter()
		.map(|reason| operator_execution_program_public_reason(reason))
		.collect::<Vec<_>>();
	let issue = node.linear_issue();

	OperatorExecutionProgramNodeStatus {
		program_stage: node.stage().as_str().to_owned(),
		lifecycle_state: node.lifecycle_state().as_str().to_owned(),
		readiness_state: node.state().as_str().to_owned(),
		issue_identifier: issue.map(|issue| issue.issue_identifier().to_owned()),
		issue_state: issue.map(|issue| issue.issue_state().to_owned()),
		dispatch_action: node.dispatch_action().map(|action| action.as_str().to_owned()),
		next_action: operator_execution_program_node_next_action(node, &reason_codes),
		reason_codes,
		reasons,
	}
}

pub(crate) fn operator_execution_program_missing_contract_nodes(
	record: &ExecutionProgramRecord,
) -> Vec<OperatorExecutionProgramNodeStatus> {
	record
		.program()
		.nodes()
		.iter()
		.map(|node| {
			let issue = node.linear_issue();

			OperatorExecutionProgramNodeStatus {
				program_stage: node.stage().as_str().to_owned(),
				lifecycle_state: String::from("stale"),
				readiness_state: String::from("stale"),
				issue_identifier: issue.map(|issue| issue.issue_identifier().to_owned()),
				issue_state: issue.map(|issue| issue.issue_state().to_owned()),
				dispatch_action: None,
				reason_codes: vec![String::from("source_decision_contract_missing")],
				reasons: vec![String::from("source Decision Contract is missing")],
				next_action: String::from(
					"Restore or supersede the source Decision Contract before dispatching this program.",
				),
			}
		})
		.collect()
}

pub(crate) fn operator_execution_program_mapped_issue_identifiers(
	record: &ExecutionProgramRecord,
) -> Vec<String> {
	let mut identifiers = record
		.program()
		.nodes()
		.iter()
		.filter_map(|node| node.linear_issue().map(|issue| issue.issue_identifier().to_owned()))
		.collect::<Vec<_>>();

	identifiers.sort();
	identifiers.dedup();

	identifiers
}

pub(crate) fn operator_execution_program_reason_codes(reasons: &[String]) -> Vec<String> {
	let mut seen = BTreeSet::new();

	for reason in reasons {
		seen.insert(operator_execution_program_reason_code(reason).to_owned());
	}

	seen.into_iter().collect()
}

pub(crate) fn operator_execution_program_reason_code(reason: &str) -> &'static str {
	if reason == "node no longer matches the accepted Decision Contract" {
		"accepted_contract_mismatch"
	} else if reason == "node dispatch intent is not-ready" {
		"dispatch_intent_not_ready"
	} else if reason == "node dispatch intent is paused" {
		"dispatch_intent_paused"
	} else if reason == "node already has a current lane" {
		"current_lane_present"
	} else if reason == "node dispatch intent is terminal" {
		"dispatch_intent_terminal"
	} else if reason == "node is ready for normal Linear issue execution" {
		"ready_for_linear_execution"
	} else if reason == "node has no acceptance expectations" {
		"acceptance_expectations_missing"
	} else if reason == "node has no validation expectations" {
		"validation_expectations_missing"
	} else if reason.starts_with("dependency `") {
		"dependency_not_terminal"
	} else if reason.starts_with("conflict domain `") {
		"conflict_domain_occupied"
	} else if reason == "node has no normal Linear issue mapping" {
		"linear_issue_mapping_missing"
	} else if reason.contains(" is already terminal in `") {
		"mapped_issue_terminal"
	} else if reason.contains(" is not in a startable state") {
		"mapped_issue_not_startable"
	} else if reason.contains(" already carries `") {
		"mapped_issue_active_label_present"
	} else if reason.contains(" is owned by the retained post-review lifecycle") {
		"mapped_issue_post_review_owner"
	} else if reason.contains(" carries `decodex:manual-only`") {
		"mapped_issue_manual_only"
	} else if reason.contains(" carries `decodex:needs-attention`") {
		"mapped_issue_needs_attention"
	} else if reason.contains(" has open tracker dependency blockers") {
		"mapped_issue_open_blockers"
	} else if reason.contains(" is missing a generic dispatch briefing") {
		"mapped_issue_dispatch_briefing_missing"
	} else {
		"program_readiness_blocked"
	}
}

pub(crate) fn operator_execution_program_public_reason(reason: &str) -> String {
	if reason.starts_with("conflict domain `") {
		String::from("another active or retained program node occupies this conflict domain")
	} else if reason.contains(" is owned by the retained post-review lifecycle") {
		String::from(
			"Review & Landing owns this issue until post-review landing or closeout finishes",
		)
	} else if reason.starts_with("dependency `") {
		String::from("a dependency has not reached a required terminal state")
	} else {
		reason.to_owned()
	}
}

pub(crate) fn operator_execution_program_node_next_action(
	node: &ExecutionNodeEvaluation,
	reason_codes: &[String],
) -> String {
	if matches!(
		node.lifecycle_state(),
		crate::execution_program::ExecutionProgramNodeLifecycleState::Stale
			| crate::execution_program::ExecutionProgramNodeLifecycleState::Superseded
	) {
		return String::from(
			"Refresh or supersede the accepted Decision Contract before dispatching this program.",
		);
	}
	if reason_codes.iter().any(|code| code == "dependency_not_terminal") {
		return String::from(
			"Complete the dependency issue or refresh the Execution Program dependency plan if this remains stale.",
		);
	}
	if matches!(
		node.lifecycle_state(),
		crate::execution_program::ExecutionProgramNodeLifecycleState::NeedsAttention
	) || reason_codes.iter().any(|code| code == "mapped_issue_needs_attention")
	{
		return String::from(
			"Resolve the mapped issue's needs-attention stop before dispatching this node.",
		);
	}
	if matches!(
		node.lifecycle_state(),
		crate::execution_program::ExecutionProgramNodeLifecycleState::Active
	) {
		return String::from(
			"Wait for the current lane or recover its retained state before dispatching this node.",
		);
	}
	if matches!(
		node.lifecycle_state(),
		crate::execution_program::ExecutionProgramNodeLifecycleState::PostReview
	) || reason_codes.iter().any(|code| code == "mapped_issue_post_review_owner")
	{
		return String::from(
			"Continue the retained post-review lifecycle before dispatching this program node.",
		);
	}
	if node.dispatch_action().is_some() {
		return String::from("The program scheduler can dispatch this node directly.");
	}
	if matches!(
		node.lifecycle_state(),
		crate::execution_program::ExecutionProgramNodeLifecycleState::Planned
			| crate::execution_program::ExecutionProgramNodeLifecycleState::Mapped
	) {
		return String::from("Map, promote, or unpause this intake node before dispatching it.");
	}
	if matches!(
		node.lifecycle_state(),
		crate::execution_program::ExecutionProgramNodeLifecycleState::Blocked
	) {
		return String::from(
			"Repair mapped issue blockers, briefing, or program readiness before retrying.",
		);
	}

	String::from("No operator action required.")
}
