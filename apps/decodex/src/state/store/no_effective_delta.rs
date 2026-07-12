use crate::{
	lane_authority::{
		NoEffectiveDeltaCommand, NoEffectiveDeltaDecision, NoEffectiveDeltaRecovery,
		decide_no_effective_delta,
	},
	prelude::{Result, eyre},
	state::StateStore,
};

impl StateStore {
	pub fn decide_no_effective_delta(
		&self,
		operation_id: &str,
		command: NoEffectiveDeltaCommand,
	) -> Result<NoEffectiveDeltaDecision> {
		let mut state = self.lock_without_refresh()?;
		let current = state.no_effective_delta_recoveries.get(operation_id).cloned();
		let decision =
			decide_no_effective_delta(current.as_ref(), command).map_err(|rejection| {
				eyre::eyre!("No-effective-delta decision rejected: {rejection:?}")
			})?;
		let NoEffectiveDeltaDecision::Retry(recovery) = &decision else {
			return Ok(decision);
		};
		validate_recovery_lane(&state, recovery)?;
		if current.is_none() {
			if recovery.operation_id() != operation_id {
				eyre::bail!("No-effective-delta operation key does not match the command.");
			}
			if let Some(sqlite) = self.sqlite.as_ref() {
				sqlite
					.lock()
					.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?
					.insert_no_effective_delta_recovery(recovery)?;
			}
			state.no_effective_delta_recoveries.insert(operation_id.to_owned(), recovery.clone());
		}
		Ok(decision)
	}

	pub fn no_effective_delta_recovery(
		&self,
		operation_id: &str,
	) -> Result<Option<NoEffectiveDeltaRecovery>> {
		Ok(self.lock()?.no_effective_delta_recoveries.get(operation_id).cloned())
	}
}

fn validate_recovery_lane(
	state: &crate::state::StateData,
	recovery: &NoEffectiveDeltaRecovery,
) -> Result<()> {
	if !state.lanes.contains_key(recovery.lane_id()) {
		eyre::bail!("No-effective-delta recovery references an unknown canonical lane.");
	}
	Ok(())
}
