use crate::{
	execution_program::{
		ExecutionConflictDomain, ExecutionConflictDomainKind, ExecutionLinearIssueMapping,
		ExecutionProgram, ExecutionProgramDependency, ExecutionProgramNode,
		ExecutionProgramNodeStage, ExecutionQueueIntent,
	},
	loop_contract::{DecisionPromotion, DecisionPromotionActorKind},
	orchestrator::tests::operator::status::{
		self, Connection, DecisionContract, FakeTracker, HashMap,
		OperatorCodexAccountControlStatus, OperatorExecutionProgramNodeStatus,
		OperatorExecutionProgramStatus, OperatorStatusSnapshot, ReviewHandoffMarker, ServiceConfig,
		StateStore, Value, WorkflowDocument, env, orchestrator,
	},
};

#[test]
fn operator_status_text_surfaces_execution_program_summary() {
	let snapshot = OperatorStatusSnapshot {
		project_id: String::from("decodex"),
		run_limit: 10,
		status_source: None,
		snapshot_age_seconds: None,
		warnings: Vec::new(),
		warning_details: Vec::new(),
		connector_backoffs: Vec::new(),
		projects: Vec::new(),
		account_control: OperatorCodexAccountControlStatus {
			mode: String::from("balanced"),
			account_selector: None,
		},
		accounts: Vec::new(),
		current_lanes: Vec::new(),
		queued_candidates: Vec::new(),
		recent_runs: Vec::new(),
		history_lanes: Vec::new(),
		execution_programs: vec![OperatorExecutionProgramStatus {
			program_id: String::from("program-853"),
			status: String::from("blocked"),
			source_contract_id: Some(String::from("contract-852")),
			intake_kind: Some(String::from("goal_intake")),
			public_summary: Some(String::from("Resolve promoted program work.")),
			node_count: 3,
			planned_count: 0,
			mapped_count: 0,
			ready_count: 1,
			queued_count: 0,
			blocked_count: 1,
			held_count: 0,
			active_count: 0,
			needs_attention_count: 0,
			completed_count: 1,
			stale_count: 0,
			superseded_count: 0,
			dispatchable_count: 0,
			mapped_issue_identifiers: vec![String::from("XY-853")],
			node_readbacks: vec![OperatorExecutionProgramNodeStatus {
				program_stage: String::from("runtime"),
				lifecycle_state: String::from("blocked"),
				readiness_state: String::from("blocked"),
				issue_identifier: Some(String::from("XY-853")),
				issue_state: Some(String::from("Todo")),
				dispatch_action: None,
				reason_codes: vec![String::from("dependency_not_terminal")],
				reasons: vec![String::from(
					"a dependency has not reached a required terminal state",
				)],
				next_action: String::from(
					"Complete the dependency issue or refresh the Execution Program dependency plan if this remains stale.",
				),
			}],
			readback_warning: None,
		}],
		worktrees: Vec::new(),
		post_review_lanes: Vec::new(),
	};
	let rendered = orchestrator::render_operator_status(&snapshot);

	assert!(rendered.contains("Execution programs: 1"));
	assert!(rendered.contains("Execution Programs"));
	assert!(rendered.contains(
		"program_id: program-853 status=blocked source_contract_id: contract-852 intake_kind=goal_intake summary=\"Resolve promoted program work.\" nodes=3 planned=0 mapped=0 ready=1 queued=0 blocked=1 held=0 active=0 attention=0 completed=1 stale=0 superseded=0 dispatchable=0 mapped_issues=XY-853"
	));
	assert!(rendered.contains(
		"node: issue=XY-853 issue_state=Todo program_stage=runtime lifecycle=blocked readiness=blocked dispatch_action=none reason_codes=dependency_not_terminal reasons=\"a dependency has not reached a required terminal state\" next_action=\"Complete the dependency issue or refresh the Execution Program dependency plan if this remains stale.\""
	));
}

#[test]
fn operator_status_json_uses_direct_dispatch_program_fields() {
	let snapshot: OperatorStatusSnapshot = serde_json::from_value(serde_json::json!({
		"project_id": "decodex",
		"run_limit": 10,
		"warnings": [],
		"warning_details": [],
		"connector_backoffs": [],
		"projects": [],
		"account_control": {
			"mode": "balanced",
			"account_selector": null,
		},
		"accounts": [],
		"current_lanes": [],
		"recent_runs": [],
		"history_lanes": [],
		"execution_programs": [{
			"program_id": "direct-dispatch-program",
			"source_contract_id": null,
			"node_count": 1,
			"planned_count": 0,
			"mapped_count": 0,
			"ready_count": 1,
			"queued_count": 0,
			"blocked_count": 0,
			"held_count": 0,
			"active_count": 0,
			"needs_attention_count": 0,
			"completed_count": 0,
			"stale_count": 0,
			"superseded_count": 0,
			"dispatchable_count": 1,
			"mapped_issue_identifiers": ["XY-853"],
			"node_readbacks": [{
				"lifecycle_state": "ready",
				"readiness_state": "ready",
				"issue_identifier": "XY-853",
				"issue_state": "Todo",
				"dispatch_action": "dispatch",
				"reason_codes": ["ready_for_linear_execution"],
				"reasons": ["node is ready for normal Linear issue execution"],
				"next_action": "The program scheduler can dispatch this node directly."
			}],
			"readback_warning": null,
		}],
		"queued_candidates": [],
		"worktrees": [],
		"post_review_lanes": [],
	}))
	.expect("operator snapshot should deserialize");
	let program = snapshot.execution_programs.first().expect("program should deserialize");

	assert_eq!(program.status, "unknown");
	assert_eq!(program.intake_kind, None);
	assert_eq!(program.public_summary, None);
	assert_eq!(program.dispatchable_count, 1);
	assert_eq!(program.node_readbacks[0].dispatch_action.as_deref(), Some("dispatch"));
}

#[test]
fn operator_status_snapshot_surfaces_program_intake_and_node_readbacks() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	seed_program_readback_status(&state_store, &config);

	let snapshot = build_program_readback_snapshot(&config, &workflow, &state_store);
	let program = snapshot.execution_programs.first().expect("program should surface");
	let program_json = program_readback_json(&snapshot);

	assert_program_readback_summary(program);
	assert_program_readback_json(&program_json);
	assert_program_node_readbacks(program, &program_json);
}

#[test]
fn operator_status_program_readback_prefers_post_review_owner_over_stale_active_label() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue_id = "issue-post-review";
	let issue_identifier = "PUB-946";
	let branch_name = "x/pubfi-pub-946";
	let conflict = ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, "runtime")
		.expect("conflict domain should build");
	let node =
		status_program_active_node("node-post-review", issue_id, issue_identifier, "In Review")
			.with_conflict_domains([conflict])
			.expect("conflict domain should attach");
	let program = ExecutionProgram::from_issue_batch_intake(
		"program-post-review-owner",
		config.service_id(),
		"program-post-review-owner-fingerprint",
		"Track post-review owner readback.",
		vec![node],
	)
	.expect("program should build");

	state_store
		.upsert_execution_program(config.service_id(), program)
		.expect("program should persist");
	state_store
		.upsert_review_handoff_marker(
			config.service_id(),
			issue_id,
			&ReviewHandoffMarker::new(
				"pub-946-attempt-1",
				1,
				branch_name,
				"https://github.com/hack-ink/pubfi/pull/946",
				"main",
				branch_name,
				"1111111111111111111111111111111111111111",
			),
		)
		.expect("review lifecycle should persist");

	let snapshot = build_program_readback_snapshot(&config, &workflow, &state_store);
	let program = snapshot.execution_programs.first().expect("program should surface");
	let node = program.node_readbacks.first().expect("post-review node should surface");

	assert_eq!(program.status, "active");
	assert_eq!(program.active_count, 1);
	assert_eq!(program.blocked_count, 0);
	assert_eq!(node.lifecycle_state, "post_review");
	assert_eq!(node.readiness_state, "blocked");
	assert!(node.reason_codes.contains(&String::from("mapped_issue_post_review_owner")));
	assert!(!node.reason_codes.contains(&String::from("mapped_issue_active_label_present")));
	assert!(!node.reason_codes.contains(&String::from("conflict_domain_occupied")));
	assert_eq!(
		node.reasons,
		vec![String::from(
			"Review & Landing owns this issue until post-review landing or closeout finishes",
		)]
	);
	assert_eq!(
		node.next_action,
		"Continue the retained post-review lifecycle before dispatching this program node."
	);
}

#[test]
fn operator_status_program_readback_refreshes_live_tracker_issue_mapping() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let stale_mapping = status_program_issue_mapping("issue-live-refresh", "PUB-1597", "Todo")
		.with_needs_attention_label(true);
	let node = ExecutionProgramNode::new(
		"node-live-refresh",
		ExecutionProgramNodeStage::Runtime,
		"Close stale Program attention after the mapped issue is terminal.",
		ExecutionQueueIntent::ReadyToQueue,
	)
	.expect("node should build")
	.with_acceptance_expectations(["The mapped issue reflects live tracker state."])
	.expect("acceptance should attach")
	.with_validation_expectations(["Build operator status."])
	.expect("validation should attach")
	.with_linear_issue(stale_mapping)
	.expect("stale mapping should attach");
	let program = ExecutionProgram::from_issue_batch_intake(
		"program-live-refresh",
		config.service_id(),
		"program-live-refresh-fingerprint",
		"Refresh live Program issue metadata.",
		vec![node],
	)
	.expect("program should build");

	state_store
		.upsert_execution_program(config.service_id(), program)
		.expect("program should persist");

	let live_issue = status::sample_issue_with_sort_fields(
		"issue-live-refresh",
		"PUB-1597",
		"Done",
		&[],
		Some(1),
		"2026-06-19T00:00:00.000Z",
	);
	let tracker = FakeTracker::new(vec![live_issue]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");
	let program = snapshot.execution_programs.first().expect("program should surface");

	assert_eq!(program.status, "completed");
	assert_eq!(program.completed_count, 1);
	assert_eq!(program.needs_attention_count, 0);
	assert_eq!(program.blocked_count, 0);
	assert_eq!(program.dispatchable_count, 0);
	assert!(
		program.node_readbacks.is_empty(),
		"terminal refreshed Program nodes should not render stale attention readbacks"
	);
	assert_eq!(tracker.refresh_queries.borrow().len(), 1);
	assert_eq!(tracker.refresh_queries.borrow()[0], vec![String::from("issue-live-refresh")]);
}

fn seed_program_readback_status(state_store: &StateStore, config: &ServiceConfig) {
	let program = ExecutionProgram::from_issue_batch_intake(
		"program-status-readback",
		config.service_id(),
		"program-status-fingerprint",
		"Coordinate status readback work.",
		vec![
			status_program_node(
				"node-ready",
				"issue-ready",
				"PUB-941",
				"Todo",
				ExecutionQueueIntent::ReadyToQueue,
			),
			status_program_node(
				"node-queued",
				"issue-queued",
				"PUB-942",
				"Todo",
				ExecutionQueueIntent::Queued,
			),
			status_program_node_with_dependency(
				"node-blocked",
				"issue-blocked",
				"PUB-943",
				"Todo",
				"PUB-944",
			),
			status_program_node(
				"node-dependency",
				"issue-dependency",
				"PUB-944",
				"Todo",
				ExecutionQueueIntent::NotReady,
			),
			status_program_active_node("node-active", "issue-active", "PUB-945", "In Progress"),
		],
	)
	.expect("program should build");

	state_store
		.upsert_execution_program(config.service_id(), program)
		.expect("program should persist");
}

fn build_program_readback_snapshot(
	config: &ServiceConfig,
	workflow: &WorkflowDocument,
	state_store: &StateStore,
) -> OperatorStatusSnapshot {
	let tracker = FakeTracker::new(Vec::new());

	orchestrator::build_live_operator_status_snapshot(&tracker, config, workflow, state_store, 10)
		.expect("status snapshot should build")
}

fn assert_program_readback_summary(program: &OperatorExecutionProgramStatus) {
	assert_eq!(program.program_id, "program-status-readback");
	assert_eq!(program.status, "blocked");
	assert_eq!(program.intake_kind.as_deref(), Some("issue_batch_intake"));
	assert_eq!(program.public_summary.as_deref(), Some("Coordinate status readback work."));
	assert_eq!(program.ready_count, 2);
	assert_eq!(program.queued_count, 0);
	assert_eq!(program.blocked_count, 1);
	assert_eq!(program.held_count, 2);
	assert_eq!(program.active_count, 1);
	assert_eq!(program.stale_count, 0);
	assert_eq!(program.dispatchable_count, 2);
	assert_eq!(
		program.mapped_issue_identifiers,
		vec![
			String::from("PUB-941"),
			String::from("PUB-942"),
			String::from("PUB-943"),
			String::from("PUB-944"),
			String::from("PUB-945"),
		]
	);
}

fn program_readback_json(snapshot: &OperatorStatusSnapshot) -> Value {
	let snapshot_json = serde_json::to_value(snapshot).expect("snapshot should serialize");

	snapshot_json["execution_programs"]
		.as_array()
		.expect("execution programs should serialize as an array")
		.first()
		.expect("program should serialize")
		.clone()
}

fn assert_program_readback_json(program_json: &Value) {
	assert_eq!(program_json["program_id"], "program-status-readback");
	assert_eq!(program_json["status"], "blocked");
	assert_eq!(program_json["intake_kind"], "issue_batch_intake");
	assert_eq!(program_json["public_summary"], "Coordinate status readback work.");
	assert_eq!(program_json["ready_count"], 2);
	assert_eq!(program_json["queued_count"], 0);
	assert_eq!(program_json["active_count"], 1);
	assert_eq!(program_json["held_count"], 2);
	assert_eq!(program_json["dispatchable_count"], 2);
	assert!(program_json.get("contract").is_none());
	assert!(program_json.get("graph").is_none());
}

fn assert_program_node_readbacks(program: &OperatorExecutionProgramStatus, program_json: &Value) {
	let node_by_issue = program
		.node_readbacks
		.iter()
		.filter_map(|node| node.issue_identifier.as_deref().map(|issue| (issue, node)))
		.collect::<HashMap<_, _>>();
	let ready_node = node_by_issue.get("PUB-941").expect("ready node should render");
	let queued_node = node_by_issue.get("PUB-942").expect("queued node should render");
	let blocked_node = node_by_issue.get("PUB-943").expect("blocked node should render");
	let held_node = node_by_issue.get("PUB-944").expect("held node should render");
	let active_node = node_by_issue.get("PUB-945").expect("active node should render");

	assert_eq!(ready_node.dispatch_action.as_deref(), Some("dispatch"));
	assert_eq!(queued_node.dispatch_action.as_deref(), Some("dispatch"));
	assert_eq!(blocked_node.lifecycle_state, "blocked");
	assert_eq!(blocked_node.dispatch_action.as_deref(), None);
	assert!(blocked_node.reason_codes.contains(&String::from("dependency_not_terminal")));
	assert_eq!(
		blocked_node.reasons,
		vec![String::from("a dependency has not reached a required terminal state")]
	);
	assert!(blocked_node.next_action.contains("Execution Program dependency plan"));
	assert_eq!(held_node.lifecycle_state, "mapped");
	assert!(held_node.reason_codes.contains(&String::from("dispatch_intent_not_ready")));
	assert_eq!(active_node.lifecycle_state, "active");
	assert!(active_node.reason_codes.contains(&String::from("mapped_issue_active_label_present")));

	let node_json = program_json["node_readbacks"]
		.as_array()
		.expect("node readbacks should serialize as an array")
		.iter()
		.find(|node| node["issue_identifier"] == "PUB-945")
		.expect("active node json should serialize");

	assert_eq!(node_json["lifecycle_state"], "active");
	assert_eq!(node_json["readiness_state"], "blocked");
	assert_eq!(node_json["program_stage"], "runtime");
	assert_eq!(node_json["dispatch_action"], serde_json::Value::Null);
}

#[test]
fn operator_status_json_surfaces_missing_contract_program_recovery() {
	let (_temp_dir, config, workflow) = status::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let contract = accepted_status_decision_contract_fixture();
	let program = ExecutionProgram::from_accepted_contract(
		"program-missing-contract",
		config.service_id(),
		&contract,
		vec![status_program_node(
			"node-stale",
			"issue-stale",
			"PUB-946",
			"Todo",
			ExecutionQueueIntent::ReadyToQueue,
		)],
	)
	.expect("program should build");

	state_store
		.upsert_execution_program(config.service_id(), program)
		.expect("program should persist");

	let tracker = FakeTracker::new(Vec::new());
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("status snapshot should build");
	let program = snapshot.execution_programs.first().expect("program should surface");

	assert_eq!(program.program_id, "program-missing-contract");
	assert_eq!(program.status, "stale");
	assert_eq!(program.source_contract_id.as_deref(), Some(contract.contract_id()));
	assert_eq!(program.intake_kind.as_deref(), Some("goal_intake"));
	assert_eq!(program.stale_count, 1);
	assert_eq!(program.readback_warning.as_deref(), Some("source_decision_contract_missing"));
	assert_eq!(program.mapped_issue_identifiers, vec![String::from("PUB-946")]);

	let node = program.node_readbacks.first().expect("stale node should render");

	assert_eq!(node.lifecycle_state, "stale");
	assert_eq!(node.readiness_state, "stale");
	assert_eq!(node.issue_identifier.as_deref(), Some("PUB-946"));
	assert_eq!(node.reason_codes, vec![String::from("source_decision_contract_missing")]);
	assert!(node.next_action.contains("Decision Contract"));

	let snapshot_json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
	let program_json = snapshot_json["execution_programs"]
		.as_array()
		.expect("execution programs should serialize as an array")
		.first()
		.expect("program should serialize");

	assert_eq!(program_json["status"], "stale");
	assert_eq!(program_json["readback_warning"], "source_decision_contract_missing");
	assert_eq!(program_json["node_readbacks"][0]["program_stage"], "runtime");
	assert_eq!(
		program_json["node_readbacks"][0]["reason_codes"][0],
		"source_decision_contract_missing"
	);
	assert_eq!(
		program_json["node_readbacks"][0]["next_action"],
		"Restore or supersede the source Decision Contract before dispatching this program."
	);
	assert!(program_json.get("contract").is_none());
	assert!(program_json.get("decision_contract").is_none());
}

#[test]
fn operator_status_readback_uses_migrated_removed_flat_decision_contract_fields() {
	let (temp_dir, config, workflow) = status::temp_project_layout();
	let state_path = temp_dir.path().join("runtime.sqlite3");
	let state_store = StateStore::open(&state_path).expect("state store should open");
	let contract = accepted_status_decision_contract_fixture();
	let program = ExecutionProgram::from_accepted_contract(
		"program-removed-flat-contract",
		config.service_id(),
		&contract,
		vec![status_program_node(
			"node-removed-flat",
			"issue-removed-flat",
			"PUB-947",
			"Todo",
			ExecutionQueueIntent::ReadyToQueue,
		)],
	)
	.expect("program should build");

	state_store
		.upsert_execution_program(config.service_id(), program)
		.expect("program should persist");

	let mut removed_field_payload =
		serde_json::to_value(&contract).expect("contract should encode as JSON");
	let readiness = removed_field_payload
		.get_mut("execution_readiness")
		.expect("readiness should exist")
		.as_object_mut()
		.expect("readiness should be an object");

	readiness.remove("proposed_issues");
	readiness.insert(
		String::from("proposed_issue_summaries"),
		serde_json::json!(["Flat summary that must be migrated before readback."]),
	);
	readiness.insert(
		String::from("queue_intent"),
		serde_json::json!(["Removed queue intent that must not be re-admitted."]),
	);

	{
		let connection = Connection::open(&state_path).expect("sqlite should open");

		connection
			.execute(
				"INSERT INTO decision_contracts (
						project_id, contract_id, source_issue_id, status, payload_json, created_at,
						created_at_unix, updated_at, updated_at_unix
					) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
				rusqlite::params![
					config.service_id(),
					contract.contract_id(),
					"PUB-947",
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
		connection
			.execute("UPDATE schema_meta SET value = '11' WHERE key = 'schema_version'", [])
			.expect("schema version should mark removed-field state");
	}

	drop(state_store);

	let state_store = StateStore::open(&state_path).expect("removed fields should migrate");
	let migrated_contract = state_store
		.decision_contract(config.service_id(), contract.contract_id())
		.expect("migrated contract read should succeed")
		.expect("migrated contract should exist");

	assert_eq!(
		migrated_contract.contract().execution_readiness().proposed_issues()[0].objective(),
		"Flat summary that must be migrated before readback."
	);

	let snapshot = build_program_readback_snapshot(&config, &workflow, &state_store);
	let program = snapshot.execution_programs.first().expect("program should surface");

	assert_eq!(program.program_id, "program-removed-flat-contract");
	assert_eq!(program.status, "stale");
	assert_ne!(program.readback_warning.as_deref(), Some("source_decision_contract_missing"));
	assert_eq!(program.stale_count, 1);
	assert_eq!(program.mapped_issue_identifiers, vec![String::from("PUB-947")]);
}

fn status_program_node(
	node_id: &str,
	issue_id: &str,
	issue_identifier: &str,
	issue_state: &str,
	queue_intent: ExecutionQueueIntent,
) -> ExecutionProgramNode {
	let mapping = status_program_issue_mapping(issue_id, issue_identifier, issue_state);

	ExecutionProgramNode::new(
		node_id,
		ExecutionProgramNodeStage::Runtime,
		format!("Resolve {issue_identifier}."),
		queue_intent,
	)
	.expect("node should build")
	.with_acceptance_expectations([format!("{issue_identifier} acceptance is explicit.")])
	.expect("acceptance should attach")
	.with_validation_expectations([String::from("Run focused validation.")])
	.expect("validation should attach")
	.with_linear_issue(mapping)
	.expect("mapping should attach")
}

fn status_program_node_with_dependency(
	node_id: &str,
	issue_id: &str,
	issue_identifier: &str,
	issue_state: &str,
	dependency_identifier: &str,
) -> ExecutionProgramNode {
	status_program_node(
		node_id,
		issue_id,
		issue_identifier,
		issue_state,
		ExecutionQueueIntent::ReadyToQueue,
	)
	.with_dependencies([
		ExecutionProgramDependency::new(dependency_identifier).expect("dependency should build")
	])
	.expect("dependency should attach")
}

fn status_program_issue_mapping(
	issue_id: &str,
	issue_identifier: &str,
	issue_state: &str,
) -> ExecutionLinearIssueMapping {
	ExecutionLinearIssueMapping::new(issue_id, issue_identifier, issue_state)
		.expect("mapping should build")
}

fn status_program_active_node(
	node_id: &str,
	issue_id: &str,
	issue_identifier: &str,
	issue_state: &str,
) -> ExecutionProgramNode {
	let mapping = status_program_issue_mapping(issue_id, issue_identifier, issue_state)
		.with_active_label(true);

	ExecutionProgramNode::new(
		node_id,
		ExecutionProgramNodeStage::Runtime,
		format!("Resolve {issue_identifier}."),
		ExecutionQueueIntent::ReadyToQueue,
	)
	.expect("node should build")
	.with_acceptance_expectations([format!("{issue_identifier} acceptance is explicit.")])
	.expect("acceptance should attach")
	.with_validation_expectations([String::from("Run focused validation.")])
	.expect("validation should attach")
	.with_linear_issue(mapping)
	.expect("mapping should attach")
}

fn accepted_status_decision_contract_fixture() -> DecisionContract {
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
				"2026-06-09T10:00:00Z",
				"conversation",
				Some(String::from("User accepted the program boundary.")),
			)
			.expect("promotion should build"),
		)
		.expect("contract should promote");

	contract
}
