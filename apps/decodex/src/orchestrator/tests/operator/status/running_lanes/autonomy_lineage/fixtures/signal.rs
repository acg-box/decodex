use std::collections::BTreeMap;

use crate::{
	autonomy_signal::{
		AutonomySignal, AutonomySignalConfidence, AutonomySignalEvidenceClass,
		AutonomySignalFreshness, AutonomySignalInput, AutonomySignalPrivacy,
		AutonomySignalSourceType,
	},
	orchestrator::tests::operator::status::running_lanes::autonomy_lineage::fixtures::{
		AUTONOMY_RUN_ID, OBJECTIVE_ID, SERVICE_ID, proposal,
	},
	state::StateStore,
	tracker::TrackerIssue,
};

pub(super) fn record_autonomy_signal(state_store: &StateStore, issue: &TrackerIssue) -> String {
	let signal = AutonomySignal::runtime_health(AutonomySignalInput {
		project_id: SERVICE_ID.to_owned(),
		objective_id: String::from(OBJECTIVE_ID),
		objective_version: 1,
		source_type: AutonomySignalSourceType::Report,
		source_refs: vec![String::from("status:operator:run-autonomy")],
		primary_source_refs: vec![String::from("apps/decodex/src/loop_contract.rs")],
		issue_id: Some(issue.id.clone()),
		run_id: Some(String::from(AUTONOMY_RUN_ID)),
		attempt_id: Some(String::from("1")),
		head_sha: Some(String::from("0123456789abcdef0123456789abcdef01234567")),
		captured_at: String::from("2026-06-23T00:01:00Z"),
		freshness: AutonomySignalFreshness::Fresh,
		summary: String::from("Operator status needs autonomy lineage readback."),
		evidence: vec![String::from("Status readback carried source refs.")],
		evidence_class: AutonomySignalEvidenceClass::LiveReadback,
		contradictions: Vec::new(),
		gaps: Vec::new(),
		confidence: AutonomySignalConfidence::High,
		privacy: AutonomySignalPrivacy::Public,
		observed_counts: BTreeMap::new(),
		review_evidence: None,
		proposal_only: true,
		created_at: String::from("2026-06-23T00:01:00Z"),
	})
	.expect("signal should build");
	let signal_id = signal.id().to_owned();

	state_store.record_autonomy_signal(SERVICE_ID, signal).expect("signal should persist");

	signal_id
}

pub(super) fn record_sensitive_autonomy_readback_fixture(
	state_store: &StateStore,
	issue: &TrackerIssue,
) {
	let signal = AutonomySignal::runtime_health(AutonomySignalInput {
		project_id: SERVICE_ID.to_owned(),
		objective_id: String::from(OBJECTIVE_ID),
		objective_version: 1,
		source_type: AutonomySignalSourceType::Report,
		source_refs: vec![String::from("status:operator:sensitive-readback")],
		primary_source_refs: vec![String::from("apps/decodex/src/loop_contract.rs")],
		issue_id: Some(issue.id.clone()),
		run_id: Some(String::from(AUTONOMY_RUN_ID)),
		attempt_id: Some(String::from("1")),
		head_sha: Some(String::from("0123456789abcdef0123456789abcdef01234567")),
		captured_at: String::from("2026-06-23T00:04:00Z"),
		freshness: AutonomySignalFreshness::Fresh,
		summary: String::from("Operator status must redact raw autonomy gaps."),
		evidence: vec![String::from("Sensitive fixture stays out of public readback.")],
		evidence_class: AutonomySignalEvidenceClass::LiveReadback,
		contradictions: vec![String::from(
			"Local contradiction references /Users/x/.codex/private-evidence.json",
		)],
		gaps: vec![String::from(
			"Local gap references GITHUB_PAT_Y from the developer environment.",
		)],
		confidence: AutonomySignalConfidence::Medium,
		privacy: AutonomySignalPrivacy::Public,
		observed_counts: BTreeMap::new(),
		review_evidence: None,
		proposal_only: true,
		created_at: String::from("2026-06-23T00:04:00Z"),
	})
	.expect("sensitive signal fixture should build");
	let signal_id = signal.id().to_owned();

	state_store
		.record_autonomy_signal(SERVICE_ID, signal)
		.expect("sensitive signal should persist");

	let sensitive_proposal = state_store
		.compile_autonomy_proposal_dry_run(
			proposal::autonomy_proposal_input(
				"apps/decodex/src/orchestrator/status.rs",
				&issue.identifier,
			),
			&[signal_id],
		)
		.expect("sensitive proposal should compile");

	state_store
		.record_autonomy_proposal(SERVICE_ID, sensitive_proposal)
		.expect("sensitive proposal should persist");
}
