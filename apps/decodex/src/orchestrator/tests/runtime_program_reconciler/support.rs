use std::{collections::BTreeSet, path::Path};

use rusqlite::Connection;

use crate::{
	execution_program::{
		ExecutionLinearIssueMapping, ExecutionProgram, ExecutionProgramNode,
		ExecutionProgramNodeStage, ExecutionQueueIntent,
	},
	loop_contract::{DecisionContract, DecisionPromotion, DecisionPromotionActorKind},
	orchestrator::tests,
	tracker::{self, TrackerIssue, TrackerLabel},
};

pub(in crate::orchestrator::tests::runtime_program_reconciler) fn program_reconciler_program(
	nodes: Vec<ExecutionProgramNode>,
) -> ExecutionProgram {
	ExecutionProgram::from_issue_batch_intake(
		"program-reconciler",
		"pubfi",
		"program-reconciler-fingerprint",
		"Program reconciler test intake.",
		nodes,
	)
	.expect("program should build")
}

pub(in crate::orchestrator::tests::runtime_program_reconciler) fn program_reconciler_node(
	node_id: &str,
	issue: &TrackerIssue,
	queue_intent: ExecutionQueueIntent,
) -> ExecutionProgramNode {
	program_reconciler_node_with_mapping(node_id, issue, queue_intent)
}

pub(in crate::orchestrator::tests::runtime_program_reconciler) fn program_reconciler_node_with_mapping(
	node_id: &str,
	issue: &TrackerIssue,
	queue_intent: ExecutionQueueIntent,
) -> ExecutionProgramNode {
	let mapping = program_reconciler_mapping(issue);

	ExecutionProgramNode::new(
		node_id,
		ExecutionProgramNodeStage::Runtime,
		format!("Resolve {}.", issue.identifier),
		queue_intent,
	)
	.expect("node should build")
	.with_acceptance_expectations([format!("{} is executable.", issue.identifier)])
	.expect("acceptance should attach")
	.with_validation_expectations([String::from("Run focused validation.")])
	.expect("validation should attach")
	.with_linear_issue(mapping)
	.expect("issue mapping should attach")
}

pub(in crate::orchestrator::tests::runtime_program_reconciler) fn program_reconciler_mapping(
	issue: &TrackerIssue,
) -> ExecutionLinearIssueMapping {
	ExecutionLinearIssueMapping::new(&issue.id, &issue.identifier, &issue.state.name)
		.expect("mapping should build")
		.with_active_label(issue.has_label(&tracker::automation_active_label("pubfi")))
		.with_opt_out_label(issue.has_label("decodex:manual-only"))
		.with_needs_attention_label(issue.has_label("decodex:needs-attention"))
		.with_open_tracker_blockers(!issue.blockers.is_empty())
}

pub(in crate::orchestrator::tests::runtime_program_reconciler) fn program_reconciler_issue(
	id: &str,
	identifier: &str,
	state_name: &str,
	labels: &[&str],
) -> TrackerIssue {
	let mut issue = tests::sample_issue_with_project_slug_and_sort_fields(
		id,
		identifier,
		"pubfi",
		state_name,
		&[],
		Some(3),
		"2026-06-12T00:00:00.000Z",
	);

	issue.labels = labels
		.iter()
		.copied()
		.collect::<BTreeSet<_>>()
		.into_iter()
		.enumerate()
		.map(|(index, label)| TrackerLabel {
			id: format!("label-current-{index}"),
			name: label.to_owned(),
		})
		.collect();

	issue
}

pub(in crate::orchestrator::tests::runtime_program_reconciler) fn program_reconciler_queue_label()
-> String {
	tracker::automation_queue_label("pubfi")
}

pub(in crate::orchestrator::tests::runtime_program_reconciler) fn program_reconciler_accepted_contract()
-> DecisionContract {
	let mut contract: DecisionContract = serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/decision_contract/research_x_latent_contract.json"
	)))
	.expect("decision contract fixture should deserialize");

	contract
		.promote(
			DecisionPromotion::new(
				"operator",
				DecisionPromotionActorKind::User,
				"2026-06-17T00:00:00Z",
				"program-reconciler-test",
				Some(String::from("Accepted for program reconciler regression coverage.")),
			)
			.expect("promotion should build"),
		)
		.expect("contract should promote");

	contract
}

pub(in crate::orchestrator::tests::runtime_program_reconciler) fn insert_removed_flat_decision_contract(
	state_path: &Path,
	service_id: &str,
	source_issue_id: Option<&str>,
	contract: &DecisionContract,
) {
	let mut removed_field_payload =
		serde_json::to_value(contract).expect("contract should encode as JSON");
	let readiness = removed_field_payload
		.get_mut("execution_readiness")
		.expect("readiness should exist")
		.as_object_mut()
		.expect("readiness should be an object");

	readiness.remove("proposed_issues");
	readiness.insert(
		String::from("proposed_issue_summaries"),
		serde_json::json!(["Flat summary that must be migrated before dispatch."]),
	);

	let connection = Connection::open(state_path).expect("sqlite should open");

	connection
		.execute(
			"INSERT INTO decision_contracts (
			project_id, contract_id, source_issue_id, status, payload_json, created_at,
			created_at_unix, updated_at, updated_at_unix
		) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			rusqlite::params![
				service_id,
				contract.contract_id(),
				source_issue_id,
				contract.status().as_str(),
				serde_json::to_string(&removed_field_payload)
					.expect("removed-field payload should serialize"),
				"2026-06-17T00:00:00Z",
				1_i64,
				"2026-06-17T00:00:00Z",
				1_i64,
			],
		)
		.expect("removed-field decision contract row should insert");
}
