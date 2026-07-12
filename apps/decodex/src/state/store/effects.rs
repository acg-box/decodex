use crate::{
	lane_authority::{EffectCommand, LaneEffect, apply_effect_command},
	prelude::{Result, eyre},
	state::StateStore,
};

impl StateStore {
	pub fn plan_lane_effect(&self, effect: LaneEffect) -> Result<LaneEffect> {
		effect.validate()?;
		let mut state = self.lock_without_refresh()?;
		validate_effect_lane(&state, &effect)?;
		if let Some(existing) = state.lane_effects.get(effect.effect_id()) {
			if existing.has_same_plan_identity(&effect) {
				return Ok(existing.clone());
			}
			eyre::bail!("Immutable lane effect identity cannot be replaced.");
		}
		if state.lane_effects.values().any(|existing| {
			existing.operation_id() == effect.operation_id()
				&& existing.ordinal() == effect.ordinal()
		}) {
			eyre::bail!("Lane effect operation ordinal is already occupied.");
		}
		if let Some(sqlite) = self.sqlite.as_ref() {
			sqlite
				.lock()
				.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?
				.insert_lane_effect(&effect)?;
		}
		state.lane_effects.insert(effect.effect_id().to_owned(), effect.clone());
		Ok(effect)
	}

	pub fn apply_lane_effect_command(
		&self,
		effect_id: &str,
		expected_journal_epoch: u64,
		command: EffectCommand,
	) -> Result<LaneEffect> {
		let mut state = self.lock_without_refresh()?;
		let current = state
			.lane_effects
			.get(effect_id)
			.cloned()
			.ok_or_else(|| eyre::eyre!("Lane effect does not exist."))?;
		validate_effect_lane(&state, &current)?;
		let next = apply_effect_command(&current, expected_journal_epoch, command)
			.map_err(|rejection| eyre::eyre!("Lane effect transition rejected: {rejection:?}"))?;
		if next == current {
			return Ok(next);
		}
		if let Some(sqlite) = self.sqlite.as_ref() {
			sqlite
				.lock()
				.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?
				.cas_lane_effect(expected_journal_epoch, &next)?;
		}
		state.lane_effects.insert(effect_id.to_owned(), next.clone());
		Ok(next)
	}

	pub fn lane_effect(&self, effect_id: &str) -> Result<Option<LaneEffect>> {
		Ok(self.lock()?.lane_effects.get(effect_id).cloned())
	}
}

fn validate_effect_lane(state: &crate::state::StateData, effect: &LaneEffect) -> Result<()> {
	let lane = state
		.lanes
		.get(effect.lane_id())
		.ok_or_else(|| eyre::eyre!("Lane effect references an unknown canonical lane."))?;
	if lane.binding_fingerprint() != effect.binding_fingerprint()
		|| lane.claim_run_id() != Some(effect.claim_run_id())
		|| lane.epoch() != effect.expected_lane_epoch()
	{
		eyre::bail!("Lane effect prerequisites drifted before journal mutation.");
	}
	Ok(())
}
