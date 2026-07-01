use rusqlite::{self, Error, Row};

use crate::{
	autonomy_objective::AutonomyObjectiveContract,
	autonomy_proposal::AutonomyProposal,
	autonomy_signal::AutonomySignal,
	prelude::eyre,
	state::{
		AutonomyObjectiveRuntimeRecord, AutonomyObjectiveRuntimeRowParts,
		AutonomyProposalRuntimeRecord, AutonomyProposalRuntimeRowParts,
		AutonomySignalRuntimeRecord, AutonomySignalRuntimeRowParts,
	},
};

pub(in crate::state) fn autonomy_objective_runtime_row_parts(
	row: &Row<'_>,
) -> std::result::Result<AutonomyObjectiveRuntimeRowParts, Error> {
	Ok(AutonomyObjectiveRuntimeRowParts {
		project_id: row.get(0)?,
		objective_id: row.get(1)?,
		version: row.get(2)?,
		state: row.get(3)?,
		payload_json: row.get(4)?,
		created_at: row.get(5)?,
		created_at_unix: row.get(6)?,
		updated_at: row.get(7)?,
		updated_at_unix: row.get(8)?,
	})
}

pub(in crate::state) fn autonomy_objective_record_from_row_parts(
	parts: AutonomyObjectiveRuntimeRowParts,
) -> crate::prelude::Result<AutonomyObjectiveRuntimeRecord> {
	let objective = serde_json::from_str::<AutonomyObjectiveContract>(&parts.payload_json)?;
	let objective_state = objective.state();
	let version = u64::try_from(parts.version)
		.map_err(|_| eyre::eyre!("Autonomy objective row version must be greater than zero."))?;

	objective.validate()?;

	if parts.project_id != objective.project_id() {
		eyre::bail!(
			"Autonomy objective row project `{}` contained payload project `{}`.",
			parts.project_id,
			objective.project_id()
		);
	}
	if parts.objective_id != objective.id() {
		eyre::bail!(
			"Autonomy objective row `{}` contained payload `{}`.",
			parts.objective_id,
			objective.id()
		);
	}
	if version != objective.version() {
		eyre::bail!(
			"Autonomy objective row `{}` version {} contained payload version {}.",
			parts.objective_id,
			version,
			objective.version()
		);
	}
	if parts.state != objective_state.as_str() {
		eyre::bail!(
			"Autonomy objective row `{}` version {} state `{}` differed from payload state `{}`.",
			parts.objective_id,
			version,
			parts.state,
			objective_state.as_str()
		);
	}

	Ok(AutonomyObjectiveRuntimeRecord {
		project_id: parts.project_id,
		state: objective_state,
		objective,
		created_at: parts.created_at,
		created_at_unix: parts.created_at_unix,
		updated_at: parts.updated_at,
		updated_at_unix: parts.updated_at_unix,
	})
}

pub(in crate::state) fn autonomy_signal_runtime_row_parts(
	row: &Row<'_>,
) -> std::result::Result<AutonomySignalRuntimeRowParts, Error> {
	Ok(AutonomySignalRuntimeRowParts {
		project_id: row.get(0)?,
		signal_id: row.get(1)?,
		objective_id: row.get(2)?,
		objective_version: row.get(3)?,
		kind: row.get(4)?,
		fingerprint: row.get(5)?,
		freshness: row.get(6)?,
		evidence_class: row.get(7)?,
		confidence: row.get(8)?,
		privacy: row.get(9)?,
		payload_json: row.get(10)?,
		created_at: row.get(11)?,
		created_at_unix: row.get(12)?,
		updated_at: row.get(13)?,
		updated_at_unix: row.get(14)?,
	})
}

pub(in crate::state) fn autonomy_signal_record_from_row_parts(
	parts: AutonomySignalRuntimeRowParts,
) -> crate::prelude::Result<AutonomySignalRuntimeRecord> {
	let signal = serde_json::from_str::<AutonomySignal>(&parts.payload_json)?;
	let version = u64::try_from(parts.objective_version).map_err(|_| {
		eyre::eyre!("Autonomy signal row objective_version must be greater than zero.")
	})?;

	signal.validate()?;

	if parts.project_id != signal.project_id() {
		eyre::bail!(
			"Autonomy signal row project `{}` contained payload project `{}`.",
			parts.project_id,
			signal.project_id()
		);
	}
	if parts.signal_id != signal.id() {
		eyre::bail!(
			"Autonomy signal row `{}` contained payload `{}`.",
			parts.signal_id,
			signal.id()
		);
	}
	if parts.objective_id != signal.objective_id() {
		eyre::bail!(
			"Autonomy signal row objective `{}` contained payload `{}`.",
			parts.objective_id,
			signal.objective_id()
		);
	}
	if version != signal.objective_version() {
		eyre::bail!(
			"Autonomy signal row `{}` objective version {} contained payload version {}.",
			parts.signal_id,
			version,
			signal.objective_version()
		);
	}
	if parts.kind != signal.kind().as_str()
		|| parts.fingerprint != signal.fingerprint()
		|| parts.freshness != signal.freshness().as_str()
		|| parts.evidence_class != signal.evidence_class().as_str()
		|| parts.confidence != signal.confidence().as_str()
		|| parts.privacy != signal.privacy().as_str()
	{
		eyre::bail!(
			"Autonomy signal row `{}` readback columns differed from payload.",
			parts.signal_id
		);
	}

	Ok(AutonomySignalRuntimeRecord {
		project_id: parts.project_id,
		signal,
		created_at: parts.created_at,
		created_at_unix: parts.created_at_unix,
		updated_at: parts.updated_at,
		updated_at_unix: parts.updated_at_unix,
	})
}

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
