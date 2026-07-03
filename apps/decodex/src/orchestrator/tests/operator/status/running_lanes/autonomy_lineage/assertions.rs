use std::collections::BTreeSet;

use crate::orchestrator::{
	self, OperatorAutonomyLineageStatus, OperatorStatusSnapshot,
	tests::operator::status::running_lanes::autonomy_lineage::fixtures::{
		OBJECTIVE_ID, SeededAutonomyLineage,
	},
};

pub(super) fn assert_autonomy_readback(
	snapshot: &OperatorStatusSnapshot,
	seeded: &SeededAutonomyLineage,
) {
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let loop_status = run.loop_status.as_ref().expect("loop status should render");
	let objective =
		loop_status.autonomy_objective.as_ref().expect("objective readback should render");
	let lineage = autonomy_lineage_for_seed(snapshot, seeded);
	let clean_signal = loop_status
		.autonomy_signals
		.iter()
		.find(|signal| signal.source_refs.contains(&String::from("status:operator:run-autonomy")))
		.expect("clean autonomy signal should render");
	let sensitive_signal = loop_status
		.autonomy_signals
		.iter()
		.find(|signal| {
			signal.source_refs.contains(&String::from("status:operator:sensitive-readback"))
		})
		.expect("sensitive autonomy signal should render");
	let refused = loop_status
		.autonomy_proposals
		.iter()
		.find(|proposal| proposal.refusal_reasons.contains(&String::from("disallowed_surface")))
		.expect("refused proposal should render");
	let sensitive_proposal = loop_status
		.autonomy_proposals
		.iter()
		.find(|proposal| proposal.gaps.contains(&String::from("redacted_sensitive_detail")))
		.expect("sensitive proposal should render");
	let report = loop_status.autonomy_report.as_ref().expect("report readback should render");
	let rendered = orchestrator::render_operator_status(snapshot);
	let snapshot_json = serde_json::to_string(snapshot).expect("snapshot should serialize");

	assert_eq!(objective.objective_id, OBJECTIVE_ID);
	assert_eq!(objective.objective_version, 1);
	assert_eq!(clean_signal.freshness, "fresh");
	assert_eq!(sensitive_signal.gaps, ["redacted_sensitive_detail"]);
	assert_eq!(sensitive_signal.contradictions, ["redacted_sensitive_detail"]);
	assert!(sensitive_signal.known_gaps.contains(&String::from("gap_or_contradiction_redacted")));
	assert_eq!(lineage.decision_contracts[0].contract_id, seeded.decision_contract_id);
	assert_eq!(lineage.program_intake[0].intake_kind, "goal_intake");
	assert_eq!(lineage.completeness, "complete");
	assert!(lineage.known_gaps.is_empty());

	assert_dogfood_execution_evidence(lineage, seeded);

	assert_eq!(refused.refusals[0].reason, "disallowed_surface");
	assert_eq!(sensitive_proposal.gaps, ["redacted_sensitive_detail"]);
	assert_eq!(sensitive_proposal.contradictions, ["redacted_sensitive_detail"]);
	assert!(
		report
			.known_gaps
			.iter()
			.any(|gap| gap.contains("redacted") || gap == "proposal_public_fields_redacted")
	);
	assert_eq!(report.authority, "derived_query_view");
	assert!(!report.audit_authority);
	assert_eq!(report.completeness, "partial");
	assert!(rendered.contains("loop_autonomy_signals: runtime_health:quality-autonomy@v1"));
	assert!(rendered.contains("sources=1"));
	assert!(rendered.contains("report=derived_query_view"));
	assert!(!snapshot_json.contains("dry_run_record"));
	assert!(!snapshot_json.contains("/Users/x"));
	assert!(!snapshot_json.contains("GITHUB_PAT_Y"));
	assert!(!rendered.contains("/Users/x"));
	assert!(!rendered.contains("GITHUB_PAT_Y"));
}

pub(super) fn autonomy_lineage_for_seed<'a>(
	snapshot: &'a OperatorStatusSnapshot,
	seeded: &SeededAutonomyLineage,
) -> &'a OperatorAutonomyLineageStatus {
	let run = snapshot.current_lanes.first().expect("current lane should exist");
	let loop_status = run.loop_status.as_ref().expect("loop status should render");

	loop_status
		.autonomy_lineage
		.iter()
		.find(|lineage| lineage.proposal_id.as_deref() == Some(&seeded.accepted_proposal_id))
		.expect("accepted proposal lineage should render")
}

fn assert_dogfood_execution_evidence(
	lineage: &OperatorAutonomyLineageStatus,
	seeded: &SeededAutonomyLineage,
) {
	let evidence_kinds = lineage
		.execution_evidence
		.iter()
		.map(|evidence| evidence.kind.as_str())
		.collect::<BTreeSet<_>>();
	let source_refs = lineage
		.execution_evidence
		.iter()
		.flat_map(|evidence| evidence.source_refs.iter().map(String::as_str))
		.collect::<BTreeSet<_>>();

	assert_eq!(evidence_kinds, BTreeSet::from(["post_land", "pr", "validation"]));
	assert!(lineage.execution_evidence.iter().all(|evidence| {
		evidence.issue_identifier.as_deref() == Some(&seeded.generated_issue_identifier)
			&& evidence.completeness == "complete"
			&& evidence.known_gaps.is_empty()
	}));
	assert!(source_refs.contains("https://github.com/hack-ink/decodex/pull/1091"));
	assert!(source_refs.contains("validation:cargo-make-check:passed"));
	assert!(source_refs.contains("post_land:decodex-land:merge-install-restart-audit"));
}
