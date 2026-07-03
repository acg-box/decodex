use std::collections::BTreeMap;

use serde_json::Value;

use crate::{
	autonomy_objective::{AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind},
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalInput, AutonomySignalPrivacy,
		AutonomySignalSourceType,
	},
	mcp::{
		McpCapabilityProfile, McpContext,
		tests::support::{self},
	},
	state::StateStore,
};

#[test]
fn autonomy_resources_expose_summaries_without_private_payloads() {
	let repo = support::test_repo();
	let db_path = repo.path().join("runtime.sqlite3");
	let state_store = StateStore::open(&db_path).expect("state store should open");
	let proposal_id = support::seed_autonomy_mcp_state(&state_store);
	let signal_id = state_store
		.recent_autonomy_signals_for_project("decodex", 1)
		.expect("signals should list")[0]
		.signal_id()
		.to_owned();
	let context = McpContext {
		repo_root: repo.path().to_path_buf(),
		config_path: None,
		project_id: Some(String::from("decodex")),
		state_store: Some(state_store),
	};
	let responses = support::run_stdio_with_context(
			context,
			&[
				r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://projects/decodex/autonomy"}}"#,
				r#"{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{"uri":"decodex://projects/decodex/autonomy/objectives/quality-autonomy/current"}}"#,
				&format!(
					r#"{{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{{"uri":"decodex://projects/decodex/autonomy/signals/{signal_id}"}}}}"#
				),
				&format!(
					r#"{{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{{"uri":"decodex://projects/decodex/autonomy/proposals/{proposal_id}"}}}}"#
				),
				r#"{"jsonrpc":"2.0","id":5,"method":"resources/read","params":{"uri":"decodex://projects/decodex/autonomy/evidence"}}"#,
			]
			.join("\n"),
		);
	let summary = support::resource_response_json(&responses, 0);
	let objective = support::resource_response_json(&responses, 1);
	let signal = support::resource_response_json(&responses, 2);
	let proposal = support::resource_response_json(&responses, 3);
	let evidence = support::resource_response_json(&responses, 4);
	let combined = serde_json::json!({
		"summary": summary,
		"objective": objective,
		"signal": signal,
		"proposal": proposal,
		"evidence": evidence
	});
	let serialized = serde_json::to_string(&combined).expect("resources should serialize");

	assert_eq!(combined["summary"]["schema"], "decodex.mcp.autonomy_summary/1");
	assert_eq!(combined["objective"]["objective"]["state"], "accepted");
	assert_eq!(combined["signal"]["signal"]["kind"], "runtime_health");
	assert_eq!(combined["proposal"]["proposal"]["state"], "decision_candidate");
	assert_eq!(combined["evidence"]["evidence"]["signal_count"], 1);
	assert!(serialized.contains("access_boundary_only"));
	assert!(!serialized.contains("private evidence payload"));
	assert!(!serialized.contains("raw_payload"));

	support::assert_no_sensitive_observability_content(&combined);
}

#[test]
fn autonomy_resources_redact_local_private_signal_refs() {
	let repo = support::test_repo();
	let state_store = StateStore::open_in_memory().expect("state store should open");

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
		source_type: AutonomySignalSourceType::Memory,
		source_refs: vec![
			String::from("memory:private:alpha"),
			String::from("report:private:beta"),
		],
		primary_source_refs: vec![String::from("memory:private:primary")],
		issue_id: Some(String::from("XY-1090")),
		run_id: None,
		attempt_id: None,
		head_sha: None,
		captured_at: String::from("2026-06-23T00:01:00Z"),
		freshness: AutonomySignalFreshness::Fresh,
		summary: String::from("Private memory signal is summarized."),
		evidence: vec![String::from("private evidence summarized")],
		evidence_class: AutonomySignalEvidenceClass::Inference,
		contradictions: Vec::new(),
		gaps: Vec::new(),
		confidence: AutonomySignalConfidence::Medium,
		privacy: AutonomySignalPrivacy::LocalPrivate,
		observed_counts: BTreeMap::new(),
		review_evidence: None,
		proposal_only: true,
		created_at: String::from("2026-06-23T00:01:05Z"),
	})
	.expect("local private signal should validate");
	let signal_id = signal.id().to_owned();

	state_store.record_autonomy_signal("decodex", signal).expect("signal should persist");

	let responses = support::run_stdio_with_context(
			McpContext {
				repo_root: repo.path().to_path_buf(),
				config_path: None,
				project_id: Some(String::from("decodex")),
				state_store: Some(state_store),
			},
			&[
				r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"decodex://projects/decodex/autonomy"}}"#,
				&format!(
					r#"{{"jsonrpc":"2.0","id":2,"method":"resources/read","params":{{"uri":"decodex://projects/decodex/autonomy/signals/{signal_id}"}}}}"#
				),
			]
			.join("\n"),
		);
	let summary = support::resource_response_json(&responses, 0);
	let signal = support::resource_response_json(&responses, 1);
	let combined = serde_json::json!({
		"summary": summary,
		"signal": signal
	});
	let serialized = serde_json::to_string(&combined).expect("resources should serialize");

	for private_ref in ["memory:private:alpha", "report:private:beta", "memory:private:primary"] {
		assert!(!serialized.contains(private_ref), "local-private ref leaked: {private_ref}");
	}

	assert_eq!(combined["signal"]["signal"]["source_refs"], serde_json::json!([]));
	assert_eq!(combined["signal"]["signal"]["source_ref_count"], 2);
	assert_eq!(combined["signal"]["signal"]["primary_source_refs"], serde_json::json!([]));
	assert_eq!(combined["signal"]["signal"]["primary_source_ref_count"], 1);
	assert_eq!(combined["signal"]["signal"]["redaction_level"], "local_private");
	assert_eq!(combined["summary"]["signals"][0]["source_refs"], serde_json::json!([]));
	assert_eq!(combined["summary"]["signals"][0]["source_ref_count"], 2);
}

#[test]
fn autonomy_tools_are_plan_profile_and_apply_requires_authority() {
	let repo = support::test_repo();
	let observe_responses = support::run_stdio_with_profile(
		repo.path(),
		McpCapabilityProfile::Observe,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"autonomy_submit_signal","arguments":{"kind":"runtime_health","signal":{}}}}"#,
	);
	let observe_structured =
		&support::response_at(&observe_responses, 0)["result"]["structuredContent"];

	assert_eq!(observe_structured["reason"], "insufficient_capability_profile");
	assert_eq!(observe_structured["required_capability_profile"], "plan");

	let observe_accept_responses = support::run_stdio_with_profile(
		repo.path(),
		McpCapabilityProfile::Observe,
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"autonomy_accept_objective","arguments":{"objectiveId":"quality-autonomy","objectiveVersion":1}}}"#,
	);
	let observe_accept_structured =
		&support::response_at(&observe_accept_responses, 0)["result"]["structuredContent"];

	assert_eq!(observe_accept_structured["reason"], "insufficient_capability_profile");
	assert_eq!(observe_accept_structured["required_capability_profile"], "plan");

	let state_store = StateStore::open_in_memory().expect("state store should open");
	let context = McpContext {
		repo_root: repo.path().to_path_buf(),
		config_path: None,
		project_id: Some(String::from("decodex")),
		state_store: Some(state_store),
	};
	let responses = support::run_stdio_with_context(
		context,
		r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"autonomy_draft_objective","arguments":{"mode":"apply","objective":{"schema":"decodex.autonomy_objective/1","record_version":1,"project_id":"decodex","id":"quality-autonomy","version":1,"state":"draft","summary":"Improve quality.","goals":["Reduce churn."],"non_goals":["Do not bypass authority."],"metrics":["Validation retry count."],"allowed_surfaces":["apps/decodex/src"],"allowed_signal_kinds":["runtime_health"],"validation_gates":["cargo make check"],"review_policy":"review required","memory_policy":"source-linked only","report_policy":"public-safe only"}}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
	assert_eq!(result["structuredContent"]["reason"], "missing_authority");
	assert_eq!(result["structuredContent"]["tool"], "autonomy_draft_objective");
}

#[test]
fn autonomy_accept_objective_accepts_draft_without_execution_authority() {
	let repo = support::test_repo();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let draft_call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"autonomy_draft_objective","arguments":{"mode":"apply","objective":{"schema":"decodex.autonomy_objective/1","record_version":1,"project_id":"decodex","id":"self-iteration-pilot","version":1,"state":"draft","summary":"Pilot Decodex self-iteration only on the decodex project.","goals":["Reduce repeated operator intervention.","Convert Decodex-only feedback into evidence-backed proposals."],"non_goals":["Do not touch other projects.","Do not bypass review, landing, install, restart, or plugin-sync gates."],"metrics":["Manual-attention count.","Validated proposal replay completeness."],"allowed_surfaces":["apps/decodex/src","automations/decodex","docs","plugins/decodex","plugins/knowledge"],"allowed_signal_kinds":["runtime_health","protocol_drift","execution_friction","docs_skill_drift","validation_regression","user_feedback_cluster"],"validation_gates":["cargo make check-docs","cargo test -p decodex mcp --lib"],"review_policy":"challenge required before promotion","memory_policy":"source-linked evidence only","report_policy":"public-safe source refs with known gaps"},"authority":{"source":"mcp-test","reason":"store draft objective"}}}}"#;
	let accept_missing_authority_call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"autonomy_accept_objective","arguments":{"mode":"apply","objectiveId":"self-iteration-pilot","objectiveVersion":1}}}"#;
	let accept_call = r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"autonomy_accept_objective","arguments":{"mode":"apply","objectiveId":"self-iteration-pilot","objectiveVersion":1,"authority":{"acceptedBy":"operator","acceptedByKind":"user","acceptedAt":"2026-06-27T00:00:00Z","acceptanceSource":"conversation"}}}}"#;
	let read_call = r#"{"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"decodex://projects/decodex/autonomy/objectives/self-iteration-pilot/current"}}"#;
	let responses = support::run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(state_store),
		},
		&format!("{draft_call}\n{accept_missing_authority_call}\n{accept_call}\n{read_call}"),
	);
	let draft_result = &support::response_at(&responses, 0)["result"]["structuredContent"];
	let missing_authority_result = &support::response_at(&responses, 1)["result"];
	let accept_result = &support::response_at(&responses, 2)["result"]["structuredContent"];
	let read_result = &support::response_at(&responses, 3)["result"]["contents"][0]["text"];
	let read_json: Value =
		serde_json::from_str(read_result.as_str().expect("resource text should parse"))
			.expect("resource should be json");

	assert_eq!(draft_result["schema"], "decodex.mcp.autonomy_objective_result/1");
	assert_eq!(draft_result["objective"]["state"], "draft");
	assert_eq!(draft_result["persisted"], true);
	assert_eq!(missing_authority_result["isError"], true);
	assert_eq!(missing_authority_result["structuredContent"]["reason"], "missing_authority");
	assert_eq!(accept_result["schema"], "decodex.mcp.autonomy_objective_result/1");
	assert_eq!(accept_result["objective"]["state"], "accepted");
	assert_eq!(accept_result["objective"]["acceptance_present"], true);
	assert_eq!(accept_result["authority_effect"], "accepted_objective_no_execution_authority");
	assert_eq!(read_json["objective"]["objective_id"], "self-iteration-pilot");
	assert_eq!(read_json["objective"]["state"], "accepted");
}

#[test]
fn autonomy_accept_objective_refuses_caller_supplied_runtime_policy_authority() {
	let repo = support::test_repo();
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_autonomy_objective_draft("decodex", support::autonomy_objective_fixture())
		.expect("objective draft should persist");

	let responses = support::run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(state_store),
		},
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"autonomy_accept_objective","arguments":{"mode":"apply","objectiveId":"quality-autonomy","objectiveVersion":1,"authority":{"acceptedBy":"policy:auto","acceptedByKind":"runtime_policy","acceptanceSource":"caller-supplied-policy"}}}}"#,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["schema"], "decodex.mcp.refusal/1");
	assert_eq!(result["structuredContent"]["reason"], "objective_acceptance_refused");
	assert!(
		result["structuredContent"]["message"]
			.as_str()
			.expect("refusal message should be text")
			.contains("trusted Decodex authority state")
	);
}

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
