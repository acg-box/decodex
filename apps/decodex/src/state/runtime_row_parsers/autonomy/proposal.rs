use rusqlite::{Error, Row};

use crate::{
	autonomy_proposal::AutonomyProposal,
	prelude::eyre,
	state::{AutonomyProposalRuntimeRecord, AutonomyProposalRuntimeRowParts},
};

pub(in crate::state) fn autonomy_proposal_runtime_row_parts(
	row: &Row<'_>,
) -> std::result::Result<AutonomyProposalRuntimeRowParts, Error> {
	Ok(AutonomyProposalRuntimeRowParts {
		project_id: row.get(0)?,
		proposal_id: row.get(1)?,
		objective_id: row.get(2)?,
		objective_version: row.get(3)?,
		state: row.get(4)?,
		fingerprint: row.get(5)?,
		source_family: row.get(6)?,
		intended_surface: row.get(7)?,
		payload_json: row.get(8)?,
		created_at: row.get(9)?,
		created_at_unix: row.get(10)?,
		updated_at: row.get(11)?,
		updated_at_unix: row.get(12)?,
	})
}

pub(in crate::state) fn autonomy_proposal_record_from_row_parts(
	parts: AutonomyProposalRuntimeRowParts,
) -> crate::prelude::Result<AutonomyProposalRuntimeRecord> {
	let proposal = serde_json::from_str::<AutonomyProposal>(&parts.payload_json)?;
	let version = u64::try_from(parts.objective_version).map_err(|_| {
		eyre::eyre!("Autonomy proposal row objective_version must be greater than zero.")
	})?;

	proposal.validate()?;

	if parts.project_id != proposal.project_id() {
		eyre::bail!(
			"Autonomy proposal row project `{}` contained payload project `{}`.",
			parts.project_id,
			proposal.project_id()
		);
	}
	if parts.proposal_id != proposal.id() {
		eyre::bail!(
			"Autonomy proposal row `{}` contained payload `{}`.",
			parts.proposal_id,
			proposal.id()
		);
	}
	if parts.objective_id != proposal.objective_id() {
		eyre::bail!(
			"Autonomy proposal row objective `{}` contained payload `{}`.",
			parts.objective_id,
			proposal.objective_id()
		);
	}
	if version != proposal.objective_version() {
		eyre::bail!(
			"Autonomy proposal row `{}` objective version {} contained payload version {}.",
			parts.proposal_id,
			version,
			proposal.objective_version()
		);
	}
	if parts.state != proposal.state().as_str()
		|| parts.fingerprint != proposal.fingerprint()
		|| parts.source_family != proposal.source_family()
		|| parts.intended_surface != proposal.intended_surface()
	{
		eyre::bail!(
			"Autonomy proposal row `{}` readback columns differed from payload.",
			parts.proposal_id
		);
	}

	Ok(AutonomyProposalRuntimeRecord {
		project_id: parts.project_id,
		state: proposal.state(),
		proposal,
		created_at: parts.created_at,
		created_at_unix: parts.created_at_unix,
		updated_at: parts.updated_at,
		updated_at_unix: parts.updated_at_unix,
	})
}
