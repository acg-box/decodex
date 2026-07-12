use crate::{
	lane_authority::{
		RepairHandoffAuthority, SupersededCloseoutCommand, SupersededCloseoutOperation,
		SupersessionAcceptance, SupersessionEdge, accept_supersession,
		transition_superseded_closeout,
	},
	prelude::{Result, eyre},
	state::StateStore,
};

impl StateStore {
	pub fn record_repair_handoff(&self, handoff: RepairHandoffAuthority) -> Result<()> {
		handoff.validate()?;
		let mut state = self.lock_without_refresh()?;
		if let Some(existing) = state.repair_handoffs.get(handoff.handoff_id()) {
			if existing == &handoff {
				return Ok(());
			}
			eyre::bail!("Immutable repair handoff cannot be replaced.");
		}
		let predecessor = state
			.lanes
			.get(handoff.predecessor_lane_id())
			.ok_or_else(|| eyre::eyre!("Repair handoff predecessor lane does not exist."))?;
		if predecessor.epoch() != handoff.predecessor_epoch() {
			eyre::bail!("Repair handoff predecessor epoch is stale.");
		}
		if !state.lanes.contains_key(handoff.successor_lane_id()) {
			eyre::bail!("Repair handoff successor lane does not exist.");
		}
		if state.repair_handoffs.values().any(|existing| {
			existing.predecessor_lane_id() == handoff.predecessor_lane_id()
				&& existing.predecessor_epoch() == handoff.predecessor_epoch()
		}) {
			eyre::bail!("An active repair handoff already owns the predecessor epoch.");
		}
		if let Some(sqlite) = self.sqlite.as_ref() {
			sqlite
				.lock()
				.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?
				.insert_repair_handoff(&handoff)?;
		}
		state.repair_handoffs.insert(handoff.handoff_id().to_owned(), handoff);
		Ok(())
	}

	pub fn attest_superseded_closeout(
		&self,
		handoff_id: &str,
		acceptance: &SupersessionAcceptance,
		predecessor_pr_version_digest: &str,
		effect_plan: Vec<crate::lane_authority::CloseoutEffectPlanItem>,
	) -> Result<SupersededCloseoutOperation> {
		let mut state = self.lock_without_refresh()?;
		let handoff = state
			.repair_handoffs
			.get(handoff_id)
			.cloned()
			.ok_or_else(|| eyre::eyre!("Repair handoff does not exist."))?;
		let predecessor = state
			.lanes
			.get(handoff.predecessor_lane_id())
			.cloned()
			.ok_or_else(|| eyre::eyre!("Repair handoff predecessor lane does not exist."))?;
		let existing = state.supersession_edges.get(handoff.predecessor_lane_id());
		let edge = accept_supersession(&handoff, acceptance, predecessor.epoch(), existing)
			.map_err(|rejection| eyre::eyre!("Supersession acceptance rejected: {rejection:?}"))?;
		let operation = SupersededCloseoutOperation::attest(
			edge,
			predecessor_pr_version_digest,
			&acceptance.default_branch_reachability,
			effect_plan,
		)?;
		if let Some(existing) = state.superseded_closeout_operations.get(operation.operation_id()) {
			if existing.has_same_plan(&operation) {
				return Ok(existing.clone());
			}
			eyre::bail!("Superseded closeout operation authority-key collision.");
		}
		if let Some(sqlite) = self.sqlite.as_ref() {
			sqlite
				.lock()
				.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?
				.insert_superseded_closeout_operation(&operation)?;
		}
		state
			.superseded_closeout_operations
			.insert(operation.operation_id().to_owned(), operation.clone());
		Ok(operation)
	}

	pub fn commit_supersession(&self, operation_id: &str) -> Result<SupersessionEdge> {
		let mut state = self.lock_without_refresh()?;
		let operation = state
			.superseded_closeout_operations
			.get(operation_id)
			.cloned()
			.ok_or_else(|| eyre::eyre!("Superseded closeout operation does not exist."))?;
		let edge = operation.edge().clone();
		let handoff = state
			.repair_handoffs
			.get(edge.handoff_id())
			.cloned()
			.ok_or_else(|| eyre::eyre!("Repair handoff does not exist."))?;
		let predecessor = state
			.lanes
			.get(handoff.predecessor_lane_id())
			.cloned()
			.ok_or_else(|| eyre::eyre!("Repair handoff predecessor lane does not exist."))?;
		let planned_effects = operation.planned_effects(predecessor.binding_fingerprint())?;
		if let Some(run_id) = predecessor.claim_run_id() {
			let lease = state
				.leases
				.get(handoff.predecessor_lane_id().tracker_issue_id())
				.ok_or_else(|| eyre::eyre!("Supersession exact conflict lease is missing."))?;
			if lease.project_id != handoff.predecessor_lane_id().project_key()
				|| lease.run_id != run_id
			{
				eyre::bail!("Supersession conflict lease ownership drifted.");
			}
		}
		let next = if let Some(sqlite) = self.sqlite.as_ref() {
			sqlite
				.lock()
				.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?
				.commit_supersession(
					&handoff,
					&edge,
					&operation,
					predecessor.binding_fingerprint(),
				)?
		} else {
			crate::lane_authority::transition(
				&predecessor,
				predecessor.epoch(),
				predecessor.binding_fingerprint(),
				crate::lane_authority::LaneCommand::BeginSupersededCleanup,
			)
			.map_err(|rejection| {
				eyre::eyre!("Supersession lane transition rejected: {rejection:?}")
			})?
		};
		state.lanes.insert(handoff.predecessor_lane_id().clone(), next);
		state.supersession_edges.insert(handoff.predecessor_lane_id().clone(), edge.clone());
		for effect in planned_effects {
			state.lane_effects.insert(effect.effect_id().to_owned(), effect);
		}
		let next_operation = transition_superseded_closeout(
			&operation,
			operation.stage_epoch(),
			SupersededCloseoutCommand::CommitTerminalAuthority,
		)
		.map_err(|rejection| eyre::eyre!("Superseded closeout stage rejected: {rejection:?}"))?;
		state.superseded_closeout_operations.insert(operation_id.to_owned(), next_operation);
		if predecessor.claim_run_id().is_some() {
			state.leases.remove(handoff.predecessor_lane_id().tracker_issue_id());
		}
		Ok(edge)
	}

	pub fn superseded_closeout_operation(
		&self,
		operation_id: &str,
	) -> Result<Option<SupersededCloseoutOperation>> {
		Ok(self.lock()?.superseded_closeout_operations.get(operation_id).cloned())
	}

	pub fn advance_superseded_closeout(
		&self,
		operation_id: &str,
		command: SupersededCloseoutCommand,
	) -> Result<SupersededCloseoutOperation> {
		let mut state = self.lock_without_refresh()?;
		let current = state
			.superseded_closeout_operations
			.get(operation_id)
			.cloned()
			.ok_or_else(|| eyre::eyre!("Superseded closeout operation does not exist."))?;
		validate_closeout_receipt_gate(&state, &current, command)?;
		let next = transition_superseded_closeout(&current, current.stage_epoch(), command)
			.map_err(|rejection| {
				eyre::eyre!("Superseded closeout stage rejected: {rejection:?}")
			})?;
		if next == current {
			return Ok(current);
		}
		let terminal_lane =
			if next.stage() == crate::lane_authority::SupersededCloseoutStage::Terminal {
				let lane_id = next.edge().predecessor_lane_id();
				let lane = state.lanes.get(lane_id).ok_or_else(|| {
					eyre::eyre!("Superseded closeout predecessor lane is missing.")
				})?;
				Some(
					crate::lane_authority::transition(
						lane,
						lane.epoch(),
						lane.binding_fingerprint(),
						crate::lane_authority::LaneCommand::CompleteTerminalCleanup,
					)
					.map_err(|rejection| {
						eyre::eyre!("Terminal cleanup Lane transition rejected: {rejection:?}")
					})?,
				)
			} else {
				None
			};
		if let Some(sqlite) = self.sqlite.as_ref() {
			sqlite
				.lock()
				.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?
				.advance_superseded_closeout(&current, &next)?;
		}
		if let Some(terminal) = terminal_lane {
			state.lanes.insert(terminal.id().clone(), terminal);
		}
		state.superseded_closeout_operations.insert(operation_id.to_owned(), next.clone());
		Ok(next)
	}

	pub fn supersession_edge(
		&self,
		predecessor_lane_id: &crate::lane_authority::LaneId,
	) -> Result<Option<SupersessionEdge>> {
		Ok(self.lock()?.supersession_edges.get(predecessor_lane_id).cloned())
	}
}

fn validate_closeout_receipt_gate(
	state: &crate::state::StateData,
	operation: &SupersededCloseoutOperation,
	command: SupersededCloseoutCommand,
) -> Result<()> {
	let mut effects = state
		.lane_effects
		.values()
		.filter(|effect| effect.operation_id() == operation.operation_id())
		.collect::<Vec<_>>();
	effects.sort_by_key(|effect| effect.ordinal());
	let required = match command {
		SupersededCloseoutCommand::CommitTerminalAuthority => return Ok(()),
		SupersededCloseoutCommand::ReconcilePredecessorPr => effects.get(..1),
		SupersededCloseoutCommand::ReconcileResources | SupersededCloseoutCommand::Complete => {
			Some(effects.as_slice())
		},
	}
	.ok_or_else(|| eyre::eyre!("Superseded closeout required effect is missing."))?;
	if required.is_empty()
		|| required.iter().any(|effect| {
			effect.state() != crate::lane_authority::EffectState::Succeeded
				|| effect.receipt().is_none()
		}) {
		eyre::bail!("Superseded closeout stage requires durable effect receipts.");
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use std::collections::BTreeSet;

	use tempfile::TempDir;

	use super::*;
	use crate::{
		lane_authority::{
			CloseoutEffectPlanItem, EffectCommand, EffectReceipt, LaneCommand, LaneEffectKind,
			LaneId, LanePhase, PatchDisposition, ProjectBinding,
		},
		state::ProjectRegistration,
	};

	#[test]
	fn terminal_authority_edge_and_exact_conflict_release_commit_together() {
		let temp = TempDir::new().expect("tempdir");
		let database = temp.path().join("state.sqlite");
		let store = StateStore::open(&database).expect("store");
		store.upsert_project(&project(&temp)).expect("project");
		store
			.try_acquire_registered_lease("pubfi", "predecessor", "run-1", "In Progress")
			.expect("predecessor claim");
		let predecessor = LaneId::new("pubfi", "predecessor").expect("lane");
		let successor = LaneId::new("pubfi", "successor").expect("lane");
		store
			.transition_lane(
				successor.clone(),
				0,
				"binding-1",
				LaneCommand::Admit { intake_authority_id: String::from("successor-authority") },
			)
			.expect("successor lane");
		let predecessor_epoch = store.lane(&predecessor).expect("read").expect("lane").epoch();
		let handoff = RepairHandoffAuthority::new(
			"handoff-1",
			"github:helixbox/pubfi-mono",
			predecessor.clone(),
			"PUB-1704",
			"https://github.com/helixbox/pubfi-mono/pull/826",
			"predecessor-head",
			predecessor_epoch,
			"patch-set",
			BTreeSet::from([String::from("patch-a")]),
			successor.clone(),
			"PUB-1705",
			"findings",
			"review-checkpoint",
			"actor",
			"event-1",
		)
		.expect("handoff");
		store.record_repair_handoff(handoff).expect("record handoff");
		let operation = store
			.attest_superseded_closeout(
				"handoff-1",
				&SupersessionAcceptance {
					handoff_id: String::from("handoff-1"),
					repository_key: String::from("github:helixbox/pubfi-mono"),
					successor_lane_id: successor,
					successor_pr_url: String::from(
						"https://github.com/helixbox/pubfi-mono/pull/827",
					),
					successor_head_oid: String::from("successor-head"),
					successor_merge_oid: String::from("successor-merge"),
					default_branch_reachability: String::from("reachable"),
					landed_successor: true,
					predecessor_operation_active: false,
					dispositions: vec![PatchDisposition::LandedInSuccessor {
						predecessor_patch_unit_digest: String::from("patch-a"),
						reachability_evidence: String::from("reachable"),
					}],
				},
				"predecessor-pr-version",
				vec![
					CloseoutEffectPlanItem::new(
						LaneEffectKind::GithubPrClose,
						"pr-request",
						"pr-closed",
						"pr-facts",
					)
					.expect("PR effect"),
					CloseoutEffectPlanItem::new(
						LaneEffectKind::WorktreeRemove,
						"worktree-request",
						"worktree-removed",
						"worktree-facts",
					)
					.expect("worktree effect"),
				],
			)
			.expect("attest operation");
		let edge =
			store.commit_supersession(operation.operation_id()).expect("commit supersession");
		assert_eq!(
			store.lane(&predecessor).expect("read").expect("lane").phase(),
			LanePhase::TerminalCleanupPending
		);
		assert!(!store.lock().expect("state").leases.contains_key("predecessor"));
		assert!(
			store
				.advance_superseded_closeout(
					operation.operation_id(),
					SupersededCloseoutCommand::ReconcilePredecessorPr,
				)
				.is_err(),
			"stage cannot advance before the PR receipt"
		);
		succeed_effect(&store, operation.operation_id(), 0, "pr-request");
		store
			.advance_superseded_closeout(
				operation.operation_id(),
				SupersededCloseoutCommand::ReconcilePredecessorPr,
			)
			.expect("advance PR reconciliation");
		succeed_effect(&store, operation.operation_id(), 1, "worktree-request");
		store
			.advance_superseded_closeout(
				operation.operation_id(),
				SupersededCloseoutCommand::ReconcileResources,
			)
			.expect("advance resources");
		store
			.advance_superseded_closeout(
				operation.operation_id(),
				SupersededCloseoutCommand::Complete,
			)
			.expect("complete closeout");
		assert_eq!(
			store.lane(&predecessor).expect("read").expect("lane").phase(),
			LanePhase::Terminal
		);
		drop(store);

		let reopened = StateStore::open(&database).expect("reopen");
		assert_eq!(reopened.supersession_edge(&predecessor).expect("edge read"), Some(edge));
		assert_eq!(
			reopened
				.superseded_closeout_operation(operation.operation_id())
				.expect("operation read")
				.expect("operation")
				.stage(),
			crate::lane_authority::SupersededCloseoutStage::Terminal
		);
		assert_eq!(
			reopened.lane(&predecessor).expect("read").expect("lane").phase(),
			LanePhase::Terminal
		);
	}

	fn succeed_effect(store: &StateStore, operation_id: &str, ordinal: u32, request: &str) {
		let effect_id = format!("{operation_id}:{ordinal}");
		let effect = store.lane_effect(&effect_id).expect("effect read").expect("effect");
		let invoking = store
			.apply_lane_effect_command(
				&effect_id,
				effect.journal_epoch(),
				EffectCommand::BeginInvocation {
					authority_epoch: effect.authority_epoch(),
					facts_fingerprint: effect.facts_fingerprint().to_owned(),
				},
			)
			.expect("invoke effect");
		store
			.apply_lane_effect_command(
				&effect_id,
				invoking.journal_epoch(),
				EffectCommand::RecordReceipt {
					receipt: EffectReceipt::new(
						&format!("receipt-{ordinal}"),
						request,
						"result",
						None,
						Some("version"),
						"2026-07-12T00:00:00Z",
						1,
					)
					.expect("receipt"),
				},
			)
			.expect("record receipt");
	}

	fn project(temp: &TempDir) -> ProjectRegistration {
		ProjectRegistration {
			service_id: String::from("pubfi"),
			config_path: temp.path().join("project.toml"),
			repo_root: temp.path().join("repo"),
			worktree_root: temp.path().join("repo/.worktrees"),
			workflow_path: temp.path().join("WORKFLOW.md"),
			tracker_api_key_env_var: String::from("LINEAR_API_KEY"),
			github_token_env_var: String::from("GITHUB_TOKEN"),
			enabled: true,
			config_fingerprint: String::from("binding-1"),
			binding: ProjectBinding::new(
				"pubfi",
				"helixbox",
				"pubfi-mono",
				"team-pubfi",
				"decodex:queued:pubfi",
				"binding-1",
			)
			.expect("binding"),
			updated_at: String::from("2026-07-12T00:00:00Z"),
			updated_at_unix: 1_783_814_400,
		}
	}
}
