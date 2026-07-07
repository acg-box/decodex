mod accepted_contracts_require_readiness_without_missing_decisions;
mod failed_non_promotion_transitions_leave_contract_unchanged;
mod generated_execution_links_are_normalized_and_validated;
mod latent_contracts_reject_promotion_metadata;
mod latent_research_contract_fixture_serializes_with_expected_boundary;
mod promotion_records_acceptance_metadata_and_blocks_double_promotion;
mod proposed_issue_dependencies_must_form_known_acyclic_dag;
mod rejected_contract_cannot_be_promoted;
mod removed_flat_proposed_issue_summaries_are_rejected;
mod validation_rejects_empty_optional_boundary_values;

use crate::loop_contract::{DecisionContract, DecisionPromotion, DecisionPromotionActorKind};

fn latent_research_contract_fixture() -> DecisionContract {
	serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/fixtures/decision_contract/decision_x_latent_contract.json"
	)))
	.expect("decision X latent contract fixture should deserialize")
}

fn sample_promotion() -> DecisionPromotion {
	DecisionPromotion {
		accepted_by: String::from("operator"),
		accepted_by_kind: DecisionPromotionActorKind::User,
		accepted_at: String::from("2026-06-09T10:00:00Z"),
		acceptance_source: String::from("conversation"),
		promotion_reason: Some(String::from("User asked to push this forward.")),
	}
}
