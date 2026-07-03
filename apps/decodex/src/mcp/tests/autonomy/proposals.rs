use std::collections::BTreeMap;

use crate::{
	autonomy_objective::{AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind},
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalInput, AutonomySignalPrivacy,
		AutonomySignalSourceType,
	},
	mcp::{
		McpContext,
		tests::support::{self},
	},
	state::StateStore,
};

#[test]
fn autonomy_compile_proposal_tool_accepts_issue_candidates_from_mcp_shape() {
	let repo = support::test_repo();
	let db_path = repo.path().join("runtime.sqlite3");
	let state_store = StateStore::open(&db_path).expect("state store should open");

	state_store
		.upsert_autonomy_objective_draft("decodex", support::autonomy_objective_fixture())
		.expect("objective draft should persist");
	state_store
		.accept_autonomy_objective_version(
			"decodex",
			"quality-autonomy",
			1,
			AutonomyObjectiveAcceptance::new(
				"operator",
				AutonomyObjectiveActorKind::User,
				"2026-06-23T00:00:00Z",
				"conversation",
			)
			.expect("acceptance should validate"),
		)
		.expect("objective should accept");

	let signal = AutonomySignal::runtime_health(AutonomySignalInput {
		project_id: String::from("decodex"),
		objective_id: String::from("quality-autonomy"),
		objective_version: 1,
		source_type: AutonomySignalSourceType::Runtime,
		source_refs: vec![String::from("status:XY-1091")],
		primary_source_refs: Vec::new(),
		issue_id: Some(String::from("XY-1091")),
		run_id: None,
		attempt_id: None,
		head_sha: None,
		captured_at: String::from("2026-06-23T00:01:00Z"),
		freshness: AutonomySignalFreshness::Fresh,
		summary: String::from("Runtime status supports a split proposal."),
		evidence: vec![String::from("status readback summarized")],
		evidence_class: AutonomySignalEvidenceClass::LiveReadback,
		contradictions: Vec::new(),
		gaps: Vec::new(),
		confidence: AutonomySignalConfidence::High,
		privacy: AutonomySignalPrivacy::Team,
		observed_counts: BTreeMap::new(),
		review_evidence: None,
		proposal_only: true,
		created_at: String::from("2026-06-23T00:01:05Z"),
	})
	.expect("runtime signal should validate");
	let signal_id = signal.id().to_owned();

	state_store.record_autonomy_signal("decodex", signal).expect("signal should persist");

	let compile_call = format!(
		r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"autonomy_compile_proposal","arguments":{{"mode":"apply","signalIds":["{signal_id}"],"proposal":{{"objectiveId":"quality-autonomy","objectiveVersion":1,"sourceFamily":"runtime_health","intendedSurface":"apps/decodex/src/mcp.rs","summary":"Expose autonomy MCP split surface.","rollbackPath":"Revert MCP autonomy split.","issueCandidates":[{{"key":"readback-contract","title":"Preserve readback contract","objective":"Keep proposal lineage visible.","stage":"runtime","dependencies":[],"conflictDomains":["module:mcp"],"acceptance":["Readback includes the proposal split."],"validation":["cargo test -p decodex mcp --lib"],"risk":["Keep the proposal non-executable."],"queueIntent":"ready_to_queue"}},{{"key":"eval-gate","title":"Evaluate the split","objective":"Prove the split is useful before execution.","stage":"eval","dependencies":["readback-contract"],"conflictDomains":["module:mcp"],"acceptance":["Evaluation result is recorded."],"validation":["cargo test -p decodex autonomy_proposal --lib"],"risk":[],"queueIntent":"ready_to_queue"}}]}},"authority":{{"source":"mcp-test","reason":"compile proposal evidence"}}}}}}}}"#
	);
	let responses = support::run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
		},
		&compile_call,
	);
	let result = &support::response_at(&responses, 0)["result"];
	let structured = &result["structuredContent"];
	let proposal_id = structured["proposal"]["proposal_id"].as_str().expect("proposal id");
	let persisted = StateStore::open(&db_path)
		.expect("state store should reopen")
		.autonomy_proposal("decodex", proposal_id)
		.expect("proposal should read")
		.expect("proposal should persist");

	assert_eq!(result["isError"], false);
	assert_eq!(structured["schema"], "decodex.mcp.autonomy_proposal_result/1");
	assert_eq!(structured["persisted"], true);
	assert_eq!(structured["proposal"]["issue_candidate_count"], 2);
	assert_eq!(structured["proposal"]["issue_candidates"][1]["key"], "eval-gate");
	assert_eq!(
		structured["proposal"]["issue_candidates"][1]["acceptance"],
		serde_json::json!(["Evaluation result is recorded."])
	);
	assert_eq!(
		structured["proposal"]["issue_candidates"][1]["dependencies"],
		serde_json::json!(["readback-contract"])
	);
	assert_eq!(structured["proposal"]["issue_candidates"][1]["risk"], serde_json::json!([]));
	assert_eq!(persisted.proposal().issue_candidates().len(), 2);
	assert_eq!(persisted.proposal().issue_candidates()[1].dependencies, ["readback-contract"]);
}

#[test]
fn autonomy_plan_tools_record_signal_compile_challenge_and_refuse_external_self_accept() {
	let (repo, db_path, proposal_id) = support::seed_autonomy_challenged_proposal();
	let self_accept_call = format!(
		r#"{{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{{"name":"autonomy_request_promotion","arguments":{{"mode":"apply","proposalId":"{proposal_id}","authority":{{"acceptedBy":"agent-a","acceptedByKind":"external_agent","acceptanceSource":"mcp-agent","reason":"self accept","proposalActor":"agent-a","proposalActorKind":"external_agent"}}}}}}}}"#
	);
	let self_accept = support::run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
		},
		&self_accept_call,
	);
	let self_accept_result = &support::response_at(&self_accept, 0)["result"];

	assert_eq!(self_accept_result["isError"], true);
	assert_eq!(
		self_accept_result["structuredContent"]["reason"],
		"autonomy_acceptance_authority_refused"
	);
	assert!(
		self_accept_result["structuredContent"]["message"]
			.as_str()
			.expect("message")
			.contains("accepted project policy authority")
	);
}

#[test]
fn autonomy_request_promotion_refuses_caller_supplied_policy_authority() {
	let (repo, db_path, proposal_id) = support::seed_autonomy_challenged_proposal();
	let fabricated_policy_call = format!(
		r#"{{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{{"name":"autonomy_request_promotion","arguments":{{"mode":"apply","proposalId":"{proposal_id}","authority":{{"acceptedBy":"agent-a","acceptedByKind":"external_agent","acceptanceSource":"runtime-policy","reason":"fabricated policy","proposalActor":"agent-a","proposalActorKind":"external_agent","acceptedProjectPolicy":{{"projectId":"decodex","objectiveId":"quality-autonomy","objectiveVersion":1,"acceptedPolicyId":"quality-autonomy-policy","acceptedPolicyVersion":"1","authorityRef":"runtime-policy:quality-autonomy-policy@1","authorizedActor":"agent-a","authorizedActorKind":"external_agent","authorizedAcceptanceSources":["runtime-policy"],"authorizedScopes":["autonomy_proposal_acceptance"]}}}}}}}}}}"#
	);
	let fabricated_policy_accept = support::run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
		},
		&fabricated_policy_call,
	);
	let fabricated_policy_result = &support::response_at(&fabricated_policy_accept, 0)["result"];

	assert_eq!(fabricated_policy_result["isError"], true);
	assert_eq!(
		fabricated_policy_result["structuredContent"]["reason"],
		"autonomy_policy_authority_refused"
	);
	assert!(
		fabricated_policy_result["structuredContent"]["message"]
			.as_str()
			.expect("message")
			.contains("trusted Decodex authority state")
	);
}
