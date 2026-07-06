mod readback;
mod reasons;

use serde::{Deserialize, Serialize};

use crate::{
	execution_program::{ExecutionProgramEvaluation, ExecutionProgramOperatorSummary},
	state::ExecutionProgramRecord,
};

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
				.filter(|program_node| readback::node_should_render(program_node))
				.map(readback::node_status)
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
			mapped_issue_identifiers: readback::mapped_issue_identifiers(record),
			node_readbacks: readback::missing_contract_nodes(record),
			readback_warning: Some(String::from("source_decision_contract_missing")),
		}
	}
}

fn operator_execution_program_status(
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

fn operator_execution_program_unknown_status() -> String {
	String::from("unknown")
}
