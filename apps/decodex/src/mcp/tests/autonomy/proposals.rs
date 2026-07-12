use std::{
	collections::{BTreeMap, BTreeSet},
	fs,
	path::{Path, PathBuf},
};

use rusqlite::{self, Connection};

use crate::{
	autonomy_objective::{AutonomyObjectiveAcceptance, AutonomyObjectiveActorKind},
	autonomy_proposal::{AutonomyProposalChallengeInput, AutonomyProposalChallengeSource},
	autonomy_runtime_policy::{self, RuntimePolicyProgramIntakeState},
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalInput, AutonomySignalPrivacy,
		AutonomySignalSourceType,
	},
	config::ServiceConfig,
	loop_contract::DecisionContractStatus,
	mcp::{
		McpContext,
		tests::support::{self},
	},
	state::{AutonomyRuntimePolicyReceiptInput, StateStore},
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
fn records_compile_challenge_and_refuses_self_accept() {
	let (_repo, db_path, proposal_id) = support::seed_autonomy_challenged_proposal();
	let self_accept_call = format!(
		r#"{{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{{"name":"autonomy_request_promotion","arguments":{{"mode":"apply","proposalId":"{proposal_id}","authority":{{"acceptedBy":"agent-a","acceptedByKind":"external_agent","acceptanceSource":"mcp-agent","reason":"self accept","proposalActor":"agent-a","proposalActorKind":"external_agent"}}}}}}}}"#
	);
	let self_accept = support::run_stdio_with_context(
		McpContext {
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
	let (_repo, db_path, proposal_id) = support::seed_autonomy_challenged_proposal();
	let fabricated_policy_call = format!(
		r#"{{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{{"name":"autonomy_request_promotion","arguments":{{"mode":"apply","proposalId":"{proposal_id}","authority":{{"acceptedBy":"agent-a","acceptedByKind":"external_agent","acceptanceSource":"runtime-policy","reason":"fabricated policy","proposalActor":"agent-a","proposalActorKind":"external_agent","acceptedProjectPolicy":{{"projectId":"decodex","objectiveId":"quality-autonomy","objectiveVersion":1,"acceptedPolicyId":"quality-autonomy-policy","acceptedPolicyVersion":"1","authorityRef":"runtime-policy:quality-autonomy-policy@1","authorizedActor":"agent-a","authorizedActorKind":"external_agent","authorizedAcceptanceSources":["runtime-policy"],"authorizedScopes":["autonomy_proposal_acceptance"]}}}}}}}}}}"#
	);
	let fabricated_policy_accept = support::run_stdio_with_context(
		McpContext {
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

#[test]
fn runtime_policy_acceptance_and_apply_are_separate_from_legacy_promotion_and_intake() {
	let (repo, db_path, proposal_id) = support::seed_autonomy_challenged_proposal();
	let config_path = runtime_policy_config(repo.path());
	let context = || McpContext {
		config_path: Some(config_path.clone()),
		project_id: Some(String::from("decodex")),
		state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
	};
	let legacy_call = format!(
		r#"{{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{{"name":"autonomy_request_promotion","arguments":{{"mode":"apply","proposalId":"{proposal_id}"}}}}}}"#
	);
	let legacy = support::run_stdio_with_context(context(), &legacy_call);

	assert_eq!(support::response_at(&legacy, 0)["result"]["isError"], true);
	assert_eq!(
		support::response_at(&legacy, 0)["result"]["structuredContent"]["reason"],
		"missing_authority"
	);

	let unaccepted_policy_call = format!(
		r#"{{"jsonrpc":"2.0","id":61,"method":"tools/call","params":{{"name":"autonomy_apply_runtime_policy","arguments":{{"mode":"apply","proposalId":"{proposal_id}"}}}}}}"#
	);
	let unaccepted = support::run_stdio_with_context(context(), &unaccepted_policy_call);

	assert_eq!(support::response_at(&unaccepted, 0)["result"]["isError"], true);
	assert_eq!(
		support::response_at(&unaccepted, 0)["result"]["structuredContent"]["reason"],
		"autonomy_runtime_policy_apply_refused"
	);

	let accepted_at = accept_runtime_policy_for_test(&db_path, &config_path);
	let dry_run_call = format!(
		r#"{{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{{"name":"autonomy_apply_runtime_policy","arguments":{{"mode":"dry_run","proposalId":"{proposal_id}"}}}}}}"#
	);
	let dry_run = support::run_stdio_with_context(context(), &dry_run_call);
	let dry_run_result = &support::response_at(&dry_run, 0)["result"];

	assert_eq!(dry_run_result["isError"], false);
	assert_eq!(dry_run_result["structuredContent"]["eligible"], true);
	assert_eq!(dry_run_result["structuredContent"]["persisted"], false);

	let contract_id = dry_run_result["structuredContent"]["decision_contract_id"]
		.as_str()
		.expect("contract id")
		.to_owned();

	assert!(
		StateStore::open(&db_path)
			.expect("state store should reopen")
			.decision_contract("decodex", &contract_id)
			.expect("contract readback")
			.is_none()
	);

	let apply_call = format!(
		r#"{{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{{"name":"autonomy_apply_runtime_policy","arguments":{{"mode":"apply","proposalId":"{proposal_id}"}}}}}}"#
	);
	let applied = support::run_stdio_with_context(context(), &apply_call);
	let result = &support::response_at(&applied, 0)["result"];
	let structured = &result["structuredContent"];
	let contract = StateStore::open(&db_path)
		.expect("state store should reopen")
		.decision_contract("decodex", &contract_id)
		.expect("contract should read")
		.expect("contract should persist");

	assert_eq!(result["isError"], false);
	assert_eq!(structured["schema"], "decodex.mcp.autonomy_runtime_policy_apply_result/1");
	assert_eq!(structured["execution_authority_granted"], true);
	assert_eq!(structured["program_intake_present"], false);
	assert_eq!(structured["intake_team_issue_identifier"], "XY-100");
	assert_eq!(contract.status(), DecisionContractStatus::AcceptedPromoted);
	assert_eq!(
		contract.contract().promotion().expect("promotion provenance should exist").accepted_at(),
		accepted_at
	);
	assert_eq!(
		contract.contract().accepted_authority().non_goals(),
		&[
			String::from("Do not bypass review or Program Intake."),
			String::from("Keep external drafts unpublished."),
		]
	);
	assert!(
		StateStore::open(&db_path)
			.expect("state store should reopen")
			.list_execution_programs("decodex")
			.expect("programs")
			.is_empty()
	);

	let replay = support::run_stdio_with_context(context(), &apply_call);
	let replay_result = &support::response_at(&replay, 0)["result"]["structuredContent"];

	assert_eq!(replay_result["execution_authority_granted"], true);
	assert_eq!(replay_result["program_intake_present"], false);

	let proposal = StateStore::open(&db_path)
		.expect("state store should reopen")
		.autonomy_proposal("decodex", &proposal_id)
		.expect("proposal should read")
		.expect("proposal should exist");

	assert_eq!(proposal.proposal().challenge_evidence().len(), 2);
	assert!(proposal.proposal().challenge_evidence().iter().any(|challenge| {
		challenge.evidence_refs().iter().any(|reference| {
			reference.starts_with("decodex:runtime-policy-challenge/quality-autonomy-policy/1/")
				&& reference.ends_with("/2")
		})
	}));

	assert_external_challenge_changes_internal_provenance(&db_path, &config_path, &proposal_id);
	assert_post_promotion_objection_blocks_intake(
		&db_path,
		&config_path,
		&proposal_id,
		&contract_id,
	);
}

fn assert_external_challenge_changes_internal_provenance(
	db_path: &Path,
	config_path: &Path,
	proposal_id: &str,
) {
	let store = StateStore::open(db_path).expect("state store should reopen");

	store
		.record_autonomy_proposal_challenge(
			"decodex",
			proposal_id,
			AutonomyProposalChallengeInput {
				source: AutonomyProposalChallengeSource::InlineSkeptic,
				actor: String::from("additional-skeptic"),
				summary: String::from("Additional current evidence found no objection."),
				objections: Vec::new(),
				evidence_refs: vec![String::from("evidence:additional-current")],
				recorded_at: String::from("2026-07-10T12:20:00Z"),
			},
		)
		.expect("additional challenge should persist");

	let config = ServiceConfig::from_path(config_path).expect("runtime policy config should load");
	let replay_error = autonomy_runtime_policy::apply_registered_policy_promotion(
		&config,
		&store,
		"decodex",
		proposal_id,
	)
	.expect_err("changed external challenge evidence must invalidate the promoted contract");

	assert!(replay_error.to_string().contains("existing_contract_identity_mismatch"));

	let proposal = store
		.autonomy_proposal("decodex", proposal_id)
		.expect("proposal should read")
		.expect("proposal should exist");
	let internal_refs = proposal
		.proposal()
		.challenge_evidence()
		.iter()
		.flat_map(|challenge| challenge.evidence_refs())
		.filter(|reference| reference.starts_with("decodex:runtime-policy-challenge/"))
		.collect::<BTreeSet<_>>();

	assert_eq!(internal_refs.len(), 2);
}

fn assert_post_promotion_objection_blocks_intake(
	db_path: &Path,
	config_path: &Path,
	proposal_id: &str,
	contract_id: &str,
) {
	let store = StateStore::open(db_path).expect("state store should reopen");
	let config = ServiceConfig::from_path(config_path).expect("runtime policy config should load");

	store
		.record_autonomy_proposal_challenge(
			"decodex",
			proposal_id,
			AutonomyProposalChallengeInput {
				source: AutonomyProposalChallengeSource::InlineSkeptic,
				actor: String::from("post-promotion-skeptic"),
				summary: String::from("New evidence invalidates execution authority."),
				objections: vec![String::from("post_promotion_blocker")],
				evidence_refs: vec![String::from("evidence:post-promotion")],
				recorded_at: String::from("2026-07-10T12:30:00Z"),
			},
		)
		.expect("post-promotion challenge should persist");

	let error = autonomy_runtime_policy::ensure_contract_proposal_still_eligible(
		&config,
		&store,
		"decodex",
		contract_id,
	)
	.expect_err("post-promotion objection must revoke Program Intake eligibility");

	assert!(
		error.to_string().contains("recorded_challenge:post_promotion_blocker")
			|| error.to_string().contains("existing_contract_identity_mismatch")
	);

	Connection::open(db_path)
		.expect("sqlite should open")
		.execute(
			"DELETE FROM autonomy_proposals WHERE project_id = 'decodex' AND proposal_id = ?1",
			rusqlite::params![proposal_id],
		)
		.expect("proposal should delete for missing-source counterexample");

	let missing_source = autonomy_runtime_policy::ensure_contract_proposal_still_eligible(
		&config,
		&StateStore::open(db_path).expect("state store should reopen"),
		"decodex",
		contract_id,
	)
	.expect_err("runtime-policy contract without its source proposal must fail closed");

	assert!(missing_source.to_string().contains("intake_source_proposal_missing"));
}

fn accept_runtime_policy_for_test(db_path: &Path, config_path: &Path) -> String {
	let principal =
		autonomy_runtime_policy::resolved_local_principal().expect("test principal should resolve");
	let accepted_at =
		autonomy_runtime_policy::current_rfc3339().expect("acceptance timestamp should format");
	let config = ServiceConfig::from_path(config_path).expect("runtime policy config should load");
	let receipt_store = StateStore::open(db_path).expect("state store should reopen");
	let candidate = autonomy_runtime_policy::registered_policy_candidate(
		&config,
		&receipt_store,
		"decodex",
		&principal,
		&accepted_at,
		"decodex-operator-cli",
		vec![
			String::from("Do not bypass review or Program Intake."),
			String::from("Keep external drafts unpublished."),
		],
	)
	.expect("operator candidate should validate");
	let candidate_digest = autonomy_runtime_policy::runtime_policy_candidate_digest(&candidate)
		.expect("candidate digest should compute");
	let receipt_id =
		autonomy_runtime_policy::new_operator_receipt_id().expect("receipt id should generate");

	receipt_store
		.issue_autonomy_runtime_policy_receipt(AutonomyRuntimePolicyReceiptInput {
			project_id: "decodex",
			receipt_id: &receipt_id,
			principal: &principal,
			candidate_digest: &candidate_digest,
			candidate: &candidate,
			created_at: &accepted_at,
			expires_at_unix: autonomy_runtime_policy::operator_receipt_expiry_unix(),
		})
		.expect("operator receipt should persist");

	let accept_call = r#"{"jsonrpc":"2.0","id":71,"method":"tools/call","params":{"name":"autonomy_accept_runtime_policy","arguments":{"mode":"apply"}}}"#;
	let accepted = support::run_stdio_with_context(
		McpContext {
			config_path: Some(config_path.to_path_buf()),
			project_id: Some(String::from("decodex")),
			state_store: Some(StateStore::open(db_path).expect("state store should reopen")),
		},
		accept_call,
	);
	let accepted_result = &support::response_at(&accepted, 0)["result"];

	assert_eq!(accepted_result["isError"], true);
	assert_eq!(
		accepted_result["structuredContent"]["reason"],
		"autonomy_runtime_policy_operator_cli_required"
	);

	receipt_store
		.accept_autonomy_runtime_policy_with_receipt("decodex", &receipt_id, &principal)
		.expect("operator CLI receipt consumption should persist accepted policy");

	accepted_at
}

#[test]
fn runtime_policy_acceptance_rejects_unreasonable_future_timestamp() {
	let (repo, db_path, _proposal_id) = support::seed_autonomy_challenged_proposal();
	let call = r#"{"jsonrpc":"2.0","id":72,"method":"tools/call","params":{"name":"autonomy_accept_runtime_policy","arguments":{"mode":"dry_run","publicNonGoals":["Do not bypass review."],"authority":{"acceptedBy":"operator","acceptedByKind":"user","acceptedAt":"2999-01-01T00:00:00Z","acceptanceSource":"conversation"}}}}"#;
	let responses = support::run_stdio_with_context(
		McpContext {
			config_path: Some(runtime_policy_config(repo.path())),
			project_id: Some(String::from("decodex")),
			state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
		},
		call,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["reason"], "autonomy_runtime_policy_acceptance_refused");
}

#[test]
fn runtime_policy_acceptance_refuses_private_public_projection() {
	let (repo, db_path, _proposal_id) = support::seed_autonomy_challenged_proposal();
	let call = r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"autonomy_accept_runtime_policy","arguments":{"mode":"dry_run","publicNonGoals":["Do not expose GITHUB_PAT_PRIVATE."],"authority":{"acceptedBy":"operator","acceptedByKind":"user","acceptedAt":"2026-07-10T12:00:00Z","acceptanceSource":"conversation"}}}}"#;
	let responses = support::run_stdio_with_context(
		McpContext {
			config_path: Some(runtime_policy_config(repo.path())),
			project_id: Some(String::from("decodex")),
			state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
		},
		call,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["reason"], "autonomy_runtime_policy_acceptance_refused");
	assert!(!result["structuredContent"]["message"].as_str().expect("message").contains("GITHUB"));
}

#[test]
fn runtime_policy_blocks_recorded_objections_and_detects_partial_intake() {
	let (repo, db_path, proposal_id) = support::seed_autonomy_challenged_proposal();
	let config = ServiceConfig::from_path(runtime_policy_config(repo.path()))
		.expect("runtime policy config should load");
	let store = StateStore::open(&db_path).expect("state store should open");
	let policy = autonomy_runtime_policy::registered_policy_candidate(
		&config,
		&store,
		"decodex",
		"operator",
		&autonomy_runtime_policy::current_rfc3339().expect("timestamp should format"),
		"test",
		vec![String::from("Do not bypass review or Program Intake.")],
	)
	.expect("policy candidate should validate");

	store.accept_autonomy_runtime_policy(policy).expect("policy should persist");
	store
		.record_autonomy_proposal_challenge(
			"decodex",
			&proposal_id,
			AutonomyProposalChallengeInput {
				source: AutonomyProposalChallengeSource::InlineSkeptic,
				actor: String::from("independent-skeptic"),
				summary: String::from("A blocking objection remains."),
				objections: vec![String::from("claim_safety_not_proven")],
				evidence_refs: vec![String::from("review:test")],
				recorded_at: String::from("2026-07-10T12:01:00Z"),
			},
		)
		.expect("challenge should persist");

	let evaluation = autonomy_runtime_policy::evaluate_registered_policy_promotion(
		&config,
		&store,
		"decodex",
		&proposal_id,
	)
	.expect("evaluation should complete");

	assert!(
		evaluation.objections.contains(&String::from("recorded_challenge:claim_safety_not_proven"))
	);
	assert_eq!(evaluation.program_intake_state, RuntimePolicyProgramIntakeState::Absent);
}

#[test]
fn public_challenge_cannot_impersonate_runtime_policy_provenance() {
	let (repo, db_path, proposal_id) = support::seed_autonomy_challenged_proposal();
	let call = format!(
		r#"{{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{{"name":"autonomy_challenge_proposal","arguments":{{"mode":"apply","proposalId":"{proposal_id}","challenge":{{"source":"inline_skeptic","actor":"decodex-runtime-policy-challenger","summary":"spoof","evidenceRefs":["decodex:runtime-policy-internal-challenge/1"]}},"authority":{{"source":"test","reason":"test"}}}}}}}}"#
	);
	let responses = support::run_stdio_with_context(
		McpContext {
			config_path: Some(runtime_policy_config(repo.path())),
			project_id: Some(String::from("decodex")),
			state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
		},
		&call,
	);
	let result = &support::response_at(&responses, 0)["result"];

	assert_eq!(result["isError"], true);
	assert_eq!(result["structuredContent"]["reason"], "invalid_arguments");
}

#[test]
fn runtime_policy_replay_rejects_same_id_contract_with_weakened_identity() {
	let (repo, db_path, proposal_id) = support::seed_autonomy_challenged_proposal();
	let config = ServiceConfig::from_path(runtime_policy_config(repo.path()))
		.expect("runtime policy config should load");
	let store = StateStore::open(&db_path).expect("state store should open");
	let policy = autonomy_runtime_policy::registered_policy_candidate(
		&config,
		&store,
		"decodex",
		"operator",
		&autonomy_runtime_policy::current_rfc3339().expect("timestamp should format"),
		"test",
		vec![String::from("Do not bypass review or Program Intake.")],
	)
	.expect("policy candidate should validate");

	store.accept_autonomy_runtime_policy(policy).expect("policy should persist");

	let outcome = autonomy_runtime_policy::apply_registered_policy_promotion(
		&config,
		&store,
		"decodex",
		&proposal_id,
	)
	.expect("promotion should succeed");
	let contract_id = outcome.contract.contract_id().to_owned();

	drop(store);

	let connection = Connection::open(&db_path).expect("runtime db should open");
	let payload: String = connection
		.query_row(
			"SELECT payload_json FROM decision_contracts WHERE project_id = ?1 AND contract_id = ?2",
			rusqlite::params!["decodex", &contract_id],
			|row| row.get(0),
		)
		.expect("contract payload should read");
	let mut payload: serde_json::Value =
		serde_json::from_str(&payload).expect("contract payload should parse");

	payload["accepted_authority"]["non_goals"][0] =
		serde_json::Value::String(String::from("Do less review."));

	connection
		.execute(
			"UPDATE decision_contracts SET payload_json = ?1 WHERE project_id = ?2 AND contract_id = ?3",
			rusqlite::params![payload.to_string(), "decodex", &contract_id],
		)
		.expect("tampered contract should persist");

	drop(connection);

	let replay = autonomy_runtime_policy::evaluate_registered_policy_promotion(
		&config,
		&StateStore::open(&db_path).expect("state store should reopen"),
		"decodex",
		&proposal_id,
	)
	.expect_err("weakened same-id contract must be refused");

	assert!(replay.to_string().contains("existing_contract_identity_mismatch"));
}

fn runtime_policy_config(repo_root: &Path) -> PathBuf {
	let config_path = repo_root.join("project.toml");
	let body = format!(
		r#"
			service_id = "decodex"
			[paths]
			repo_root = "{}"
			[tracker]
			api_key_env_var = "HOME"
team_id = "team-test"
			[github]
			token_env_var = "HOME"
owner = "test-owner"
repository = "test-repository"
			[autonomy]
			auto_promote = true
			auto_intake = true
			[autonomy.runtime_policy]
			accepted_objective_id = "quality-autonomy"
			accepted_objective_version = "1"
			accepted_policy_id = "quality-autonomy-policy"
			accepted_policy_version = "1"
			policy_authority_ref = "decodex.runtime_policy:quality-autonomy-policy@1"
			team_issue_identifier = "XY-100"
		"#,
		repo_root.display()
	);

	fs::write(&config_path, body).expect("runtime policy config should write");

	config_path
}
