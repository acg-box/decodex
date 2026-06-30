use std::{fs, io::Cursor, path::Path, process, str};

use serde_json::Value;
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
	loop_contract::{DecisionContract, DecisionPromotion, DecisionPromotionActorKind},
	mcp::{
		self, DEFAULT_MCP_HTTP_LISTEN_ADDRESS, McpCapabilityProfile, McpContext,
		McpHttpAuthorization, McpHttpHandler, McpHttpSessions, McpServer, McpTransport,
	},
	runtime,
	state::{self, ProtocolActivityEventSummary, ProtocolActivitySummary, StateStore},
	test_support::TestEnvVarGuard,
};

pub(super) struct ParsedHttpResponse {
	pub(super) status: String,
	pub(super) headers: Vec<(String, String)>,
	pub(super) body: Vec<u8>,
}

impl ParsedHttpResponse {
	pub(super) fn parse(response: &[u8]) -> Self {
		let header_end = mcp::http_header_end(response).expect("response should include headers");
		let headers = str::from_utf8(&response[..header_end]).expect("headers should be utf-8");
		let mut lines = headers.split("\r\n");
		let status = lines.next().expect("status line should exist").to_owned();
		let headers = lines
			.filter_map(|line| {
				let (name, value) = line.split_once(':')?;

				Some((name.trim().to_ascii_lowercase(), value.trim().to_owned()))
			})
			.collect();

		Self { status, headers, body: response[(header_end + 4)..].to_vec() }
	}

	pub(super) fn header(&self, name: &str) -> Option<&str> {
		self.headers
			.iter()
			.find(|(header, _)| header == &name.to_ascii_lowercase())
			.map(|(_, value)| value.as_str())
	}

	pub(super) fn json_body(&self) -> Value {
		serde_json::from_slice(&self.body).expect("HTTP body should be JSON")
	}

	pub(super) fn body_text(&self) -> &str {
		str::from_utf8(&self.body).expect("HTTP body should be utf-8")
	}
}

pub(super) fn assert_public_lane_inspect_resource(value: &Value) {
	assert_eq!(value["schema"], "decodex.mcp.lane_inspect/1");
	assert_eq!(value["projectId"], "pubfi");
	assert_eq!(value["issue"], "PUB-012");
	assert_eq!(value["matchedRunCount"], 1);

	let run = &value["runs"][0];

	assert_eq!(run["runId"], "run-12");
	assert!(run["status"].as_str().is_some());
	assert!(run["phase"].as_str().is_some());
	assert!(run["currentOperation"].as_str().is_some());
	assert!(run["laneControlNextAction"].as_str().is_some());
	assert!(run["eventCount"].as_i64().is_some());

	assert_no_lane_runtime_identifiers(value);
}

pub(super) fn assert_public_lane_control_readback(value: &Value) {
	assert_eq!(value["schema"], "decodex.mcp.lane_control_readback/1");
	assert_eq!(value["project_id"], "pubfi");
	assert_eq!(value["read_only"], true);

	let run = find_public_lane_control_run(value, "run-12");

	assert_eq!(run["run_id"], "run-12");
	assert!(run["status"].as_str().is_some());
	assert!(run["phase"].as_str().is_some());
	assert!(run["current_operation"].as_str().is_some());
	assert!(run["lane_control_next_action"].as_str().is_some());
	assert!(run["event_count"].as_i64().is_some());

	assert_no_lane_runtime_identifiers(value);
}

pub(super) fn find_public_lane_control_run<'a>(value: &'a Value, run_id: &str) -> &'a Value {
	for key in ["current_lanes", "recent_runs"] {
		if let Some(run) = value[key]
			.as_array()
			.into_iter()
			.flatten()
			.find(|run| run.get("run_id").and_then(Value::as_str) == Some(run_id))
		{
			return run;
		}
	}

	panic!("public lane-control readback should include run {run_id}");
}

pub(super) fn assert_no_lane_runtime_identifiers(value: &Value) {
	let serialized = serde_json::to_string(value).expect("value should serialize");

	for sensitive in [
		"threadId",
		"turnId",
		"threadStatus",
		"processId",
		"processAlive",
		"processLivenessReason",
		"thread_id",
		"turn_id",
		"thread_status",
		"process_id",
		"process_alive",
		"process_liveness_reason",
		"worktreePath",
		"worktree_path",
		"thread-12",
		"turn-12",
	] {
		assert!(!serialized.contains(sensitive), "lane inspect leaked {sensitive}");
	}
}

pub(super) fn seed_autonomy_challenged_proposal() -> (TempDir, std::path::PathBuf, String) {
	let repo = test_repo();
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
	let signal_responses = run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
		},
		signal_call,
	);
	let signal_result = &response_at(&signal_responses, 0)["result"]["structuredContent"];
	let signal_id = signal_result["signal"]["signal_id"].as_str().expect("signal id");
	let proposal_call = format!(
		r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"autonomy_compile_proposal","arguments":{{"mode":"apply","signalIds":["{signal_id}"],"proposal":{{"objectiveId":"quality-autonomy","objectiveVersion":1,"sourceFamily":"runtime_health","intendedSurface":"apps/decodex/src/mcp.rs","summary":"Expose autonomy MCP surface.","challengeRequirements":["independent challenge"],"rollbackPath":"Revert MCP autonomy surface."}},"authority":{{"source":"mcp-test","reason":"compile proposal evidence"}}}}}}}}"#
	);
	let proposal_responses = run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
		},
		&proposal_call,
	);
	let proposal_result = &response_at(&proposal_responses, 0)["result"]["structuredContent"];
	let proposal_id = proposal_result["proposal"]["proposal_id"].as_str().expect("proposal id");
	let challenge_call = format!(
		r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"autonomy_challenge_proposal","arguments":{{"mode":"apply","proposalId":"{proposal_id}","challenge":{{"source":"inline_skeptic","actor":"skeptic","summary":"Challenge recorded.","objections":[]}},"authority":{{"source":"mcp-test","reason":"record challenge"}}}}}}}}"#
	);
	let challenge_responses = run_stdio_with_context(
		McpContext {
			repo_root: repo.path().to_path_buf(),
			config_path: None,
			project_id: Some(String::from("decodex")),
			state_store: Some(StateStore::open(&db_path).expect("state store should reopen")),
		},
		&challenge_call,
	);
	let challenge_result = &response_at(&challenge_responses, 0)["result"]["structuredContent"];

	assert_eq!(signal_result["schema"], "decodex.mcp.autonomy_signal_result/1");
	assert_eq!(signal_result["persisted"], true);
	assert_eq!(proposal_result["schema"], "decodex.mcp.autonomy_proposal_result/1");
	assert_eq!(proposal_result["proposal"]["state"], "decision_candidate");
	assert_eq!(challenge_result["schema"], "decodex.mcp.autonomy_challenge_result/1");
	assert_eq!(challenge_result["challenge_evidence_count"], 1);

	(repo, db_path, proposal_id.to_owned())
}

pub(super) fn run_stdio(repo_root: &Path, input: &str) -> Vec<Value> {
	run_stdio_raw(repo_root, input)
		.lines()
		.map(|line| serde_json::from_str::<Value>(line).expect("response should be JSON"))
		.collect()
}

pub(super) fn run_stdio_with_context(context: McpContext, input: &str) -> Vec<Value> {
	run_stdio_raw_with_context(context, input)
		.lines()
		.map(|line| serde_json::from_str::<Value>(line).expect("response should be JSON"))
		.collect()
}

pub(super) fn run_stdio_with_profile(
	repo_root: &Path,
	capability_profile: McpCapabilityProfile,
	input: &str,
) -> Vec<Value> {
	let context = McpContext {
		repo_root: repo_root.to_path_buf(),
		config_path: None,
		project_id: None,
		state_store: None,
	};

	run_stdio_raw_with_profile(context, capability_profile, input)
		.lines()
		.map(|line| serde_json::from_str::<Value>(line).expect("response should be JSON"))
		.collect()
}

pub(super) fn project_mcp_context(repo_root: &Path, config_path: &Path) -> McpContext {
	McpContext {
		repo_root: repo_root.to_path_buf(),
		config_path: Some(config_path.to_path_buf()),
		project_id: Some(String::from("pubfi")),
		state_store: None,
	}
}

pub(super) fn run_stdio_raw(repo_root: &Path, input: &str) -> String {
	let context = McpContext {
		repo_root: repo_root.to_path_buf(),
		config_path: None,
		project_id: None,
		state_store: None,
	};

	run_stdio_raw_with_context(context, input)
}

pub(super) fn run_stdio_raw_with_context(context: McpContext, input: &str) -> String {
	run_stdio_raw_with_profile(context, McpCapabilityProfile::Admin, input)
}

pub(super) fn run_stdio_raw_with_profile(
	context: McpContext,
	capability_profile: McpCapabilityProfile,
	input: &str,
) -> String {
	let mut output = Vec::new();

	mcp::serve_stdio_with_profile(
		Cursor::new(format!("{input}\n")),
		&mut output,
		context,
		capability_profile,
	)
	.expect("stdio server should run");

	String::from_utf8(output).expect("stdout should be utf-8")
}

pub(super) fn response_at(responses: &[Value], index: usize) -> &Value {
	responses.get(index).expect("response should exist")
}

pub(super) fn response_error(responses: &[Value], index: usize) -> &Value {
	response_at(responses, index).get("error").expect("error response")
}

pub(super) fn resource_response_json(responses: &[Value], index: usize) -> Value {
	let contents = response_at(responses, index)["result"]["contents"]
		.as_array()
		.expect("resource contents array");
	let text = contents[0]["text"].as_str().expect("resource text should exist");

	serde_json::from_str(text).expect("resource text should be JSON")
}

pub(super) fn assert_tool_output_schema_variant(
	tool: &Value,
	schema: &str,
	required_field: Option<&str>,
) {
	let variants = tool["outputSchema"]["oneOf"].as_array().expect("oneOf variants");
	let variant = variants
		.iter()
		.find(|variant| {
			variant["properties"]["schema"]["enum"]
				.as_array()
				.expect("schema enum")
				.iter()
				.any(|value| value.as_str() == Some(schema))
		})
		.expect("schema variant should exist");

	if let Some(required_field) = required_field {
		assert!(
			variant["required"]
				.as_array()
				.expect("required array")
				.iter()
				.any(|value| value.as_str() == Some(required_field))
		);
	}
}

pub(super) fn sensitive_observability_fixture() -> Value {
	serde_json::json!({
		"schema": "decodex.operator.snapshot/1",
		"project": {
			"repoRoot": "/private/repo",
			"config_path": "/private/project.toml",
			"visible": "kept"
		},
		"runs": [
			{
				"issue": "XY-994",
				"effective_cwd": "/private/worktree",
				"private_evidence": {
					"read_command": "decodex evidence --config /private/project.toml --issue XY-994"
				},
				"github_cli_authority": {
					"github_command_path": "/private/bin/gh",
					"github_token_env_var": "GITHUB_PAT_Y"
				},
				"nested": {
					"readCommand": "decodex evidence --config /private/project.toml",
					"privateEvidenceRef": "private-ref",
					"safe": "kept"
				}
			}
		]
	})
}

pub(super) fn observability_snapshot_fixture() -> Value {
	serde_json::json!({
		"schema": "decodex.mcp.status_resource/1",
		"project_id": "decodex",
		"status_source": "live",
		"run_limit": 10,
		"current_lanes": [observability_current_lane_fixture()],
		"recent_runs": [observability_recent_run_fixture()],
		"post_review_lanes": [observability_post_review_lane_fixture()]
	})
}

pub(super) fn observability_current_lane_fixture() -> Value {
	serde_json::json!({
		"run_id": "run-1",
		"issue_id": "issue-1",
		"issue_identifier": "XY-996",
		"attempt_number": 2,
		"status": "running",
		"attempt_status": "starting",
		"phase": "implementing",
		"run_phase": "implement_to_validation_ready",
		"wait_reason": "model_execution",
		"current_operation": "model_execution",
		"lane_control_next_action": "inspect_or_interrupt_orphaned_live_thread",
		"event_count": 6,
		"last_event_type": "turn/delta",
		"last_event_at": "2026-06-18T00:00:00Z",
		"last_protocol_activity_at": "2026-06-18T00:00:01Z",
		"last_progress_at": "2026-06-18T00:00:02Z",
		"progress_diagnostic": "protocol_only_activity",
		"suspected_stall": false,
		"protocol_activity": observability_protocol_activity_fixture(),
		"child_agent_activity": {
			"event_count": 2,
			"current_bucket": "protocol_activity",
			"path": "/private/activity-marker"
		},
		"phase_acceptance": observability_phase_acceptance_fixture(),
		"private_evidence": {
			"raw": "hidden"
		},
		"worktree_path": "/private/worktree"
	})
}

pub(super) fn observability_protocol_activity_fixture() -> Value {
	serde_json::json!({
		"turn_status": "running",
		"waiting_reason": "model_execution",
		"recent_events": [
			{
				"event_type": "turn/delta",
				"category": "work_progress",
				"detail": "diff updated",
				"private_evidence": "private-ref"
			},
			{
				"event_type": "response/reasoning/summary",
				"category": "reasoning",
				"detail": "hidden chain of thought",
				"text": "private reasoning text",
				"summary": "private reasoning summary",
				"body": "private reasoning body"
			},
			{
				"event_type": "configWarning",
				"category": "warning",
				"detail": "config at /private/worktree using GITHUB_PAT_Y"
			},
			{
				"event_type": "error",
				"category": "protocol_error",
				"detail": "failed under /Users/x/worktree with LINEAR_API_KEY_HACKINK"
			},
			{
				"event_type": "configWarning",
				"category": "warning",
				"detail": "state marker under /srv/decodex/runtime"
			},
			{
				"event_type": "error",
				"category": "protocol_error",
				"detail": "upstream auth failed for ghp_abcdefghijklmnopqrstuvwxyz123456"
			},
			{
				"event_type": "error",
				"category": "protocol_error",
				"detail": "upstream auth failed for 8Nf4Qz7Lb2Rc9Vx5Tm3Pq6Wy1Hs8Ka0U"
			}
		]
	})
}

pub(super) fn observability_phase_acceptance_fixture() -> Value {
	serde_json::json!({
		"phase": "handoff_evidence",
		"decision": "accepted",
		"reason_code": "phase_goal_satisfied",
		"objective_covered": true,
		"effective_delta_present": true,
		"changed_surfaces": ["phase-private-surface"],
		"non_goal_passed": true,
		"validation_passed": true,
		"recorded_at": "2026-06-18T00:00:03Z",
		"run_id": "phase-private-run",
		"attempt_number": 2,
		"next_action": "request_review"
	})
}

pub(super) fn observability_review_status_fixture(
	head_sha: &str,
	active_fingerprint: &str,
	stop_fingerprint: &str,
	round: i64,
) -> Value {
	serde_json::json!({
		"phase": "handoff",
		"status": "pending",
		"checkpoint": {
			"head_sha": head_sha,
			"round": round,
			"nonclean_rounds": 2,
			"active_fingerprints": [active_fingerprint],
			"stop_fingerprint": stop_fingerprint,
			"updated_at": "2026-06-18T00:00:04Z"
		},
		"privateEvidenceRef": "private-review-ref"
	})
}

pub(super) fn observability_recent_run_fixture() -> Value {
	serde_json::json!({
		"run_id": "run-1",
		"issue_id": "issue-1",
		"issue_identifier": "XY-996",
		"status": "running",
		"loop_status": {
			"review": {
				"status": "duplicate_recent"
			}
		}
	})
}

pub(super) fn observability_post_review_lane_fixture() -> Value {
	serde_json::json!({
		"project_id": "decodex",
		"issue_id": "issue-1",
		"issue_identifier": "XY-996",
		"issue_state": "In Review",
		"branch_name": "private-branch-name",
		"worktree_path": "/private/review-worktree",
		"classification": "review_pending",
		"reason": "external_review_pending",
		"pr_url": "https://example/pr/1",
		"pr_head_sha": "private-pr-head",
		"pr_state": "OPEN",
		"review_state": "pending",
		"review_decision": "REVIEW_REQUIRED",
		"mergeable": "MERGEABLE",
		"check_state": "PENDING",
		"unresolved_review_threads": 1,
		"shadowed_by_current_lane": false,
		"readback_warning": "none",
		"readback_root_cause": "none",
		"loop_status": {
			"review": observability_review_status_fixture(
				"private-lane-head-sha",
				"lane-fingerprint-private",
				"lane-stop-fingerprint-private",
				4
			)
		},
		"private_evidence_ref": "private-pr-ref"
	})
}

pub(super) fn assert_observability_is_sanitized(value: &Value) {
	let serialized = serde_json::to_string(value).expect("value should serialize");

	for sensitive in [
		"repoRoot",
		"config_path",
		"effective_cwd",
		"private_evidence",
		"privateEvidenceRef",
		"read_command",
		"readCommand",
		"github_cli_authority",
		"github_command_path",
		"github_token_env_var",
		"/private",
		"GITHUB_PAT_Y",
	] {
		assert!(!serialized.contains(sensitive), "sanitized value leaked {sensitive}");
	}

	assert!(serialized.contains("kept"));
}

pub(super) fn assert_no_sensitive_observability_content(value: &Value) {
	let serialized = serde_json::to_string(value).expect("value should serialize");

	for sensitive in [
		"/private",
		"/Users/x",
		"private_evidence",
		"privateEvidenceRef",
		"private_evidence_ref",
		"private-ref",
		"private-review-ref",
		"private-pr-ref",
		"worktree_path",
		"worktreePath",
		"raw",
		"hidden chain of thought",
		"private reasoning text",
		"private reasoning summary",
		"private reasoning body",
		"GITHUB_PAT_Y",
		"LINEAR_API_KEY_HACKINK",
		"/srv/decodex/runtime",
		"ghp_abcdefghijklmnopqrstuvwxyz123456",
		"8Nf4Qz7Lb2Rc9Vx5Tm3Pq6Wy1Hs8Ka0U",
		"active_fingerprints",
		"stop_fingerprint",
		"head_sha",
		"changed_surfaces",
		"recorded_at",
		"phase-private-surface",
		"phase-private-run",
		"private-head-sha",
		"fingerprint-private",
		"stop-fingerprint-private",
		"private-branch-name",
		"private-pr-head",
		"private-lane-head-sha",
		"lane-fingerprint-private",
		"lane-stop-fingerprint-private",
	] {
		assert!(!serialized.contains(sensitive), "sanitized value leaked {sensitive}");
	}
}

pub(super) fn autonomy_objective_fixture() -> AutonomyObjectiveContract {
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

pub(super) fn seed_autonomy_mcp_state(state_store: &StateStore) -> String {
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
		observed_counts: std::collections::BTreeMap::new(),
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
				created_at: String::from("2026-06-23T00:02:00Z"),
			},
			&[signal_id],
		)
		.expect("proposal should compile");
	let proposal_id = proposal.id().to_owned();

	state_store.record_autonomy_proposal("decodex", proposal).expect("proposal should persist");

	proposal_id
}

pub(super) fn http_handler(
	repo_root: &Path,
	capability_profile: McpCapabilityProfile,
) -> McpHttpHandler {
	http_handler_with_allowed_origins(repo_root, capability_profile, Vec::new())
}

pub(super) fn http_handler_with_authorization(
	repo_root: &Path,
	capability_profile: McpCapabilityProfile,
	authorization: McpHttpAuthorization,
) -> McpHttpHandler {
	let context = McpContext {
		repo_root: repo_root.to_path_buf(),
		config_path: None,
		project_id: None,
		state_store: None,
	};

	http_handler_with_context_and_authorization(
		context,
		capability_profile,
		Vec::new(),
		authorization,
	)
}

pub(super) fn http_handler_with_allowed_origins(
	repo_root: &Path,
	capability_profile: McpCapabilityProfile,
	allowed_origins: Vec<String>,
) -> McpHttpHandler {
	let context = McpContext {
		repo_root: repo_root.to_path_buf(),
		config_path: None,
		project_id: None,
		state_store: None,
	};

	http_handler_with_context(context, capability_profile, allowed_origins)
}

pub(super) fn http_handler_with_context(
	context: McpContext,
	capability_profile: McpCapabilityProfile,
	allowed_origins: Vec<String>,
) -> McpHttpHandler {
	http_handler_with_context_and_authorization(
		context,
		capability_profile,
		allowed_origins,
		McpHttpAuthorization::disabled(),
	)
}

pub(super) fn http_handler_with_context_and_authorization(
	context: McpContext,
	capability_profile: McpCapabilityProfile,
	allowed_origins: Vec<String>,
	authorization: McpHttpAuthorization,
) -> McpHttpHandler {
	McpHttpHandler {
		server: McpServer { context, capability_profile, transport: McpTransport::StreamableHttp },
		sessions: McpHttpSessions::default(),
		allowed_origins,
		listen_address: Some(String::from(DEFAULT_MCP_HTTP_LISTEN_ADDRESS)),
		authorization,
	}
}

pub(super) fn run_http(handler: &mut McpHttpHandler, request: Vec<u8>) -> ParsedHttpResponse {
	let response =
		handler.handle_request_bytes(&request).expect("HTTP handler should return response");

	ParsedHttpResponse::parse(&response)
}

pub(super) fn http_json_rpc(handler: &mut McpHttpHandler, session_id: &str, body: &str) -> Value {
	let response = run_http(
		handler,
		http_post(
			"/mcp",
			[("Origin", "http://127.0.0.1:8193"), ("Mcp-Session-Id", session_id)],
			body,
		),
	);

	assert_eq!(response.status, "HTTP/1.1 200 OK");
	assert_eq!(response.header("content-type"), Some("application/json"));
	assert_eq!(response.header("access-control-allow-origin"), Some("http://127.0.0.1:8193"));

	response.json_body()
}

pub(super) fn http_resource_read_json(
	handler: &mut McpHttpHandler,
	session_id: &str,
	id: u64,
	uri: &str,
) -> Value {
	let request = serde_json::json!({
		"jsonrpc": "2.0",
		"id": id,
		"method": "resources/read",
		"params": {
			"uri": uri
		}
	})
	.to_string();
	let response = http_json_rpc(handler, session_id, &request);
	let contents = response["result"]["contents"].as_array().expect("resource contents array");
	let text = contents[0]["text"].as_str().expect("resource text should exist");

	serde_json::from_str(text).expect("resource text should be JSON")
}

pub(super) fn http_post<'a>(
	path: &str,
	headers: impl IntoIterator<Item = (&'a str, &'a str)>,
	body: &str,
) -> Vec<u8> {
	let mut request = format!(
		"POST {path} HTTP/1.1\r\nHost: 127.0.0.1:8193\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
		body.len()
	);

	for (name, value) in headers {
		request.push_str(name);
		request.push_str(": ");
		request.push_str(value);
		request.push_str("\r\n");
	}

	request.push_str("\r\n");
	request.push_str(body);

	request.into_bytes()
}

pub(super) fn http_delete<'a>(
	path: &str,
	headers: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<u8> {
	let mut request =
		format!("DELETE {path} HTTP/1.1\r\nHost: 127.0.0.1:8193\r\nContent-Length: 0\r\n");

	for (name, value) in headers {
		request.push_str(name);
		request.push_str(": ");
		request.push_str(value);
		request.push_str("\r\n");
	}

	request.push_str("\r\n");

	request.into_bytes()
}

pub(super) fn http_options<'a>(
	path: &str,
	headers: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<u8> {
	let mut request =
		format!("OPTIONS {path} HTTP/1.1\r\nHost: 127.0.0.1:8193\r\nContent-Length: 0\r\n");

	for (name, value) in headers {
		request.push_str(name);
		request.push_str(": ");
		request.push_str(value);
		request.push_str("\r\n");
	}

	request.push_str("\r\n");

	request.into_bytes()
}

pub(super) fn test_repo() -> TempDir {
	let repo = TempDir::new().expect("temp repo should exist");

	write_file(repo.path().join("Cargo.toml"), "[workspace]\n");
	write_file(repo.path().join("docs/index.md"), "# Docs\n");
	write_file(repo.path().join("docs/policy.md"), "# Policy\n");
	write_file(repo.path().join("docs/spec/runtime.md"), "# Runtime\n\nSpec body.\n");
	write_file(repo.path().join("docs/decisions/mcp-gateway.md"), "# MCP\n");
	write_file(repo.path().join("docs/research/sample-report.md"), "# Sample Research\n");

	repo
}

pub(super) fn isolated_mcp_runtime_home(repo: &TempDir) -> TestEnvVarGuard {
	let runtime_home = repo.path().join("operator-home");
	let runtime_home = runtime_home.to_string_lossy().into_owned();

	TestEnvVarGuard::set_many([
		("CODEX_HOME".to_owned(), runtime_home.clone()),
		("HOME".to_owned(), runtime_home),
	])
}

#[test]
fn mcp_project_fixture_runtime_store_stays_under_isolated_home() {
	let operator_runtime_db =
		runtime::runtime_db_path().expect("operator runtime path should resolve");
	let repo = test_repo();
	let _runtime_home_guard = isolated_mcp_runtime_home(&repo);
	let config_path = repo.path().join("project.toml");

	seed_project_runtime_for_mcp_resources(repo.path(), &config_path);
	run_stdio_with_context(
		project_mcp_context(repo.path(), &config_path),
		r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decodex_lane_control","arguments":{"action":"steer","projectId":"pubfi","issue":"PUB-012","runId":"run-12","expectedTurnId":"turn-12","message":"Please stop after the current safe point.","authority":{"reason":"operator requested steer","source":"mcp-test","inspectedRunId":"run-12","expectedTurnId":"turn-12"}}}}"#,
	);

	let fixture_runtime_db =
		runtime::runtime_db_path().expect("fixture runtime path should resolve");
	let state_store = runtime::open_runtime_store().expect("fixture runtime store should open");
	let events = state_store
		.list_private_execution_events("pubfi", "PUB-012", "run-12", 1)
		.expect("fixture private evidence should read");

	assert_ne!(fixture_runtime_db, operator_runtime_db);
	assert!(fixture_runtime_db.starts_with(repo.path()));
	assert!(!events.is_empty());
	assert!(
		events
			.iter()
			.all(|event| event.payload().get("source").and_then(Value::as_str) == Some("mcp-test")),
		"mcp fixture private evidence should stay in isolated runtime store"
	);
}

pub(super) fn seed_project_runtime_for_mcp_resources(repo_root: &Path, config_path: &Path) {
	let state_store = runtime::open_runtime_store().expect("runtime store should open");

	write_project_config(config_path, repo_root);
	write_project_workflow(repo_root);

	runtime::register_project_config(&state_store, config_path, true)
		.expect("project should register");

	for index in 1..=12 {
		let issue_id = format!("PUB-{index:03}");
		let run_id = format!("run-{index:02}");
		let worktree_path = repo_root.join(format!("worktrees/{issue_id}"));
		let attempt_status = if index == 12 { "running" } else { "succeeded" };

		state_store
			.upsert_worktree(
				"pubfi",
				&issue_id,
				&format!("x/pubfi-{index:03}"),
				&worktree_path.display().to_string(),
			)
			.expect("worktree should record");
		state_store
			.record_run_attempt(&run_id, &issue_id, 1, attempt_status)
			.expect("run attempt should record");
		state_store
			.append_event(&run_id, 1, "turn/completed", r#"{"status":"completed"}"#)
			.expect("event should record");

		if index == 12 {
			seed_mcp_lane_runtime_markers(&state_store, &worktree_path, &run_id);
			seed_mcp_lane_runtime_activity(&state_store, &run_id);
		}
	}
}

pub(super) fn seed_mcp_test_private_control_evidence() {
	let state_store = runtime::open_runtime_store().expect("runtime store should open");

	state_store
		.append_private_execution_event(
			"pubfi",
			"PUB-012",
			"run-12",
			1,
			"control_action",
			serde_json::json!({
				"schema": "decodex.run_control_action/v1",
				"source": "mcp-test",
				"action": "project_control_fixture"
			}),
		)
		.expect("mcp-test private evidence should record");
}

pub(super) fn seed_mcp_lane_runtime_activity(state_store: &StateStore, run_id: &str) {
	state_store
		.append_event(
			run_id,
			2,
			"configWarning",
			r#"{"summary":"config at /private/worktree using GITHUB_PAT_Y"}"#,
		)
		.expect("warning event should record");
	state_store
		.append_event(
			run_id,
			3,
			"error",
			r#"{"error":{"codexErrorInfo":"failed under /Users/x/worktree with LINEAR_API_KEY_HACKINK"}}"#,
		)
		.expect("error event should record");
	state_store
		.append_event(
			run_id,
			4,
			"configWarning",
			r#"{"summary":"state marker under /srv/decodex/runtime"}"#,
		)
		.expect("generic path warning event should record");
	state_store
				.append_event(
					run_id,
					5,
					"error",
					r#"{"error":{"codexErrorInfo":"upstream auth failed for ghp_abcdefghijklmnopqrstuvwxyz123456"}}"#,
				)
				.expect("token-shaped error event should record");
	state_store
		.append_event(
			run_id,
			6,
			"error",
			r#"{"error":{"codexErrorInfo":"upstream auth failed for 8Nf4Qz7Lb2Rc9Vx5Tm3Pq6Wy1Hs8Ka0U"}}"#,
		)
		.expect("bare token-shaped error event should record");

	let protocol_activity = ProtocolActivitySummary {
		turn_status: Some(String::from("completed")),
		waiting_reason: Some(String::from("turn_completed")),
		rate_limit_status: None,
		recent_events: vec![
			ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("config at /private/worktree using GITHUB_PAT_Y")),
			},
			ProtocolActivityEventSummary {
				event_type: String::from("error"),
				category: String::from("protocol_error"),
				detail: Some(String::from(
					"failed under /Users/x/worktree with LINEAR_API_KEY_HACKINK",
				)),
			},
			ProtocolActivityEventSummary {
				event_type: String::from("configWarning"),
				category: String::from("warning"),
				detail: Some(String::from("state marker under /srv/decodex/runtime")),
			},
			ProtocolActivityEventSummary {
				event_type: String::from("error"),
				category: String::from("protocol_error"),
				detail: Some(String::from(
					"upstream auth failed for ghp_abcdefghijklmnopqrstuvwxyz123456",
				)),
			},
			ProtocolActivityEventSummary {
				event_type: String::from("error"),
				category: String::from("protocol_error"),
				detail: Some(String::from(
					"upstream auth failed for 8Nf4Qz7Lb2Rc9Vx5Tm3Pq6Wy1Hs8Ka0U",
				)),
			},
		],
	};

	state_store
		.record_run_activity_summary(run_id, 1, None, Some(&protocol_activity))
		.expect("activity summary should record");
}

pub(super) fn seed_mcp_lane_runtime_markers(
	state_store: &StateStore,
	worktree_path: &Path,
	run_id: &str,
) {
	fs::create_dir_all(worktree_path).expect("worktree path should exist");

	let control_dir = worktree_path.join(".decodex-run-control");
	let channel_path = control_dir.join("run-12-1.channel");

	fs::create_dir_all(&control_dir).expect("run-control channel dir should exist");
	fs::write(&channel_path, "ready\n").expect("run-control channel should write");

	state_store
		.upsert_lease("pubfi", "PUB-012", run_id, "In Progress")
		.expect("lease should record");
	state_store.update_run_thread(run_id, "thread-12").expect("thread should record");
	state_store.update_run_turn(run_id, "turn-12").expect("turn should record");
	state_store
		.publish_run_control_channel_for_active_attempt(run_id, 1, &channel_path, "local_file")
		.expect("control channel should publish")
		.expect("active control channel should exist");

	state::write_run_activity_marker_for_process(worktree_path, run_id, 1, process::id())
		.expect("activity marker should record process");
	state::write_run_thread_marker(worktree_path, run_id, 1, "thread-12")
		.expect("thread marker should record");
	state::write_run_turn_marker(worktree_path, run_id, 1, "turn-12")
		.expect("turn marker should record");
}

pub(super) fn write_project_config(config_path: &Path, repo_root: &Path) {
	write_file(
		config_path.to_path_buf(),
		&format!(
			r#"
service_id = "pubfi"

[tracker]
api_key_env_var = "HOME"

[github]
token_env_var = "PATH"

[paths]
repo_root = "{}"
"#,
			repo_root.display()
		),
	);
}

pub(super) fn write_project_workflow(repo_root: &Path) {
	write_file(
		repo_root.join("WORKFLOW.md"),
		r#"
+++
version = 1
max_turns = 1

[tracker]
queued_state = "Todo"
in_progress_state = "In Progress"
success_state = "Done"
terminal_states = ["Done", "Canceled"]

[tools]
comment = "issue_comment"
transition = "issue_transition"
label = "issue_label"
progress_checkpoint = "issue_progress_checkpoint"
review_checkpoint = "issue_review_checkpoint"
review_handoff = "issue_review_handoff"
terminal_finalize = "issue_terminal_finalize"
+++
"#,
	);
}

pub(super) fn write_decodex_project_config(config_path: &Path, repo_root: &Path) {
	write_file(
		config_path.to_path_buf(),
		&format!(
			r#"
service_id = "decodex"

[tracker]
api_key_env_var = "HOME"

[github]
token_env_var = "PATH"

[codex]
review = "standard"

[paths]
repo_root = "{}"
worktree_root = ".worktrees"
"#,
			repo_root.display()
		),
	);
}

pub(super) fn write_decodex_workflow(repo_root: &Path) {
	write_file(
		repo_root.join("WORKFLOW.md"),
		r#"+++
version = 1

[tracker]
provider = "linear"
startable_states = ["Todo"]
terminal_states = ["Done", "Canceled", "Duplicate"]
in_progress_state = "In Progress"
success_state = "In Review"
completed_state = "Done"
failure_state = "Todo"
opt_out_label = "decodex:manual-only"
needs_attention_label = "decodex:needs-attention"

[agent]
transport = "stdio://"

[execution]
max_attempts = 3
max_turns = 3
max_retry_backoff_ms = 300000
gate_profiles = {}
canonicalize_commands = ["cargo make fmt"]
verify_commands = ["cargo make test"]

[execution.workspace_hooks]
after_create_commands = []
before_remove_commands = []
timeout_seconds = 60

[context]
read_first = []
+++
"#,
	);
}

pub(super) fn accepted_mcp_goal_contract() -> DecisionContract {
	let mut contract: DecisionContract = serde_json::from_value(serde_json::json!({
		"schema": crate::loop_contract::DECISION_CONTRACT_SCHEMA,
		"record_version": crate::loop_contract::DECISION_CONTRACT_RECORD_VERSION,
		"contract_id": "mcp-goal-contract",
		"status": "draft_latent",
		"source_intent": {
			"summary": "Expose MCP planning tools.",
			"user_utterance": "arrange MCP planning tools",
			"source_issue_identifier": "XY-852"
		},
		"research_provenance": [
			{
				"kind": "spec",
				"reference": "docs/spec/runtime.md",
				"summary": "MCP planning tools are schema-bound."
			}
		],
		"research_evidence": [
			{
				"claim": "Goal intake can preview generated issue briefs.",
				"support": "Program Intake dry-run renders public-safe issue plans.",
				"source_ref": "docs/spec/loop-runtime.md"
			}
		],
		"research_options": [
			{
				"option": "Expose a small schema-bound planning facade.",
				"status": "selected",
				"tradeoffs": ["Keeps internal graph mechanics out of tool output."]
			}
		],
		"accepted_authority": {
			"accepted_objectives": ["Expose schema-bound MCP planning tools."],
			"non_goals": ["Do not expose raw Program graph mutation."],
			"constraints": ["Dry-run must not persist Program Intake rows."],
			"assumptions": ["The promoted contract owns issue shaping."],
			"objections": ["Apply must require explicit authority."],
			"stop_conditions": ["Stop when authority is missing."]
		},
		"execution_readiness": {
			"summary": "Ready for issue shaping.",
			"ready_for_issue_shaping": true,
			"missing_decisions": [],
			"validation_expectations": ["MCP intake dry-run returns public-safe issue rows."],
			"risk_notes": ["Do not expose internal Program node ids."],
			"proposed_issues": [
				{
					"key": "mcp-planning-tools",
					"title": "Expose schema-bound MCP planning tools.",
					"objective": "Expose schema-bound MCP planning tools.",
					"stage": "runtime",
					"dependencies": [],
					"conflict_domains": ["module:decodex-research-intake-tools"],
					"acceptance": ["Planning tools are listed through tools/list."],
					"validation": ["cargo test -p decodex mcp::tests -- --nocapture"],
					"risk": ["Do not expose internal graph mechanics."],
					"queue_intent": "ready_to_queue"
				}
			],
			"conflict_domains": ["module:decodex-research-intake-tools"]
		},
		"links": {
			"generated_issue_ids": [],
			"generated_issue_identifiers": [],
			"execution_program_node_ids": []
		},
		"evidence_boundary": {
			"private_evidence_refs": [],
			"public_projection_refs": [],
			"public_summary": "MCP planning tools are ready for issue shaping."
		}
	}))
	.expect("contract should deserialize");

	contract
		.promote(
			DecisionPromotion::new(
				"operator",
				DecisionPromotionActorKind::User,
				"2026-06-18T00:00:00Z",
				"test",
				Some(String::from("Accepted for MCP intake dry-run testing.")),
			)
			.expect("promotion should build"),
		)
		.expect("contract should promote");

	contract
}

pub(super) fn write_file(path: std::path::PathBuf, contents: &str) {
	let parent = path.parent().expect("test path should have parent");

	fs::create_dir_all(parent).expect("parent directory should exist");
	fs::write(path, contents).expect("test file should write");
}

pub(super) fn latent_decision_contract_fixture() -> DecisionContract {
	serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/decision_contract/research_x_latent_contract.json"
	)))
	.expect("research X latent contract fixture should deserialize")
}
