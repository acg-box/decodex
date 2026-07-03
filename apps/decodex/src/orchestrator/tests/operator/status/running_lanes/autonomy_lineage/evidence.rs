use std::collections::BTreeSet;

use crate::{
	orchestrator,
	orchestrator::tests::operator::status::running_lanes::{
		self,
		autonomy_lineage::{
			assertions,
			fixtures::{self, ReplayEvidenceSeed, SERVICE_ID},
		},
	},
	state::{ReviewHandoffMarker, StateStore},
};

#[test]
fn autonomy_lineage_does_not_use_unlinked_review_lifecycle_as_pr_evidence() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let seeded = fixtures::seed_autonomy_lineage_without_execution_evidence(
		&state_store,
		&config,
		&workflow,
		&issue,
	);
	let (generated_issue_id, generated_issue_identifier) =
		fixtures::generated_issue_link(&state_store, &seeded.decision_contract_id);
	let stale_review_marker = ReviewHandoffMarker::new(
		"stale-review-run",
		1,
		"y/decodex-stale-review",
		"https://github.com/hack-ink/decodex/pull/stale",
		"main",
		"y/decodex-stale-review",
		"abcdefabcdefabcdefabcdefabcdefabcdefabcd",
	);

	state_store
		.upsert_review_handoff_marker(SERVICE_ID, &generated_issue_id, &stale_review_marker)
		.expect("stale review marker should persist");

	for (kind, source_ref, summary) in [
		(
			"validation",
			"validation:cargo-make-check:passed",
			"Repo validation gate passed before review handoff.",
		),
		(
			"post_land",
			"post_land:decodex-land:merge-install-restart-audit",
			"Post-land evidence was recorded after normal lifecycle authority.",
		),
	] {
		fixtures::record_replay_evidence_event(
			&state_store,
			&generated_issue_id,
			ReplayEvidenceSeed {
				proposal_id: &seeded.accepted_proposal_id,
				decision_contract_id: &seeded.decision_contract_id,
				run_id: "run-dogfood-review",
				kind,
				source_ref,
				summary,
				pr_head_ref: None,
				pr_head_oid: None,
			},
		);
	}

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let lineage = assertions::autonomy_lineage_for_seed(&snapshot, &seeded);
	let evidence_kinds = lineage
		.execution_evidence
		.iter()
		.map(|evidence| evidence.kind.as_str())
		.collect::<BTreeSet<_>>();

	assert_eq!(lineage.program_intake[0].intake_kind, "goal_intake");
	assert_eq!(lineage.completeness, "partial");
	assert!(lineage.known_gaps.contains(&String::from("pr_evidence_missing")));
	assert_eq!(evidence_kinds, BTreeSet::from(["post_land", "validation"]));
	assert!(lineage.execution_evidence.iter().all(|evidence| evidence.issue_identifier.as_deref()
		== Some(generated_issue_identifier.as_str())));
	assert!(!lineage.execution_evidence.iter().any(|evidence| {
		evidence.source_refs.iter().any(|source_ref| source_ref.contains("stale"))
	}));
}

#[test]
fn autonomy_lineage_marks_same_pr_stale_head_lifecycle_as_partial() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let seeded = fixtures::seed_autonomy_lineage_without_execution_evidence(
		&state_store,
		&config,
		&workflow,
		&issue,
	);
	let (generated_issue_id, _generated_issue_identifier) =
		fixtures::generated_issue_link(&state_store, &seeded.decision_contract_id);
	let stale_head_oid = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
	let fresh_head_oid = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
	let stale_review_marker = ReviewHandoffMarker::new(
		"run-dogfood-review",
		1,
		"y/decodex-xy-1091",
		"https://github.com/hack-ink/decodex/pull/1091",
		"main",
		"y/decodex-xy-1091",
		stale_head_oid,
	);

	state_store
		.upsert_review_handoff_marker(SERVICE_ID, &generated_issue_id, &stale_review_marker)
		.expect("stale review marker should persist");

	fixtures::record_replay_evidence_event(
		&state_store,
		&generated_issue_id,
		ReplayEvidenceSeed {
			proposal_id: &seeded.accepted_proposal_id,
			decision_contract_id: &seeded.decision_contract_id,
			run_id: "run-dogfood-review",
			kind: "pr",
			source_ref: "https://github.com/hack-ink/decodex/pull/1091",
			summary: "PR-backed review handoff readback recorded.",
			pr_head_ref: Some("y/decodex-xy-1091"),
			pr_head_oid: Some(fresh_head_oid),
		},
	);

	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");
	let lineage = assertions::autonomy_lineage_for_seed(&snapshot, &seeded);
	let pr_evidence = lineage
		.execution_evidence
		.iter()
		.find(|evidence| evidence.kind == "pr")
		.expect("partial PR replay evidence should render");

	assert_eq!(lineage.completeness, "partial");
	assert_eq!(pr_evidence.completeness, "partial");
	assert!(pr_evidence.known_gaps.contains(&String::from("review_lifecycle_stale_or_mismatched")));
	assert!(lineage.known_gaps.contains(&String::from("review_lifecycle_stale_or_mismatched")));
}
