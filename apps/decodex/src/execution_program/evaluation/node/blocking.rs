use std::collections::{BTreeMap, HashSet};

use crate::execution_program::{
	model::{
		ExecutionConflictDomain, ExecutionLinearIssueMapping, ExecutionProgramDependency,
		ExecutionProgramNode,
	},
	policy::{ExecutionDependencySnapshot, ExecutionWorkflowPolicy},
};

pub(super) fn collect_blocking_readiness_reasons(
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
