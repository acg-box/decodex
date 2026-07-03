use sha2::{Digest as _, Sha256};

use crate::{
	autonomy_proposal::{
		AUTONOMY_PROPOSAL_RECORD_VERSION, AUTONOMY_PROPOSAL_SCHEMA, AutonomyProposal,
	},
	prelude::Result,
};

pub(super) fn autonomy_proposal_schema() -> String {
	AUTONOMY_PROPOSAL_SCHEMA.to_owned()
}

pub(super) const fn autonomy_proposal_record_version() -> u16 {
	AUTONOMY_PROPOSAL_RECORD_VERSION
}

pub(super) fn autonomy_proposal_id(fingerprint: &str) -> String {
	format!("autonomy_proposal:{fingerprint}")
}

pub(super) fn autonomy_proposal_fingerprint(proposal: &AutonomyProposal) -> Result<String> {
	let material = serde_json::json!({
		"project_id": proposal.project_id,
		"objective_id": proposal.objective_id,
		"objective_version": proposal.objective_version,
		"source_signal_ids": proposal.source_signal_ids,
		"affected_identifiers": proposal.affected_identifiers,
		"source_family": proposal.source_family,
		"intended_surface": proposal.intended_surface,
		"issue_candidates": proposal.issue_candidates,
	});
	let payload = serde_json::to_vec(&material)?;
	let digest = Sha256::digest(payload);
	let mut hash = String::with_capacity(64);

	for byte in digest {
		hash.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
		hash.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
	}

	Ok(hash)
}
