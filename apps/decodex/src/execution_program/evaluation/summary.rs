//! Full-program readiness summaries.

use super::{super::model::ExecutionProgramNodeLifecycleState, node::ExecutionNodeEvaluation};

/// Full readiness result for one Execution Program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionProgramEvaluation {
	pub(in crate::execution_program) program_id: String,
	pub(in crate::execution_program) nodes: Vec<ExecutionNodeEvaluation>,
}
impl ExecutionProgramEvaluation {
	/// Node evaluations.
	pub(crate) fn nodes(&self) -> &[ExecutionNodeEvaluation] {
		&self.nodes
	}

	/// Nodes that are internally ready.
	pub(crate) fn ready_node_ids(&self) -> Vec<&str> {
		self.nodes
			.iter()
			.filter(|node| node.state == super::super::model::ExecutionReadinessState::Ready)
			.map(|node| node.node_id.as_str())
			.collect()
	}

	/// Nodes that can be dispatched directly by the program scheduler.
	pub(crate) fn dispatchable_node_ids(&self) -> Vec<&str> {
		self.nodes
			.iter()
			.filter(|node| node.dispatchable())
			.map(|node| node.node_id.as_str())
			.collect()
	}

	/// Operator-facing progress summary without exposing graph operations as workflow.
	pub(crate) fn operator_summary(&self) -> ExecutionProgramOperatorSummary {
		let mut summary = ExecutionProgramOperatorSummary {
			program_id: self.program_id.clone(),
			planned_count: 0,
			mapped_count: 0,
			ready_count: 0,
			queued_count: 0,
			blocked_count: 0,
			held_count: 0,
			active_count: 0,
			needs_attention_count: 0,
			completed_count: 0,
			stale_count: 0,
			superseded_count: 0,
			dispatchable_count: 0,
			mapped_issue_identifiers: Vec::new(),
		};

		for node in &self.nodes {
			match node.lifecycle_state {
				ExecutionProgramNodeLifecycleState::Planned => {
					summary.planned_count += 1;
					summary.held_count += 1;
				},
				ExecutionProgramNodeLifecycleState::Mapped => {
					summary.mapped_count += 1;
					summary.held_count += 1;
				},
				ExecutionProgramNodeLifecycleState::Ready => summary.ready_count += 1,
				ExecutionProgramNodeLifecycleState::Queued => summary.queued_count += 1,
				ExecutionProgramNodeLifecycleState::Blocked => summary.blocked_count += 1,
				ExecutionProgramNodeLifecycleState::Active => {
					summary.active_count += 1;
					summary.held_count += 1;
				},
				ExecutionProgramNodeLifecycleState::PostReview => {
					summary.active_count += 1;
					summary.held_count += 1;
				},
				ExecutionProgramNodeLifecycleState::NeedsAttention => {
					summary.needs_attention_count += 1;
				},
				ExecutionProgramNodeLifecycleState::Completed => summary.completed_count += 1,
				ExecutionProgramNodeLifecycleState::Stale => summary.stale_count += 1,
				ExecutionProgramNodeLifecycleState::Superseded => summary.superseded_count += 1,
			}

			if node.dispatchable() {
				summary.dispatchable_count += 1;
			}

			if let Some(issue) = &node.linear_issue {
				summary.mapped_issue_identifiers.push(issue.issue_identifier.clone());
			}
		}

		summary.mapped_issue_identifiers.sort();
		summary.mapped_issue_identifiers.dedup();

		summary
	}
}

/// Compact operator readback for Execution Program progress.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionProgramOperatorSummary {
	/// Program id.
	pub(crate) program_id: String,
	/// Count of planned nodes without a normal Linear issue mapping.
	pub(crate) planned_count: usize,
	/// Count of mapped nodes that are intentionally held from queueing.
	pub(crate) mapped_count: usize,
	/// Count of ready nodes.
	pub(crate) ready_count: usize,
	/// Count of queued nodes.
	pub(crate) queued_count: usize,
	/// Count of blocked or intentionally not-ready nodes.
	pub(crate) blocked_count: usize,
	/// Count of held nodes that are planned, mapped, or active.
	pub(crate) held_count: usize,
	/// Count of active nodes.
	pub(crate) active_count: usize,
	/// Count of human-attention nodes.
	pub(crate) needs_attention_count: usize,
	/// Count of done or canceled nodes.
	pub(crate) completed_count: usize,
	/// Count of stale nodes.
	pub(crate) stale_count: usize,
	/// Count of superseded nodes.
	pub(crate) superseded_count: usize,
	/// Count of nodes the program scheduler can dispatch directly.
	pub(crate) dispatchable_count: usize,
	/// Normal Linear issue identifiers linked to the program.
	pub(crate) mapped_issue_identifiers: Vec<String>,
}
