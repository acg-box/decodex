//! Decision Contract authority helpers for execution programs.

use sha2::{Digest, Sha256};

use crate::{
	loop_contract::{DecisionContract, DecisionContractStatus},
	prelude::{Result, eyre},
};

pub(super) fn ensure_accepted_contract(contract: &DecisionContract) -> Result<()> {
	contract.validate()?;

	if contract.status() != DecisionContractStatus::AcceptedPromoted {
		eyre::bail!(
			"Execution Programs can only derive from accepted Decision Contracts; `{}` is `{}`.",
			contract.contract_id(),
			contract.status().as_str()
		);
	}

	Ok(())
}

pub(super) fn decision_contract_fingerprint(contract: &DecisionContract) -> Result<String> {
	contract.validate()?;

	let payload = serde_json::to_vec(contract)?;
	let digest = Sha256::digest(payload);

	Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>())
}

pub(super) fn decision_contract_provenance_reference(
	contract: &DecisionContract,
	kind: &str,
) -> Option<String> {
	contract
		.research_provenance()
		.iter()
		.find(|provenance| provenance.kind() == kind)
		.map(|provenance| provenance.reference().to_owned())
}

pub(super) fn decision_contract_autonomy_signal_refs(contract: &DecisionContract) -> Vec<String> {
	let mut refs = contract
		.research_evidence()
		.iter()
		.filter_map(|evidence| {
			if evidence.kind().starts_with("autonomy_signal:") {
				evidence.source_ref().map(str::to_owned)
			} else {
				None
			}
		})
		.collect::<Vec<_>>();

	refs.sort();
	refs.dedup();

	refs
}
