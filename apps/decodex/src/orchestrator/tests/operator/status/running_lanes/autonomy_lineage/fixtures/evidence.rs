use crate::{
	orchestrator::tests::operator::status::running_lanes::autonomy_lineage::fixtures::{
		ReplayEvidenceSeed, SERVICE_ID,
	},
	state::{ReviewLifecycleHandoffFixture, StateStore},
};

pub(super) struct ExecutionEvidenceSeed<'a> {
	pub(super) proposal_id: &'a str,
	pub(super) decision_contract_id: &'a str,
}

pub(super) fn record_dogfood_execution_evidence(
	state_store: &StateStore,
	seed: ExecutionEvidenceSeed<'_>,
) -> String {
	let (generated_issue_id, generated_issue_identifier) =
		generated_issue_link(state_store, seed.decision_contract_id);
	let review_marker = ReviewLifecycleHandoffFixture::new(
		"run-dogfood-review",
		1,
		"y/decodex-xy-1091",
		"https://github.com/hack-ink/decodex/pull/1091",
		"main",
		"y/decodex-xy-1091",
		"0123456789abcdef0123456789abcdef01234567",
	);

	state_store
		.upsert_review_lifecycle_handoff_fixture(SERVICE_ID, &generated_issue_id, &review_marker)
		.expect("review lifecycle handoff fixture should persist");

	record_replay_evidence_event(
		state_store,
		&generated_issue_id,
		ReplayEvidenceSeed {
			proposal_id: seed.proposal_id,
			decision_contract_id: seed.decision_contract_id,
			run_id: "run-dogfood-review",
			kind: "validation",
			source_ref: "validation:cargo-make-check:passed",
			summary: "Local validation summary referenced GITHUB_PAT_Y before clean replay evidence.",
			pr_head_ref: None,
			pr_head_oid: None,
		},
	);

	for (kind, source_ref, summary) in [
		(
			"pr",
			"https://github.com/hack-ink/decodex/pull/1091",
			"PR-backed review handoff readback recorded.",
		),
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
		record_replay_evidence_event(
			state_store,
			&generated_issue_id,
			ReplayEvidenceSeed {
				proposal_id: seed.proposal_id,
				decision_contract_id: seed.decision_contract_id,
				run_id: "run-dogfood-review",
				kind,
				source_ref,
				summary,
				pr_head_ref: (kind == "pr").then_some("y/decodex-xy-1091"),
				pr_head_oid: (kind == "pr").then_some("0123456789abcdef0123456789abcdef01234567"),
			},
		);
	}

	generated_issue_identifier
}

pub(super) fn generated_issue_link(
	state_store: &StateStore,
	decision_contract_id: &str,
) -> (String, String) {
	let linked = state_store
		.decision_contract(SERVICE_ID, decision_contract_id)
		.expect("decision contract should read")
		.expect("decision contract should exist");

	(
		linked.contract().links().generated_issue_ids()[0].clone(),
		linked.contract().links().generated_issue_identifiers()[0].clone(),
	)
}

pub(super) fn record_replay_evidence_event(
	state_store: &StateStore,
	generated_issue_id: &str,
	seed: ReplayEvidenceSeed<'_>,
) {
	state_store
		.append_private_execution_event(
			SERVICE_ID,
			generated_issue_id,
			seed.run_id,
			1,
			"autonomy/replay_evidence",
			serde_json::json!({
				"schema": "decodex.autonomy_replay_evidence/1",
				"proposal_id": seed.proposal_id,
				"contract_id": seed.decision_contract_id,
				"kind": seed.kind,
				"source_refs": [seed.source_ref],
				"summary": seed.summary,
				"pr_head_ref": seed.pr_head_ref,
				"pr_head_oid": seed.pr_head_oid,
			}),
		)
		.expect("replay evidence should persist");
}
