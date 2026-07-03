use std::{collections::BTreeMap, path::PathBuf};

use tempfile::TempDir;

use crate::{
	autonomy_objective::{
		AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind, AutonomyObjectiveContract,
	},
	autonomy_proposal::AutonomyProposalCompileInput,
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalInput, AutonomySignalPrivacy,
		AutonomySignalSourceType,
	},
	mcp::{
		McpContext,
		tests::support::{repo_fixtures, stdio},
	},
	state::StateStore,
};

pub(in crate::mcp::tests) fn seed_autonomy_challenged_proposal() -> (TempDir, PathBuf, String) {
	let repo = repo_fixtures::test_repo();
	let db_path = repo.path().join("runtime.sqlite3");
	let state_store = StateStore::open(&db_path).expect("state store should open");

	state_store
		.upsert_autonomy_objective_draft("decodex", autonomy_objective_fixture())
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

	let signal_call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"autonomy_submit_signal","arguments":{"mode":"apply","kind":"runtime_health","signal":{"objectiveId":"quality-autonomy","objectiveVersion":1,"sourceType":"runtime","sourceRefs":["status:XY-1090"],"freshness":"fresh","summary":"Runtime status is consistent.","evidence":["status readback summarized"],"evidenceClass":"live_readback","confidence":"high","privacy":"team"},"authority":{"source":"mcp-test","reason":"submit evidence"}}}}"#;
	let signal_responses = stdio::run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
		},
		signal_call,
	);
	let signal_result = &stdio::response_at(&signal_responses, 0)["result"]["structuredContent"];
	let signal_id = signal_result["signal"]["signal_id"].as_str().expect("signal id");
	let proposal_call = format!(
		r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"autonomy_compile_proposal","arguments":{{"mode":"apply","signalIds":["{signal_id}"],"proposal":{{"objectiveId":"quality-autonomy","objectiveVersion":1,"sourceFamily":"runtime_health","intendedSurface":"apps/decodex/src/mcp.rs","summary":"Expose autonomy MCP surface.","challengeRequirements":["independent challenge"],"rollbackPath":"Revert MCP autonomy surface."}},"authority":{{"source":"mcp-test","reason":"compile proposal evidence"}}}}}}}}"#
	);
	let proposal_responses = stdio::run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
		},
		&proposal_call,
	);
	let proposal_result =
		&stdio::response_at(&proposal_responses, 0)["result"]["structuredContent"];
	let proposal_id = proposal_result["proposal"]["proposal_id"].as_str().expect("proposal id");
	let challenge_call = format!(
		r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"autonomy_challenge_proposal","arguments":{{"mode":"apply","proposalId":"{proposal_id}","challenge":{{"source":"inline_skeptic","actor":"skeptic","summary":"Challenge recorded.","objections":[]}},"authority":{{"source":"mcp-test","reason":"record challenge"}}}}}}}}"#
	);
	let challenge_responses = stdio::run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
		},
		&challenge_call,
	);
	let challenge_result =
		&stdio::response_at(&challenge_responses, 0)["result"]["structuredContent"];

	assert_eq!(signal_result["schema"], "decodex.mcp.autonomy_signal_result/1");
	assert_eq!(signal_result["persisted"], true);
	assert_eq!(proposal_result["schema"], "decodex.mcp.autonomy_proposal_result/1");
	assert_eq!(proposal_result["proposal"]["state"], "decision_candidate");
	assert_eq!(challenge_result["schema"], "decodex.mcp.autonomy_challenge_result/1");
	assert_eq!(challenge_result["challenge_evidence_count"], 1);

	(repo, db_path, proposal_id.to_owned())
}

pub(in crate::mcp::tests) fn autonomy_objective_fixture() -> AutonomyObjectiveContract {
	serde_json::from_value(serde_json::json!({
		"schema": "decodex.autonomy_objective/1",
		"record_version": 1,
		"project_id": "decodex",
		"id": "quality-autonomy",
		"version": 1,
		"state": "draft",
		"summary": "Improve Decodex autonomy quality under explicit authority.",
		"goals": ["Reduce repeated validation and review churn."],
		"non_goals": ["Do not bypass Decision Contract authority."],
		"metrics": ["Validation retry count stays below objective tolerance."],
		"allowed_surfaces": ["apps/decodex/src/mcp.rs", "docs/spec/autonomy-control-plane.md"],
		"allowed_signal_kinds": ["runtime_health", "docs_skill_drift"],
		"validation_gates": ["cargo test -p decodex mcp --lib"],
		"review_policy": "independent current-head review required",
		"memory_policy": "source-linked read-only memory only",
		"report_policy": "public-safe summaries only"
	}))
	.expect("autonomy objective fixture should deserialize")
}

pub(in crate::mcp::tests) fn seed_autonomy_mcp_state(state_store: &StateStore) -> String {
	state_store
		.upsert_autonomy_objective_draft("decodex", autonomy_objective_fixture())
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
		source_refs: vec![String::from("status:XY-1090")],
		primary_source_refs: Vec::new(),
		issue_id: Some(String::from("XY-1090")),
		run_id: None,
		attempt_id: None,
		head_sha: None,
		captured_at: String::from("2026-06-23T00:01:00Z"),
		freshness: AutonomySignalFreshness::Fresh,
		summary: String::from("Runtime status is consistent."),
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

	let proposal = state_store
		.compile_autonomy_proposal_dry_run(
			AutonomyProposalCompileInput {
				project_id: String::from("decodex"),
				objective_id: String::from("quality-autonomy"),
				objective_version: 1,
				source_family: String::from("runtime_health"),
				intended_surface: String::from("apps/decodex/src/mcp.rs"),
				affected_identifiers: vec![String::from("XY-1090")],
				summary: String::from("Expose autonomy MCP surface."),
				challenge_requirements: vec![String::from("independent challenge")],
				rejected_alternatives: Vec::new(),
				rollback_path: String::from("Revert MCP autonomy surface."),
				weakened_validation_or_review: Vec::new(),
				issue_candidates: Vec::new(),
				created_at: String::from("2026-06-23T00:02:00Z"),
			},
			&[signal_id],
		)
		.expect("proposal should compile");
	let proposal_id = proposal.id().to_owned();

	state_store.record_autonomy_proposal("decodex", proposal).expect("proposal should persist");

	proposal_id
}
