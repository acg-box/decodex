use crate::{
	autonomy_proposal::{AutonomyProposal, AutonomyProposalCompileInput},
	prelude::{Result, eyre},
	state::{StateStore, store},
};

impl StateStore {
	/// Compile a non-mutating autonomy proposal dry-run from persisted objective and signal rows.
	pub(crate) fn compile_autonomy_proposal_dry_run(
		&self,
		input: AutonomyProposalCompileInput,
		signal_ids: &[String],
	) -> Result<AutonomyProposal> {
		let objective = self
			.autonomy_objective(&input.project_id, &input.objective_id, input.objective_version)?
			.map(|record| record.objective().clone());
		let mut signals = Vec::new();

		for signal_id in signal_ids {
			store::validate_required_autonomy_proposal_field("signal_id", signal_id)?;

			let signal = self.autonomy_signal(&input.project_id, signal_id)?.ok_or_else(|| {
				eyre::eyre!("Autonomy proposal signal `{signal_id}` does not exist.")
			})?;

			signals.push(signal.signal().clone());
		}

		AutonomyProposal::compile_dry_run(objective.as_ref(), &signals, input)
	}
}
