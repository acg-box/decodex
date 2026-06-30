//! Readiness evaluation and operator summaries for execution programs.

use std::collections::{BTreeMap, HashSet};

use super::{
	model::{
		ExecutionConflictDomain, ExecutionDispatchAction, ExecutionLinearIssueMapping,
		ExecutionProgram, ExecutionProgramDependency, ExecutionProgramNode,
		ExecutionProgramNodeLifecycleState, ExecutionProgramNodeStage, ExecutionQueueIntent,
		ExecutionReadinessState,
	},
	policy::{ExecutionDependencySnapshot, ExecutionWorkflowPolicy},
};
use crate::{
	loop_contract::{DecisionContract, DecisionContractStatus},
	prelude::Result,
};

/// Readiness result for one program node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionNodeEvaluation {
	node_id: String,
	stage: ExecutionProgramNodeStage,
	state: ExecutionReadinessState,
	lifecycle_state: ExecutionProgramNodeLifecycleState,
	reasons: Vec<String>,
	dispatch_action: Option<ExecutionDispatchAction>,
	linear_issue: Option<ExecutionLinearIssueMapping>,
}
impl ExecutionNodeEvaluation {
	/// Node id.
	pub(crate) fn node_id(&self) -> &str {
		&self.node_id
	}

	/// Program node stage.
	pub(crate) fn stage(&self) -> ExecutionProgramNodeStage {
		self.stage
	}

	/// Normalized readiness state.
	pub(crate) fn state(&self) -> ExecutionReadinessState {
		self.state
	}

	/// Durable lifecycle state used for operator program-intake readback.
	pub(crate) fn lifecycle_state(&self) -> ExecutionProgramNodeLifecycleState {
		self.lifecycle_state
	}

	/// Human-readable readiness reasons.
	pub(crate) fn reasons(&self) -> &[String] {
		&self.reasons
	}

	/// Direct dispatch action, if any.
	pub(crate) fn dispatch_action(&self) -> Option<ExecutionDispatchAction> {
		self.dispatch_action
	}

	/// Whether this node can be dispatched directly by the program scheduler.
	pub(crate) fn dispatchable(&self) -> bool {
		matches!(self.dispatch_action, Some(ExecutionDispatchAction::Dispatch))
	}

	/// Mapped Linear issue, when present.
	pub(crate) fn linear_issue(&self) -> Option<&ExecutionLinearIssueMapping> {
		self.linear_issue.as_ref()
	}
}

/// Full readiness result for one Execution Program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExecutionProgramEvaluation {
	pub(super) program_id: String,
	pub(super) nodes: Vec<ExecutionNodeEvaluation>,
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
			.filter(|node| node.state == ExecutionReadinessState::Ready)
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

pub(super) struct EvaluateNodeInput<'a> {
	pub(super) program: &'a ExecutionProgram,
	pub(super) node: &'a ExecutionProgramNode,
	pub(super) current_contract: Option<&'a DecisionContract>,
	pub(super) current_fingerprint: &'a str,
	pub(super) policy: &'a ExecutionWorkflowPolicy,
	pub(super) node_lookup: &'a BTreeMap<&'a str, &'a ExecutionProgramNode>,
	pub(super) dependency_lookup: &'a BTreeMap<&'a str, &'a ExecutionDependencySnapshot>,
	pub(super) occupied_conflicts: &'a HashSet<&'a ExecutionConflictDomain>,
	pub(super) active_issue_ids: &'a HashSet<&'a str>,
}

pub(super) fn evaluate_node(input: EvaluateNodeInput<'_>) -> Result<ExecutionNodeEvaluation> {
	let EvaluateNodeInput {
		program,
		node,
		current_contract,
		current_fingerprint,
		policy,
		node_lookup,
		dependency_lookup,
		occupied_conflicts,
		active_issue_ids,
	} = input;
	let authority_matches =
		current_contract.map_or(program.source_contract_id.is_none(), |contract| {
			contract.status() == DecisionContractStatus::AcceptedPromoted
				&& Some(contract.contract_id()) == program.source_contract_id.as_deref()
		});
	let mut reasons = Vec::new();
	let mut state = ExecutionReadinessState::Ready;
	let mut lifecycle_state = None;

	if !authority_matches
		|| current_fingerprint != program.accepted_contract_fingerprint
		|| current_fingerprint != node.contract_fingerprint
	{
		state = ExecutionReadinessState::Stale;
		lifecycle_state = Some(
			if current_contract.is_some_and(|contract| {
				contract.status() == DecisionContractStatus::RejectedSuperseded
			}) {
				ExecutionProgramNodeLifecycleState::Superseded
			} else {
				ExecutionProgramNodeLifecycleState::Stale
			},
		);

		reasons.push(String::from("node no longer matches the accepted Decision Contract"));
	} else if let Some(issue) = node.linear_issue()
		&& policy.issue_is_terminal(issue)
	{
		state = ExecutionReadinessState::Completed;
		lifecycle_state = Some(ExecutionProgramNodeLifecycleState::Completed);

		reasons.push(format!(
			"mapped issue `{}` is already terminal in `{}`",
			issue.issue_identifier(),
			issue.issue_state()
		));
	} else if let Some(issue) = node.linear_issue()
		&& active_issue_ids.contains(issue.issue_id())
		&& !issue.has_opt_out_label()
		&& !issue.has_needs_attention_label()
		&& !issue.has_post_review_lifecycle()
	{
		state = ExecutionReadinessState::Active;
		lifecycle_state = Some(ExecutionProgramNodeLifecycleState::Active);

		reasons.push(String::from("node already has a current lane"));
	} else {
		match node.queue_intent {
			ExecutionQueueIntent::NotReady => {
				state = ExecutionReadinessState::NotReady;

				reasons.push(String::from("node dispatch intent is not-ready"));
			},
			ExecutionQueueIntent::Paused => {
				state = ExecutionReadinessState::Paused;

				reasons.push(String::from("node dispatch intent is paused"));
			},
			ExecutionQueueIntent::Active => {
				state = ExecutionReadinessState::Active;

				reasons.push(String::from("node already has a current lane"));
			},
			ExecutionQueueIntent::Done | ExecutionQueueIntent::Canceled => {
				state = ExecutionReadinessState::Completed;

				reasons.push(String::from("node dispatch intent is terminal"));
			},
			ExecutionQueueIntent::ReadyToQueue | ExecutionQueueIntent::Queued => {
				collect_blocking_readiness_reasons(
					node,
					policy,
					node_lookup,
					dependency_lookup,
					occupied_conflicts,
					&mut reasons,
				);

				if !reasons.is_empty() {
					state = ExecutionReadinessState::Blocked;
				}
			},
		}
	}
	if state == ExecutionReadinessState::Ready {
		reasons.push(String::from("node is ready for normal Linear issue execution"));
	}

	let dispatch_action = dispatch_action_for(node, state, policy);
	let lifecycle_state = lifecycle_state.unwrap_or_else(|| lifecycle_state_for(node, state));

	Ok(ExecutionNodeEvaluation {
		node_id: node.node_id.clone(),
		stage: node.stage,
		state,
		lifecycle_state,
		reasons,
		dispatch_action,
		linear_issue: node.linear_issue.clone(),
	})
}

fn collect_blocking_readiness_reasons(
	node: &ExecutionProgramNode,
	policy: &ExecutionWorkflowPolicy,
	node_lookup: &BTreeMap<&str, &ExecutionProgramNode>,
	dependency_lookup: &BTreeMap<&str, &ExecutionDependencySnapshot>,
	occupied_conflicts: &HashSet<&ExecutionConflictDomain>,
	reasons: &mut Vec<String>,
) {
	if node.acceptance_expectations.is_empty() {
		reasons.push(String::from("node has no acceptance expectations"));
	}
	if node.validation_expectations.is_empty() {
		reasons.push(String::from("node has no validation expectations"));
	}

	for dependency in &node.dependencies {
		if !dependency_is_satisfied(dependency, policy, node_lookup, dependency_lookup) {
			reasons.push(format!(
				"dependency `{}` has not reached a required terminal state",
				dependency.dependency_id()
			));
		}
	}

	let issue_is_post_review_owned =
		node.linear_issue().is_some_and(|issue| issue.has_post_review_lifecycle());

	if !issue_is_post_review_owned {
		for domain in &node.conflict_domains {
			if occupied_conflicts.contains(domain) {
				reasons.push(format!(
					"conflict domain `{}:{}` is already occupied",
					domain.kind.as_str(),
					domain.key()
				));
			}
		}
	}

	if let Some(issue) = &node.linear_issue {
		collect_issue_mapping_reasons(issue, policy, reasons);
	} else {
		reasons.push(String::from("node has no normal Linear issue mapping"));
	}
}

fn collect_issue_mapping_reasons(
	issue: &ExecutionLinearIssueMapping,
	policy: &ExecutionWorkflowPolicy,
	reasons: &mut Vec<String>,
) {
	if policy.issue_is_terminal(issue) {
		reasons.push(format!(
			"mapped issue `{}` is already terminal in `{}`",
			issue.issue_identifier(),
			issue.issue_state()
		));
	}
	if issue.has_post_review_lifecycle {
		reasons.push(format!(
			"mapped issue `{}` is owned by the retained post-review lifecycle",
			issue.issue_identifier()
		));

		return;
	}
	if !policy.issue_is_startable(issue) {
		reasons.push(format!(
			"mapped issue `{}` is not in a startable state",
			issue.issue_identifier()
		));
	}
	if issue.has_active_label {
		reasons.push(format!(
			"mapped issue `{}` already carries `{}`",
			issue.issue_identifier(),
			policy.active_label
		));
	}
	if issue.has_opt_out_label {
		reasons.push(format!(
			"mapped issue `{}` carries `{}`",
			issue.issue_identifier(),
			policy.opt_out_label
		));
	}
	if issue.has_needs_attention_label {
		reasons.push(format!(
			"mapped issue `{}` carries `{}`",
			issue.issue_identifier(),
			policy.needs_attention_label
		));
	}
	if issue.has_open_tracker_blockers {
		reasons.push(format!(
			"mapped issue `{}` has open tracker dependency blockers",
			issue.issue_identifier()
		));
	}
	if !issue.has_generic_dispatch_briefing {
		reasons.push(format!(
			"mapped issue `{}` is missing a generic dispatch briefing",
			issue.issue_identifier()
		));
	}
}

fn dependency_is_satisfied(
	dependency: &ExecutionProgramDependency,
	policy: &ExecutionWorkflowPolicy,
	node_lookup: &BTreeMap<&str, &ExecutionProgramNode>,
	dependency_lookup: &BTreeMap<&str, &ExecutionDependencySnapshot>,
) -> bool {
	if let Some(snapshot) = dependency_lookup.get(dependency.dependency_id()) {
		if let Some(state) = &snapshot.tracker_state {
			return dependency_terminal_states(dependency, policy)
				.iter()
				.any(|terminal| terminal == state);
		}
		if let Some(queue_intent) = snapshot.queue_intent {
			return queue_intent.is_terminal();
		}
	}

	node_lookup
		.get(dependency.dependency_id())
		.is_some_and(|node| node.queue_intent().is_terminal())
}

fn dependency_terminal_states<'a>(
	dependency: &'a ExecutionProgramDependency,
	policy: &'a ExecutionWorkflowPolicy,
) -> &'a [String] {
	if dependency.required_terminal_states.is_empty() {
		policy.terminal_states()
	} else {
		&dependency.required_terminal_states
	}
}

fn dispatch_action_for(
	node: &ExecutionProgramNode,
	state: ExecutionReadinessState,
	policy: &ExecutionWorkflowPolicy,
) -> Option<ExecutionDispatchAction> {
	let issue = node.linear_issue()?;

	if state != ExecutionReadinessState::Ready {
		return None;
	}
	if !matches!(
		node.queue_intent(),
		ExecutionQueueIntent::ReadyToQueue | ExecutionQueueIntent::Queued
	) || !policy.issue_is_startable(issue)
	{
		return None;
	}

	Some(ExecutionDispatchAction::Dispatch)
}

fn lifecycle_state_for(
	node: &ExecutionProgramNode,
	state: ExecutionReadinessState,
) -> ExecutionProgramNodeLifecycleState {
	if let Some(issue) = node.linear_issue()
		&& issue.has_needs_attention_label
	{
		return ExecutionProgramNodeLifecycleState::NeedsAttention;
	}
	if let Some(issue) = node.linear_issue()
		&& issue.has_post_review_lifecycle
	{
		return ExecutionProgramNodeLifecycleState::PostReview;
	}
	if let Some(issue) = node.linear_issue()
		&& issue.has_active_label
	{
		return ExecutionProgramNodeLifecycleState::Active;
	}

	match state {
		ExecutionReadinessState::NotReady | ExecutionReadinessState::Paused => {
			if node.linear_issue().is_some() {
				ExecutionProgramNodeLifecycleState::Mapped
			} else {
				ExecutionProgramNodeLifecycleState::Planned
			}
		},
		ExecutionReadinessState::Ready => ExecutionProgramNodeLifecycleState::Ready,
		ExecutionReadinessState::Blocked => ExecutionProgramNodeLifecycleState::Blocked,
		ExecutionReadinessState::Active => ExecutionProgramNodeLifecycleState::Active,
		ExecutionReadinessState::Completed => ExecutionProgramNodeLifecycleState::Completed,
		ExecutionReadinessState::Stale => ExecutionProgramNodeLifecycleState::Stale,
	}
}
