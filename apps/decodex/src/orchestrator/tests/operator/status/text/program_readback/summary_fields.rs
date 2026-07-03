use crate::orchestrator::tests::operator::status::{
	OperatorCodexAccountControlStatus, OperatorExecutionProgramNodeStatus,
	OperatorExecutionProgramStatus, OperatorStatusSnapshot, orchestrator,
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
