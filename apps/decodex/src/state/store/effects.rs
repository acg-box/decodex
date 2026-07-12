use std::{os::unix::ffi::OsStrExt, path::Path};

use sha2::{Digest, Sha256};

use crate::{
	lane_authority::{
		EffectAuthority, EffectCommand, EffectReceipt, EffectState, LaneEffect, LaneEffectKind,
		apply_effect_command,
	},
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

	pub fn execute_worktree_remove_effect<F>(
		&self,
		effect_id: &str,
		observed_at: &str,
		observed_at_unix: i64,
		remove: F,
	) -> Result<LaneEffect>
	where
		F: FnOnce(&Path) -> Result<bool>,
	{
		let (mapping, invoking) = {
			let mut state = self.lock_without_refresh()?;
			let current = state
				.lane_effects
				.get(effect_id)
				.cloned()
				.ok_or_else(|| eyre::eyre!("Worktree cleanup effect does not exist."))?;
			if current.kind() != LaneEffectKind::WorktreeRemove {
				eyre::bail!("Effect is not a worktree cleanup effect.");
			}
			validate_effect_lane(&state, &current)?;
			let mapping = state
				.worktrees
				.get(current.lane_id().tracker_issue_id())
				.cloned()
				.ok_or_else(|| eyre::eyre!("Worktree cleanup ownership mapping is missing."))?;
			let lane = state
				.lanes
				.get(current.lane_id())
				.ok_or_else(|| eyre::eyre!("Worktree cleanup canonical lane is missing."))?;
			let facts = worktree_remove_facts_fingerprint(
				&mapping.project_id,
				&mapping.issue_id,
				&mapping.branch_name,
				&mapping.worktree_path,
			);
			if mapping.project_id != current.lane_id().project_key()
				|| lane.branch_name() != Some(mapping.branch_name.as_str())
				|| lane.worktree_path() != Some(&mapping.worktree_path)
				|| current.facts_fingerprint() != facts
			{
				eyre::bail!("Worktree cleanup ownership prerequisites drifted.");
			}
			let invoking = apply_effect_command(
				&current,
				current.journal_epoch(),
				EffectCommand::BeginInvocation {
					authority_epoch: current.authority_epoch(),
					facts_fingerprint: facts,
				},
			)
			.map_err(|rejection| {
				eyre::eyre!("Worktree cleanup invocation rejected: {rejection:?}")
			})?;
			if let Some(sqlite) = self.sqlite.as_ref() {
				sqlite
					.lock()
					.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?
					.cas_lane_effect(current.journal_epoch(), &invoking)?;
			}
			state.lane_effects.insert(effect_id.to_owned(), invoking.clone());
			(mapping, invoking)
		};

		if let Err(error) = remove(&mapping.worktree_path) {
			let _ = self.apply_lane_effect_command(
				effect_id,
				invoking.journal_epoch(),
				EffectCommand::MarkOutcomeUnknown,
			);
			return Err(error);
		}

		let receipt = EffectReceipt::new(
			&format!("worktree-remove:{effect_id}"),
			invoking.request_digest(),
			invoking.facts_fingerprint(),
			None,
			Some(invoking.facts_fingerprint()),
			observed_at,
			observed_at_unix,
		)?;
		let succeeded = apply_effect_command(
			&invoking,
			invoking.journal_epoch(),
			EffectCommand::RecordReceipt { receipt },
		)
		.map_err(|rejection| eyre::eyre!("Worktree cleanup receipt rejected: {rejection:?}"))?;
		let mut state = self.lock_without_refresh()?;
		validate_effect_lane(&state, &invoking)?;
		if let Some(sqlite) = self.sqlite.as_ref() {
			sqlite
				.lock()
				.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?
				.complete_worktree_remove_effect(
					invoking.journal_epoch(),
					&succeeded,
					&mapping.issue_id,
				)?;
		}
		state.worktrees.remove(&mapping.issue_id);
		state.lane_effects.insert(effect_id.to_owned(), succeeded.clone());
		debug_assert_eq!(succeeded.state(), EffectState::Succeeded);
		Ok(succeeded)
	}
}

pub fn worktree_remove_facts_fingerprint(
	project_id: &str,
	issue_id: &str,
	branch_name: &str,
	worktree_path: &Path,
) -> String {
	let mut digest = Sha256::new();
	for value in [project_id.as_bytes(), issue_id.as_bytes(), branch_name.as_bytes()] {
		digest.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
		digest.update(value);
	}
	let path = worktree_path.as_os_str().as_bytes();
	digest.update(u64::try_from(path.len()).unwrap_or(u64::MAX).to_be_bytes());
	digest.update(path);
	format!(
		"sha256:{}",
		digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>()
	)
}

fn validate_effect_lane(state: &crate::state::StateData, effect: &LaneEffect) -> Result<()> {
	let lane = state
		.lanes
		.get(effect.lane_id())
		.ok_or_else(|| eyre::eyre!("Lane effect references an unknown canonical lane."))?;
	if lane.binding_fingerprint() != effect.binding_fingerprint() {
		eyre::bail!("Lane effect prerequisites drifted before journal mutation.");
	}
	match effect.authority() {
		EffectAuthority::LaneClaim { claim_run_id, expected_lane_epoch }
			if lane.claim_run_id() == Some(claim_run_id.as_str())
				&& lane.epoch() == *expected_lane_epoch => {},
		EffectAuthority::TerminalOperation { operation_id, expected_stage_epoch } => {
			let operation = state
				.superseded_closeout_operations
				.get(operation_id)
				.ok_or_else(|| eyre::eyre!("Lane effect terminal operation does not exist."))?;
			if operation.edge().predecessor_lane_id() != effect.lane_id()
				|| operation.stage_epoch() != *expected_stage_epoch
			{
				eyre::bail!("Lane effect terminal-operation prerequisites drifted.");
			}
		},
		_ => eyre::bail!("Lane effect prerequisites drifted before journal mutation."),
	}
	Ok(())
}
