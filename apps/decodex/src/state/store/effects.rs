use std::{os::unix::ffi::OsStrExt, path::Path};

use sha2::{Digest, Sha256};

use crate::{
	github::{
		PullRequestCloseReadback, RemoteRefDeleteReadback, reconcile_pull_request_close,
		reconcile_remote_ref_delete,
	},
	lane_authority::{
		CloseoutEffectTarget, EffectAuthority, EffectCommand, EffectReceipt, EffectState,
		LaneEffect, LaneEffectKind, apply_effect_command,
	},
	orchestrator::git_ops::{LocalRefDeleteReadback, delete_local_branch_at_oid},
	prelude::{Result, eyre},
	state::StateStore,
};

impl StateStore {
	pub fn execute_control_resource_retire_effect(
		&self,
		effect_id: &str,
		observed_at: &str,
		observed_at_unix: i64,
	) -> Result<LaneEffect> {
		let (current, mut channel, retired_status) = {
			let state = self.lock_without_refresh()?;
			let current = state
				.lane_effects
				.get(effect_id)
				.cloned()
				.ok_or_else(|| eyre::eyre!("Control cleanup effect does not exist."))?;
			if current.kind() != LaneEffectKind::ControlResourceRetire {
				eyre::bail!("Effect is not a control cleanup effect.");
			}
			validate_effect_lane(&state, &current)?;
			let EffectAuthority::TerminalOperation { operation_id, .. } = current.authority()
			else {
				eyre::bail!("Control cleanup requires terminal-operation authority.");
			};
			let operation = state
				.superseded_closeout_operations
				.get(operation_id)
				.ok_or_else(|| eyre::eyre!("Control cleanup operation is missing."))?;
			let Some(CloseoutEffectTarget::ControlResource {
				project_key,
				issue_id,
				run_id,
				attempt_number,
				channel_path,
				transport,
				retired_status,
			}) = operation.planned_effect_target(current.ordinal())
			else {
				eyre::bail!("Control cleanup target does not match its effect.");
			};
			let channel = state
				.control_channels
				.get(run_id)
				.cloned()
				.ok_or_else(|| eyre::eyre!("Control cleanup ownership record is missing."))?;
			let facts = control_resource_facts_fingerprint(
				project_key,
				issue_id,
				run_id,
				*attempt_number,
				channel_path,
				transport,
			);
			if channel.project_id != *project_key
				|| channel.issue_id != *issue_id
				|| channel.run_id != *run_id
				|| channel.attempt_number != *attempt_number
				|| channel.channel_path != *channel_path
				|| channel.transport != *transport
				|| channel.status != crate::state::RUN_CONTROL_CHANNEL_STATUS_ACTIVE
				|| current.facts_fingerprint() != facts
			{
				eyre::bail!("Control cleanup ownership prerequisites drifted.");
			}
			(current, channel, retired_status.clone())
		};
		let invoking = self.apply_lane_effect_command(
			effect_id,
			current.journal_epoch(),
			EffectCommand::BeginInvocation {
				authority_epoch: current.authority_epoch(),
				facts_fingerprint: current.facts_fingerprint().to_owned(),
			},
		)?;
		if let Err(error) = remove_control_channel_path(&channel.channel_path) {
			let _ = self.apply_lane_effect_command(
				effect_id,
				invoking.journal_epoch(),
				EffectCommand::MarkOutcomeUnknown,
			);
			return Err(error);
		}
		let receipt = EffectReceipt::new(
			&format!("control-retire:{}:{}", channel.run_id, channel.attempt_number),
			invoking.request_digest(),
			invoking.facts_fingerprint(),
			Some(&channel.run_id),
			Some(invoking.facts_fingerprint()),
			observed_at,
			observed_at_unix,
		)?;
		let succeeded = apply_effect_command(
			&invoking,
			invoking.journal_epoch(),
			EffectCommand::RecordReceipt { receipt },
		)
		.map_err(|rejection| eyre::eyre!("Control cleanup receipt rejected: {rejection:?}"))?;
		channel.status = retired_status;
		channel.updated_at = observed_at.to_owned();
		channel.updated_at_unix = observed_at_unix;
		let mut state = self.lock_without_refresh()?;
		validate_effect_lane(&state, &invoking)?;
		if let Some(sqlite) = self.sqlite.as_ref() {
			sqlite
				.lock()
				.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?
				.complete_control_resource_retire_effect(
					invoking.journal_epoch(),
					&succeeded,
					&channel,
				)?;
		}
		state.control_channels.insert(channel.run_id.clone(), channel);
		state.lane_effects.insert(effect_id.to_owned(), succeeded.clone());
		Ok(succeeded)
	}

	pub fn execute_pull_request_close_effect(
		&self,
		effect_id: &str,
		github_token: &str,
		gh_command_path: Option<&Path>,
		cwd: &Path,
	) -> Result<LaneEffect> {
		let (current, repository_key, pull_request_number, expected_head_oid, expected_base_ref) = {
			let state = self.lock_without_refresh()?;
			let current = state
				.lane_effects
				.get(effect_id)
				.cloned()
				.ok_or_else(|| eyre::eyre!("Pull request close effect does not exist."))?;
			if current.kind() != LaneEffectKind::GithubPrClose {
				eyre::bail!("Effect is not a pull request close effect.");
			}
			validate_effect_lane(&state, &current)?;
			let EffectAuthority::TerminalOperation { operation_id, .. } = current.authority()
			else {
				eyre::bail!("Pull request close requires terminal-operation authority.");
			};
			let operation = state
				.superseded_closeout_operations
				.get(operation_id)
				.ok_or_else(|| eyre::eyre!("Pull request close operation is missing."))?;
			let Some(CloseoutEffectTarget::GithubPullRequest {
				repository_key,
				pull_request_number,
				expected_head_oid,
				expected_base_ref,
			}) = operation.planned_effect_target(current.ordinal())
			else {
				eyre::bail!("Pull request close target does not match its effect.");
			};
			(
				current,
				repository_key.clone(),
				*pull_request_number,
				expected_head_oid.clone(),
				expected_base_ref.clone(),
			)
		};
		let repository = repository_key
			.strip_prefix("github:")
			.ok_or_else(|| eyre::eyre!("Pull request close repository key is invalid."))?;
		let pr_url = format!("https://github.com/{repository}/pull/{pull_request_number}");
		let invoking = self.apply_lane_effect_command(
			effect_id,
			current.journal_epoch(),
			EffectCommand::BeginInvocation {
				authority_epoch: current.authority_epoch(),
				facts_fingerprint: current.facts_fingerprint().to_owned(),
			},
		)?;
		let readback = match reconcile_pull_request_close(
			cwd,
			&pr_url,
			&expected_head_oid,
			&expected_base_ref,
			invoking.request_digest(),
			github_token,
			gh_command_path,
		) {
			Ok(readback) => readback,
			Err(error) => {
				let _ = self.apply_lane_effect_command(
					effect_id,
					invoking.journal_epoch(),
					EffectCommand::MarkOutcomeUnknown,
				);
				return Err(error);
			},
		};
		match readback {
			PullRequestCloseReadback::AlreadyClosed(receipt) => self.apply_lane_effect_command(
				effect_id,
				invoking.journal_epoch(),
				EffectCommand::RecordReceipt { receipt },
			),
			PullRequestCloseReadback::ConditionalMutationUnsupported { .. }
			| PullRequestCloseReadback::PrerequisiteDrift { .. } => self.apply_lane_effect_command(
				effect_id,
				invoking.journal_epoch(),
				EffectCommand::RequireAttention,
			),
		}
	}

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

	pub fn execute_local_ref_delete_effect(
		&self,
		effect_id: &str,
		observed_at: &str,
		observed_at_unix: i64,
	) -> Result<LaneEffect> {
		let (current, repository_path, branch_name, expected_oid) = {
			let state = self.lock_without_refresh()?;
			let current = state
				.lane_effects
				.get(effect_id)
				.cloned()
				.ok_or_else(|| eyre::eyre!("Local ref cleanup effect does not exist."))?;
			if current.kind() != LaneEffectKind::LocalRefDelete {
				eyre::bail!("Effect is not a local ref cleanup effect.");
			}
			validate_effect_lane(&state, &current)?;
			let EffectAuthority::TerminalOperation { operation_id, .. } = current.authority()
			else {
				eyre::bail!("Local ref cleanup requires terminal-operation authority.");
			};
			let operation = state
				.superseded_closeout_operations
				.get(operation_id)
				.ok_or_else(|| eyre::eyre!("Local ref cleanup operation is missing."))?;
			let Some(CloseoutEffectTarget::LocalRef { repository_path, branch_name, expected_oid }) =
				operation.planned_effect_target(current.ordinal())
			else {
				eyre::bail!("Local ref cleanup target does not match its effect.");
			};
			(current, repository_path.clone(), branch_name.clone(), expected_oid.clone())
		};
		let invoking = self.apply_lane_effect_command(
			effect_id,
			current.journal_epoch(),
			EffectCommand::BeginInvocation {
				authority_epoch: current.authority_epoch(),
				facts_fingerprint: current.facts_fingerprint().to_owned(),
			},
		)?;
		let readback = match delete_local_branch_at_oid(
			&repository_path,
			&branch_name,
			&expected_oid,
			invoking.request_digest(),
			observed_at,
			observed_at_unix,
		) {
			Ok(readback) => readback,
			Err(error) => {
				let _ = self.apply_lane_effect_command(
					effect_id,
					invoking.journal_epoch(),
					EffectCommand::MarkOutcomeUnknown,
				);
				return Err(error);
			},
		};
		match readback {
			LocalRefDeleteReadback::AlreadyAbsent(receipt)
			| LocalRefDeleteReadback::Deleted(receipt) => self.apply_lane_effect_command(
				effect_id,
				invoking.journal_epoch(),
				EffectCommand::RecordReceipt { receipt },
			),
			LocalRefDeleteReadback::PrerequisiteDrift { .. } => self.apply_lane_effect_command(
				effect_id,
				invoking.journal_epoch(),
				EffectCommand::RequireAttention,
			),
		}
	}

	#[allow(clippy::too_many_arguments)]
	pub fn execute_remote_ref_delete_effect(
		&self,
		effect_id: &str,
		observed_at: &str,
		observed_at_unix: i64,
		github_token: &str,
		gh_command_path: Option<&Path>,
		cwd: &Path,
	) -> Result<LaneEffect> {
		let (current, repository_key, branch_name, expected_oid) = {
			let state = self.lock_without_refresh()?;
			let current = state
				.lane_effects
				.get(effect_id)
				.cloned()
				.ok_or_else(|| eyre::eyre!("Remote ref cleanup effect does not exist."))?;
			if current.kind() != LaneEffectKind::RemoteRefDelete {
				eyre::bail!("Effect is not a remote ref cleanup effect.");
			}
			validate_effect_lane(&state, &current)?;
			let EffectAuthority::TerminalOperation { operation_id, .. } = current.authority()
			else {
				eyre::bail!("Remote ref cleanup requires terminal-operation authority.");
			};
			let operation = state
				.superseded_closeout_operations
				.get(operation_id)
				.ok_or_else(|| eyre::eyre!("Remote ref cleanup operation is missing."))?;
			let Some(CloseoutEffectTarget::RemoteRef { repository_key, branch_name, expected_oid }) =
				operation.planned_effect_target(current.ordinal())
			else {
				eyre::bail!("Remote ref cleanup target does not match its effect.");
			};
			(current, repository_key.clone(), branch_name.clone(), expected_oid.clone())
		};
		let repository = repository_key
			.strip_prefix("github:")
			.ok_or_else(|| eyre::eyre!("Remote ref cleanup repository key is invalid."))?;
		let (owner, repository) = repository
			.split_once('/')
			.ok_or_else(|| eyre::eyre!("Remote ref cleanup repository key is invalid."))?;
		let invoking = self.apply_lane_effect_command(
			effect_id,
			current.journal_epoch(),
			EffectCommand::BeginInvocation {
				authority_epoch: current.authority_epoch(),
				facts_fingerprint: current.facts_fingerprint().to_owned(),
			},
		)?;
		let readback = match reconcile_remote_ref_delete(
			cwd,
			owner,
			repository,
			&branch_name,
			&expected_oid,
			invoking.request_digest(),
			observed_at,
			observed_at_unix,
			github_token,
			gh_command_path,
		) {
			Ok(readback) => readback,
			Err(error) => {
				let _ = self.apply_lane_effect_command(
					effect_id,
					invoking.journal_epoch(),
					EffectCommand::MarkOutcomeUnknown,
				);
				return Err(error);
			},
		};
		match readback {
			RemoteRefDeleteReadback::AlreadyAbsent(receipt) => self.apply_lane_effect_command(
				effect_id,
				invoking.journal_epoch(),
				EffectCommand::RecordReceipt { receipt },
			),
			RemoteRefDeleteReadback::ConditionalMutationUnsupported { .. }
			| RemoteRefDeleteReadback::PrerequisiteDrift { .. } => self.apply_lane_effect_command(
				effect_id,
				invoking.journal_epoch(),
				EffectCommand::RequireAttention,
			),
		}
	}
}

pub fn control_resource_facts_fingerprint(
	project_id: &str,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	channel_path: &Path,
	transport: &str,
) -> String {
	let mut digest = Sha256::new();
	for value in
		[project_id.as_bytes(), issue_id.as_bytes(), run_id.as_bytes(), transport.as_bytes()]
	{
		digest.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
		digest.update(value);
	}
	digest.update(attempt_number.to_be_bytes());
	let path = channel_path.as_os_str().as_bytes();
	digest.update(u64::try_from(path.len()).unwrap_or(u64::MAX).to_be_bytes());
	digest.update(path);
	format!(
		"sha256:{}",
		digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>()
	)
}

fn remove_control_channel_path(path: &Path) -> Result<()> {
	match std::fs::symlink_metadata(path) {
		Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path)?,
		Ok(_) => std::fs::remove_file(path)?,
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
		Err(error) => return Err(error.into()),
	}
	Ok(())
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
