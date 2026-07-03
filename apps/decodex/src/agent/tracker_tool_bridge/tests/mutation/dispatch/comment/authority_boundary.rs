use crate::{
	agent::tracker_tool_bridge::{
		DynamicToolHandler, ISSUE_COMMENT_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME, TrackerToolBridge,
		tests::{self, TEST_SERVICE_ID},
	},
	orchestrator::{
		self, AuthorityBoundaryCheckInput, AuthorityBoundaryDisposition,
		AuthorityBoundaryPolicyDecision,
	},
	state::StateStore,
	tracker::records,
};

#[test]
fn accepts_authority_boundary_decision_request_without_public_private_evidence_leakage() {
	let issue = tests::sample_issue();
	let tracker = tests::tracker_with_current_issue_snapshot(&issue);
	let workflow = tests::sample_workflow();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let private_diff_evidence = "private diff path /Users/example/decodex/.worktrees/DEC-1";
	let review_context = tests::sample_review_context();
	let boundary_event = orchestrator::record_authority_boundary_check_private_event(
		&state_store,
		AuthorityBoundaryCheckInput {
			project_id: &review_context.service_id,
			issue_id: &issue.id,
			issue_identifier: &issue.identifier,
			run_id: &review_context.run_id,
			attempt_number: review_context.attempt_number,
			decision_contract_ids: vec!["contract-dec-1"],
			attempted_recovery_reason: "uncovered_direction",
			changed_surfaces: vec![crate::orchestrator::AuthorityBoundaryChangedSurface {
				surface: crate::orchestrator::AuthorityBoundarySurface::Objective,
				change_summary: "Public CLI behavior would change.",
				policy_decision:
					crate::orchestrator::AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
				legacy_disposition:
					crate::orchestrator::AuthorityBoundaryDisposition::RequiresHuman,
			}],
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			final_disposition_reason: "Accepted behavior needs explicit authority.",
			improvement_signals: Vec::new(),
		},
	)
	.expect("authority boundary check should persist");
	let bridge = TrackerToolBridge::with_run_context_and_state_store(
		&tracker,
		&issue,
		&workflow,
		review_context,
		&state_store,
	);
	let label_response = DynamicToolHandler::handle_call(
		&bridge,
		ISSUE_LABEL_ADD_TOOL_NAME,
		serde_json::json!({ "label": "decodex:needs-attention" }),
	);
	let mut args = tests::manual_attention_comment_args();

	args["error_class"] = serde_json::json!("contract_boundary_required");
	args["next_action"] = serde_json::json!(
		"accept, reject, or revise decision request `dr-dec-1-2`, then clear needs-attention and requeue through Decodex"
	);
	args["decision_request"] = serde_json::json!({
		"boundary_check_id": boundary_event.record_id(),
		"decision_request_id": "dr-dec-1-2",
		"reason_code": "contract_boundary_required",
		"boundary_type": "accepted_behavior",
		"proposed_change": "Change the public CLI contract for retained lane recovery.",
		"why_exceeds_authority": "The accepted issue did not authorize changing public CLI behavior.",
		"options": [
			{
				"label": "accept",
				"description": "Authorize the CLI contract change and update the Decision Contract."
			},
			{
				"label": "reject",
				"description": "Keep the current contract and stop this recovery path."
			}
		],
		"recommendation": "Revise the Decision Contract before resuming automation.",
		"resume_condition": "Automation may resume after the issue or Decision Contract explicitly authorizes the boundary change.",
		"retained_worktree_evidence": ["retained worktree has tracked changes"],
		"retained_diff_evidence": [private_diff_evidence],
		"recovery_attempt_context": ["authority boundary check required human direction"]
	});

	let comment_response = DynamicToolHandler::handle_call(&bridge, ISSUE_COMMENT_TOOL_NAME, args);

	assert!(label_response.success);
	assert!(comment_response.success);

	let comments = tracker.comments.borrow();
	let comment = comments.first().expect("decision request summary should write");
	let record = records::parse_linear_execution_event_record(comment)
		.expect("decision request summary should include a ledger record");
	let events = state_store
		.list_private_execution_events(TEST_SERVICE_ID, &issue.id, "pub-618-attempt-2-123", 2)
		.expect("private decision event should list");
	let decision_event = events
		.iter()
		.find(|event| event.event_type() == "authority_decision_request")
		.expect("private decision request should persist");

	assert_eq!(record.event_type, "needs_attention");
	assert_eq!(record.error_class.as_deref(), Some("contract_boundary_required"));
	assert!(comment.contains("- decision_request_id: `dr-dec-1-2`"));
	assert!(comment.contains("- boundary: `accepted_behavior`"));
	assert!(comment.contains("- recommendation: Revise the Decision Contract"));
	assert!(!comment.contains(private_diff_evidence));
	assert_eq!(decision_event.payload()["decision_request_id"], serde_json::json!("dr-dec-1-2"));
	assert_eq!(
		decision_event.payload()["retained_diff_evidence"][0],
		serde_json::json!(private_diff_evidence)
	);
}
