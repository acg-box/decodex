use serde_json::Value;

use crate::autonomy_proposal::AutonomyProposal;

pub(super) fn autonomy_decision_research_options(proposal: &AutonomyProposal) -> Vec<Value> {
	let mut options = vec![serde_json::json!({
		"option": proposal.summary.clone(),
		"tradeoffs": option_tradeoffs(proposal),
		"decision": "accepted_as_latent_decision_contract",
		"rejected_reason": null,
	})];

	options.extend(proposal.rejected_alternatives.iter().map(|alternative| {
		serde_json::json!({
			"option": alternative.clone(),
			"tradeoffs": [],
			"decision": null,
			"rejected_reason": "Rejected before autonomy proposal acceptance.",
		})
	}));

	options
}

fn option_tradeoffs(proposal: &AutonomyProposal) -> Vec<String> {
	let mut tradeoffs = Vec::new();

	tradeoffs.push(format!("Rollback path: {}", proposal.rollback_path));
	tradeoffs.extend(
		proposal.review_requirements.iter().map(|item| format!("Review requirement: {item}")),
	);
	tradeoffs
		.extend(proposal.validation_gates.iter().map(|item| format!("Validation gate: {item}")));

	tradeoffs
}
