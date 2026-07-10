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
				r#"{"jsonrpc":"2.0","id":5,"method":"resources/read","params":{"uri":"decodex://projects/decodex/autonomy/proposals/affected/bridge_proposal_fingerprint/fixture"}}"#,
				r#"{"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":"decodex://projects/decodex/autonomy/evidence"}}"#,
			]
			.join("\n"),
		);
	let summary = support::resource_response_json(&responses, 0);
	let objective = support::resource_response_json(&responses, 1);
	let signal = support::resource_response_json(&responses, 2);
	let proposal = support::resource_response_json(&responses, 3);
	let proposal_by_affected_identifier = support::resource_response_json(&responses, 4);
	let evidence = support::resource_response_json(&responses, 5);
	let combined = serde_json::json!({
		"summary": summary,
		"objective": objective,
		"signal": signal,
		"proposal": proposal,
		"proposal_by_affected_identifier": proposal_by_affected_identifier,
		"evidence": evidence
	});
	let serialized = serde_json::to_string(&combined).expect("resources should serialize");

	assert_eq!(combined["summary"]["schema"], "decodex.mcp.autonomy_summary/1");
	assert_eq!(combined["objective"]["objective"]["state"], "accepted");
	assert_eq!(combined["signal"]["signal"]["kind"], "runtime_health");
	assert_eq!(combined["proposal"]["proposal"]["state"], "decision_candidate");
	assert_eq!(combined["proposal_by_affected_identifier"]["proposal"]["proposal_id"], proposal_id);
	assert_eq!(combined["evidence"]["evidence"]["signal_count"], 1);
	assert!(serialized.contains("access_boundary_only"));
	assert!(!serialized.contains("private evidence payload"));
	assert!(!serialized.contains("raw_payload"));

	support::assert_no_sensitive_observability_content(&combined);
}

#[test]
fn autonomy_resources_redact_local_private_signal_refs() {
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
