use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{CanonicalPatchSet, LaneEffect, LaneEffectKind, LaneId};
use crate::prelude::{Result, eyre};

pub const REPAIR_HANDOFF_SCHEMA: &str = "decodex/repair-handoff-authority/1";
pub const SUPERSESSION_EDGE_SCHEMA: &str = "decodex/supersession-edge/1";
pub const SUPERSEDED_CLOSEOUT_SCHEMA: &str = "decodex/superseded-closeout-operation/1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepairHandoffState {
	Active,
	Replaced,
	Cancelled,
	Accepted,
	RejectedStale,
}
impl RepairHandoffState {
	pub(crate) fn from_str(value: &str) -> Option<Self> {
		match value {
			"active" => Some(Self::Active),
			"replaced" => Some(Self::Replaced),
			"cancelled" => Some(Self::Cancelled),
			"accepted" => Some(Self::Accepted),
			"rejected_stale" => Some(Self::RejectedStale),
			_ => None,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct RepairHandoffAuthority {
	schema: String,
	handoff_id: String,
	repository_key: String,
	predecessor_lane_id: LaneId,
	predecessor_issue_identifier: String,
	predecessor_pr_url: String,
	predecessor_head_oid: String,
	predecessor_epoch: u64,
	target_base_ref: String,
	target_base_oid: String,
	merge_base_oid: String,
	ordered_commit_oids: Vec<String>,
	predecessor_patch_set_digest: String,
	predecessor_patch_unit_digests: BTreeSet<String>,
	successor_lane_id: LaneId,
	successor_issue_identifier: String,
	accepted_findings_fingerprint: String,
	source_review_checkpoint_id: String,
	actor: String,
	event_id: String,
}
impl RepairHandoffAuthority {
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		handoff_id: &str,
		repository_key: &str,
		predecessor_lane_id: LaneId,
		predecessor_issue_identifier: &str,
		predecessor_pr_url: &str,
		predecessor_head_oid: &str,
		predecessor_epoch: u64,
		target_base_ref: &str,
		target_base_oid: &str,
		predecessor_patch_set: &CanonicalPatchSet,
		successor_lane_id: LaneId,
		successor_issue_identifier: &str,
		accepted_findings_fingerprint: &str,
		source_review_checkpoint_id: &str,
		actor: &str,
		event_id: &str,
	) -> Result<Self> {
		if predecessor_head_oid != predecessor_patch_set.head_oid_hex() {
			eyre::bail!("Repair handoff predecessor head does not match its canonical PatchSet.");
		}
		let handoff = Self {
			schema: String::from(REPAIR_HANDOFF_SCHEMA),
			handoff_id: handoff_id.to_owned(),
			repository_key: repository_key.to_owned(),
			predecessor_lane_id,
			predecessor_issue_identifier: predecessor_issue_identifier.to_owned(),
			predecessor_pr_url: predecessor_pr_url.to_owned(),
			predecessor_head_oid: predecessor_head_oid.to_owned(),
			predecessor_epoch,
			target_base_ref: target_base_ref.to_owned(),
			target_base_oid: target_base_oid.to_owned(),
			merge_base_oid: predecessor_patch_set.merge_base_oid_hex(),
			ordered_commit_oids: predecessor_patch_set.ordered_commit_oids_hex(),
			predecessor_patch_set_digest: predecessor_patch_set.digest.clone(),
			predecessor_patch_unit_digests: predecessor_patch_set.patch_unit_digests(),
			successor_lane_id,
			successor_issue_identifier: successor_issue_identifier.to_owned(),
			accepted_findings_fingerprint: accepted_findings_fingerprint.to_owned(),
			source_review_checkpoint_id: source_review_checkpoint_id.to_owned(),
			actor: actor.to_owned(),
			event_id: event_id.to_owned(),
		};
		handoff.validate()?;
		Ok(handoff)
	}

	#[cfg(test)]
	#[allow(clippy::too_many_arguments)]
	pub(crate) fn new_for_test(
		handoff_id: &str,
		repository_key: &str,
		predecessor_lane_id: LaneId,
		predecessor_issue_identifier: &str,
		predecessor_pr_url: &str,
		predecessor_head_oid: &str,
		predecessor_epoch: u64,
		predecessor_patch_set_digest: &str,
		predecessor_patch_unit_digests: BTreeSet<String>,
		successor_lane_id: LaneId,
		successor_issue_identifier: &str,
		accepted_findings_fingerprint: &str,
		source_review_checkpoint_id: &str,
		actor: &str,
		event_id: &str,
	) -> Result<Self> {
		let handoff = Self {
			schema: String::from(REPAIR_HANDOFF_SCHEMA),
			handoff_id: handoff_id.to_owned(),
			repository_key: repository_key.to_owned(),
			predecessor_lane_id,
			predecessor_issue_identifier: predecessor_issue_identifier.to_owned(),
			predecessor_pr_url: predecessor_pr_url.to_owned(),
			predecessor_head_oid: predecessor_head_oid.to_owned(),
			predecessor_epoch,
			target_base_ref: String::from("refs/heads/main"),
			target_base_oid: String::from("fixture-base"),
			merge_base_oid: String::from("fixture-merge-base"),
			ordered_commit_oids: vec![predecessor_head_oid.to_owned()],
			predecessor_patch_set_digest: predecessor_patch_set_digest.to_owned(),
			predecessor_patch_unit_digests,
			successor_lane_id,
			successor_issue_identifier: successor_issue_identifier.to_owned(),
			accepted_findings_fingerprint: accepted_findings_fingerprint.to_owned(),
			source_review_checkpoint_id: source_review_checkpoint_id.to_owned(),
			actor: actor.to_owned(),
			event_id: event_id.to_owned(),
		};
		handoff.validate()?;
		Ok(handoff)
	}

	pub fn handoff_id(&self) -> &str {
		&self.handoff_id
	}

	pub fn predecessor_lane_id(&self) -> &LaneId {
		&self.predecessor_lane_id
	}

	pub fn successor_lane_id(&self) -> &LaneId {
		&self.successor_lane_id
	}

	pub const fn predecessor_epoch(&self) -> u64 {
		self.predecessor_epoch
	}

	pub fn validate(&self) -> Result<()> {
		if self.schema != REPAIR_HANDOFF_SCHEMA
			|| self.predecessor_lane_id.project_key() != self.successor_lane_id.project_key()
			|| self.predecessor_lane_id == self.successor_lane_id
			|| self.predecessor_patch_unit_digests.is_empty()
			|| self.ordered_commit_oids.is_empty()
			|| !pr_matches_repository(&self.predecessor_pr_url, &self.repository_key)
		{
			eyre::bail!("Repair handoff authority is invalid.");
		}
		for value in [
			self.handoff_id.as_str(),
			self.repository_key.as_str(),
			self.predecessor_issue_identifier.as_str(),
			self.predecessor_pr_url.as_str(),
			self.predecessor_head_oid.as_str(),
			self.target_base_ref.as_str(),
			self.target_base_oid.as_str(),
			self.merge_base_oid.as_str(),
			self.predecessor_patch_set_digest.as_str(),
			self.successor_issue_identifier.as_str(),
			self.accepted_findings_fingerprint.as_str(),
			self.source_review_checkpoint_id.as_str(),
			self.actor.as_str(),
			self.event_id.as_str(),
		] {
			if value.trim().is_empty() {
				eyre::bail!("Repair handoff authority contains an empty identity.");
			}
		}
		Ok(())
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PatchDisposition {
	LandedInSuccessor {
		predecessor_patch_unit_digest: String,
		reachability_evidence: String,
	},
	CoveredBySuccessor {
		predecessor_patch_unit_digest: String,
		successor_patch_unit_digest: String,
		review_evidence: String,
	},
	AcceptedNotRequired {
		predecessor_patch_unit_digest: String,
		accountable_operator: String,
		independent_reviewer: String,
		reason: String,
	},
}
impl PatchDisposition {
	fn predecessor_digest(&self) -> &str {
		match self {
			Self::LandedInSuccessor { predecessor_patch_unit_digest, .. }
			| Self::CoveredBySuccessor { predecessor_patch_unit_digest, .. }
			| Self::AcceptedNotRequired { predecessor_patch_unit_digest, .. } => {
				predecessor_patch_unit_digest
			},
		}
	}

	fn valid(&self) -> bool {
		match self {
			Self::LandedInSuccessor { predecessor_patch_unit_digest, reachability_evidence } => {
				![predecessor_patch_unit_digest, reachability_evidence]
					.iter()
					.any(|value| value.trim().is_empty())
			},
			Self::CoveredBySuccessor {
				predecessor_patch_unit_digest,
				successor_patch_unit_digest,
				review_evidence,
			} => ![predecessor_patch_unit_digest, successor_patch_unit_digest, review_evidence]
				.iter()
				.any(|value| value.trim().is_empty()),
			Self::AcceptedNotRequired {
				predecessor_patch_unit_digest,
				accountable_operator,
				independent_reviewer,
				reason,
			} => {
				accountable_operator != independent_reviewer
					&& ![
						predecessor_patch_unit_digest,
						accountable_operator,
						independent_reviewer,
						reason,
					]
					.iter()
					.any(|value| value.trim().is_empty())
			},
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupersessionAcceptance {
	pub handoff_id: String,
	pub repository_key: String,
	pub successor_lane_id: LaneId,
	pub successor_pr_url: String,
	pub successor_head_oid: String,
	pub successor_merge_oid: String,
	pub default_branch_reachability: String,
	pub landed_successor: bool,
	pub predecessor_operation_active: bool,
	pub dispositions: Vec<PatchDisposition>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SupersessionEdge {
	schema: String,
	edge_id: String,
	handoff_id: String,
	predecessor_lane_id: LaneId,
	successor_lane_id: LaneId,
	predecessor_epoch: u64,
	successor_merge_oid: String,
}
impl SupersessionEdge {
	pub fn edge_id(&self) -> &str {
		&self.edge_id
	}

	pub fn handoff_id(&self) -> &str {
		&self.handoff_id
	}

	pub fn predecessor_lane_id(&self) -> &LaneId {
		&self.predecessor_lane_id
	}

	pub const fn predecessor_epoch(&self) -> u64 {
		self.predecessor_epoch
	}

	pub fn validate(&self) -> Result<()> {
		if self.schema != SUPERSESSION_EDGE_SCHEMA
			|| self.edge_id.trim().is_empty()
			|| self.handoff_id.trim().is_empty()
			|| self.predecessor_lane_id == self.successor_lane_id
			|| self.successor_merge_oid.trim().is_empty()
		{
			eyre::bail!("Supersession edge is invalid.");
		}
		Ok(())
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SupersededCloseoutStage {
	AcceptanceAttested,
	TerminalAuthorityCommitted,
	PredecessorPrReconciled,
	ResourcesReconciled,
	Terminal,
}
impl SupersededCloseoutStage {
	pub(crate) const fn as_str(self) -> &'static str {
		match self {
			Self::AcceptanceAttested => "acceptance_attested",
			Self::TerminalAuthorityCommitted => "terminal_authority_committed",
			Self::PredecessorPrReconciled => "predecessor_pr_reconciled",
			Self::ResourcesReconciled => "resources_reconciled",
			Self::Terminal => "terminal",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct SupersededCloseoutOperation {
	schema: String,
	operation_id: String,
	edge: SupersessionEdge,
	predecessor_pr_version_digest: String,
	successor_reachability_digest: String,
	resource_plan_digest: String,
	effect_plan: Vec<CloseoutEffectPlanItem>,
	stage: SupersededCloseoutStage,
	stage_epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct CloseoutEffectPlanItem {
	kind: LaneEffectKind,
	request_digest: String,
	desired_state_digest: String,
	facts_fingerprint: String,
}
impl CloseoutEffectPlanItem {
	pub fn new(
		kind: LaneEffectKind,
		request_digest: &str,
		desired_state_digest: &str,
		facts_fingerprint: &str,
	) -> Result<Self> {
		if [request_digest, desired_state_digest, facts_fingerprint]
			.iter()
			.any(|value| value.trim().is_empty())
		{
			eyre::bail!("Closeout effect plan digest cannot be empty.");
		}
		Ok(Self {
			kind,
			request_digest: request_digest.to_owned(),
			desired_state_digest: desired_state_digest.to_owned(),
			facts_fingerprint: facts_fingerprint.to_owned(),
		})
	}
}
impl SupersededCloseoutOperation {
	pub fn attest(
		edge: SupersessionEdge,
		predecessor_pr_version_digest: &str,
		successor_reachability_digest: &str,
		effect_plan: Vec<CloseoutEffectPlanItem>,
	) -> Result<Self> {
		for value in [predecessor_pr_version_digest, successor_reachability_digest] {
			if value.trim().is_empty() {
				eyre::bail!("Superseded closeout prerequisite digest cannot be empty.");
			}
		}
		edge.validate()?;
		validate_closeout_effect_plan(&effect_plan)?;
		let resource_plan_digest = sha256_json(&effect_plan)?;
		let digest = Sha256::digest(
			[
				b"superseded-closeout/1".as_slice(),
				edge.edge_id.as_bytes(),
				&edge.predecessor_epoch.to_be_bytes(),
			]
			.concat(),
		);
		Ok(Self {
			schema: String::from(SUPERSEDED_CLOSEOUT_SCHEMA),
			operation_id: format!(
				"sha256:{}",
				digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()
			),
			edge,
			predecessor_pr_version_digest: predecessor_pr_version_digest.to_owned(),
			successor_reachability_digest: successor_reachability_digest.to_owned(),
			resource_plan_digest,
			effect_plan,
			stage: SupersededCloseoutStage::AcceptanceAttested,
			stage_epoch: 0,
		})
	}

	pub fn operation_id(&self) -> &str {
		&self.operation_id
	}

	pub fn edge(&self) -> &SupersessionEdge {
		&self.edge
	}

	pub const fn stage(&self) -> SupersededCloseoutStage {
		self.stage
	}

	pub const fn stage_epoch(&self) -> u64 {
		self.stage_epoch
	}

	pub fn has_same_plan(&self, other: &Self) -> bool {
		self.operation_id == other.operation_id
			&& self.edge == other.edge
			&& self.predecessor_pr_version_digest == other.predecessor_pr_version_digest
			&& self.successor_reachability_digest == other.successor_reachability_digest
			&& self.resource_plan_digest == other.resource_plan_digest
			&& self.effect_plan == other.effect_plan
	}

	pub fn planned_effects(&self, binding_fingerprint: &str) -> Result<Vec<LaneEffect>> {
		self.effect_plan
			.iter()
			.enumerate()
			.map(|(ordinal, item)| {
				let ordinal = u32::try_from(ordinal)?;
				let expected_stage_epoch =
					if item.kind == LaneEffectKind::GithubPrClose { 1 } else { 2 };
				LaneEffect::plan_for_terminal_operation(
					&format!("{}:{ordinal}", self.operation_id),
					&self.operation_id,
					ordinal,
					self.edge.predecessor_lane_id.clone(),
					binding_fingerprint,
					expected_stage_epoch,
					item.kind,
					item.kind.required_class(),
					&format!("{}:{ordinal}", self.operation_id),
					&item.request_digest,
					&item.desired_state_digest,
					&item.facts_fingerprint,
				)
			})
			.collect()
	}

	pub fn validate(&self) -> Result<()> {
		if self.schema != SUPERSEDED_CLOSEOUT_SCHEMA
			|| self.operation_id.trim().is_empty()
			|| self.predecessor_pr_version_digest.trim().is_empty()
			|| self.successor_reachability_digest.trim().is_empty()
			|| self.resource_plan_digest.trim().is_empty()
			|| sha256_json(&self.effect_plan).ok().as_deref()
				!= Some(self.resource_plan_digest.as_str())
		{
			eyre::bail!("Superseded closeout operation is invalid.");
		}
		self.edge.validate()
	}
}

fn validate_closeout_effect_plan(plan: &[CloseoutEffectPlanItem]) -> Result<()> {
	if plan.is_empty() || plan[0].kind != LaneEffectKind::GithubPrClose {
		eyre::bail!("Closeout effect plan must begin with the predecessor PR close effect.");
	}
	let mut previous_rank = 0;
	for (index, item) in plan.iter().enumerate() {
		let rank = match item.kind {
			LaneEffectKind::GithubPrClose if index == 0 => 0,
			LaneEffectKind::ControlResourceRetire => 1,
			LaneEffectKind::WorktreeRemove => 2,
			LaneEffectKind::RemoteRefDelete => 3,
			LaneEffectKind::LocalRefDelete => 4,
			_ => eyre::bail!("Closeout effect plan contains an unsupported effect kind."),
		};
		if rank < previous_rank {
			eyre::bail!("Closeout effect plan is outside registry order.");
		}
		previous_rank = rank;
	}
	Ok(())
}

fn sha256_json<T: Serialize>(value: &T) -> Result<String> {
	let digest = Sha256::digest(serde_json::to_vec(value)?);
	Ok(format!("sha256:{}", digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupersededCloseoutCommand {
	CommitTerminalAuthority,
	ReconcilePredecessorPr,
	ReconcileResources,
	Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupersededCloseoutRejection {
	StageEpochMismatch,
	InvalidStage,
}

pub fn transition_superseded_closeout(
	current: &SupersededCloseoutOperation,
	expected_stage_epoch: u64,
	command: SupersededCloseoutCommand,
) -> std::result::Result<SupersededCloseoutOperation, SupersededCloseoutRejection> {
	if current.stage_epoch != expected_stage_epoch {
		return Err(SupersededCloseoutRejection::StageEpochMismatch);
	}
	let expected = match command {
		SupersededCloseoutCommand::CommitTerminalAuthority => (
			SupersededCloseoutStage::AcceptanceAttested,
			SupersededCloseoutStage::TerminalAuthorityCommitted,
		),
		SupersededCloseoutCommand::ReconcilePredecessorPr => (
			SupersededCloseoutStage::TerminalAuthorityCommitted,
			SupersededCloseoutStage::PredecessorPrReconciled,
		),
		SupersededCloseoutCommand::ReconcileResources => (
			SupersededCloseoutStage::PredecessorPrReconciled,
			SupersededCloseoutStage::ResourcesReconciled,
		),
		SupersededCloseoutCommand::Complete => {
			(SupersededCloseoutStage::ResourcesReconciled, SupersededCloseoutStage::Terminal)
		},
	};
	if current.stage == expected.1 {
		return Ok(current.clone());
	}
	if current.stage != expected.0 {
		return Err(SupersededCloseoutRejection::InvalidStage);
	}
	let mut next = current.clone();
	next.stage = expected.1;
	next.stage_epoch += 1;
	Ok(next)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupersessionRejection {
	InvalidHandoff,
	StalePredecessorEpoch,
	WrongHandoff,
	WrongSuccessor,
	RepositoryMismatch,
	SuccessorNotLanded,
	SuccessorNotReachable,
	PredecessorOperationActive,
	IncompletePatchDisposition,
	ExistingEdge,
}

pub fn accept_supersession(
	handoff: &RepairHandoffAuthority,
	acceptance: &SupersessionAcceptance,
	current_predecessor_epoch: u64,
	existing_edge: Option<&SupersessionEdge>,
) -> std::result::Result<SupersessionEdge, SupersessionRejection> {
	if handoff.validate().is_err() {
		return Err(SupersessionRejection::InvalidHandoff);
	}
	if existing_edge.is_some() {
		return Err(SupersessionRejection::ExistingEdge);
	}
	if current_predecessor_epoch != handoff.predecessor_epoch {
		return Err(SupersessionRejection::StalePredecessorEpoch);
	}
	if acceptance.handoff_id != handoff.handoff_id {
		return Err(SupersessionRejection::WrongHandoff);
	}
	if acceptance.successor_lane_id != handoff.successor_lane_id {
		return Err(SupersessionRejection::WrongSuccessor);
	}
	if acceptance.repository_key != handoff.repository_key {
		return Err(SupersessionRejection::RepositoryMismatch);
	}
	if !pr_matches_repository(&acceptance.successor_pr_url, &acceptance.repository_key) {
		return Err(SupersessionRejection::RepositoryMismatch);
	}
	if !acceptance.landed_successor || acceptance.successor_merge_oid.trim().is_empty() {
		return Err(SupersessionRejection::SuccessorNotLanded);
	}
	if acceptance.default_branch_reachability.trim().is_empty() {
		return Err(SupersessionRejection::SuccessorNotReachable);
	}
	if acceptance.predecessor_operation_active {
		return Err(SupersessionRejection::PredecessorOperationActive);
	}
	let dispositions = acceptance
		.dispositions
		.iter()
		.map(|disposition| disposition.predecessor_digest().to_owned())
		.collect::<BTreeSet<_>>();
	if acceptance.dispositions.iter().any(|disposition| !disposition.valid())
		|| dispositions.len() != acceptance.dispositions.len()
		|| dispositions != handoff.predecessor_patch_unit_digests
	{
		return Err(SupersessionRejection::IncompletePatchDisposition);
	}
	if acceptance.successor_pr_url.trim().is_empty()
		|| acceptance.successor_head_oid.trim().is_empty()
	{
		return Err(SupersessionRejection::SuccessorNotLanded);
	}

	let edge_material = serde_json::json!({
		"handoff_id": handoff.handoff_id,
		"predecessor_epoch": handoff.predecessor_epoch,
		"successor_merge_oid": acceptance.successor_merge_oid,
	});
	let digest = Sha256::digest(serde_json::to_vec(&edge_material).expect("serializable edge"));
	let edge_id =
		format!("sha256:{}", digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>());
	Ok(SupersessionEdge {
		schema: String::from(SUPERSESSION_EDGE_SCHEMA),
		edge_id,
		handoff_id: handoff.handoff_id.clone(),
		predecessor_lane_id: handoff.predecessor_lane_id.clone(),
		successor_lane_id: handoff.successor_lane_id.clone(),
		predecessor_epoch: handoff.predecessor_epoch,
		successor_merge_oid: acceptance.successor_merge_oid.clone(),
	})
}

fn pr_matches_repository(pr_url: &str, repository_key: &str) -> bool {
	let Some(repository) = repository_key.strip_prefix("github:") else {
		return false;
	};
	let prefix = format!("https://github.com/{repository}/pull/");
	pr_url.strip_prefix(&prefix).is_some_and(|number| {
		!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde::Deserialize;

	fn handoff() -> RepairHandoffAuthority {
		RepairHandoffAuthority::new_for_test(
			"handoff-1",
			"github:helixbox/pubfi-mono",
			LaneId::new("pubfi", "predecessor").expect("lane"),
			"PUB-1704",
			"https://github.com/helixbox/pubfi-mono/pull/826",
			"predecessor-head",
			7,
			"patch-set",
			BTreeSet::from([String::from("patch-a"), String::from("patch-b")]),
			LaneId::new("pubfi", "successor").expect("lane"),
			"PUB-1705",
			"findings",
			"review-checkpoint",
			"review-agent",
			"event-1",
		)
		.expect("handoff")
	}

	fn acceptance() -> SupersessionAcceptance {
		SupersessionAcceptance {
			handoff_id: String::from("handoff-1"),
			repository_key: String::from("github:helixbox/pubfi-mono"),
			successor_lane_id: LaneId::new("pubfi", "successor").expect("lane"),
			successor_pr_url: String::from("https://github.com/helixbox/pubfi-mono/pull/827"),
			successor_head_oid: String::from("successor-head"),
			successor_merge_oid: String::from("successor-merge"),
			default_branch_reachability: String::from("reachable-at-version"),
			landed_successor: true,
			predecessor_operation_active: false,
			dispositions: vec![
				PatchDisposition::LandedInSuccessor {
					predecessor_patch_unit_digest: String::from("patch-a"),
					reachability_evidence: String::from("merge-reachable"),
				},
				PatchDisposition::CoveredBySuccessor {
					predecessor_patch_unit_digest: String::from("patch-b"),
					successor_patch_unit_digest: String::from("successor-patch-b"),
					review_evidence: String::from("review-accepted"),
				},
			],
		}
	}

	fn closeout_plan() -> Vec<CloseoutEffectPlanItem> {
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
		]
	}

	#[test]
	fn exact_handoff_and_complete_disposition_create_terminal_edge() {
		let edge = accept_supersession(&handoff(), &acceptance(), 7, None).expect("edge");
		assert!(edge.edge_id().starts_with("sha256:"));
	}

	#[test]
	fn generic_relation_or_unrelated_merged_pr_cannot_create_authority() {
		let mut unrelated = acceptance();
		unrelated.successor_lane_id = LaneId::new("pubfi", "unrelated").expect("lane");
		assert_eq!(
			accept_supersession(&handoff(), &unrelated, 7, None),
			Err(SupersessionRejection::WrongSuccessor)
		);
		let mut foreign_pr = acceptance();
		foreign_pr.successor_pr_url = String::from("https://github.com/helixbox/other/pull/827");
		assert_eq!(
			accept_supersession(&handoff(), &foreign_pr, 7, None),
			Err(SupersessionRejection::RepositoryMismatch)
		);
	}

	#[test]
	fn stale_epoch_active_operation_and_incomplete_patch_set_fail_closed() {
		assert_eq!(
			accept_supersession(&handoff(), &acceptance(), 8, None),
			Err(SupersessionRejection::StalePredecessorEpoch)
		);
		let mut active = acceptance();
		active.predecessor_operation_active = true;
		assert_eq!(
			accept_supersession(&handoff(), &active, 7, None),
			Err(SupersessionRejection::PredecessorOperationActive)
		);
		let mut incomplete = acceptance();
		incomplete.dispositions.pop();
		assert_eq!(
			accept_supersession(&handoff(), &incomplete, 7, None),
			Err(SupersessionRejection::IncompletePatchDisposition)
		);
	}

	#[test]
	fn existing_terminal_edge_prevents_second_winner() {
		let edge = accept_supersession(&handoff(), &acceptance(), 7, None).expect("edge");
		assert_eq!(
			accept_supersession(&handoff(), &acceptance(), 7, Some(&edge)),
			Err(SupersessionRejection::ExistingEdge)
		);
	}

	#[test]
	fn closeout_operation_id_is_deterministic_and_plan_collision_is_detected() {
		let edge = accept_supersession(&handoff(), &acceptance(), 7, None).expect("edge");
		let first = SupersededCloseoutOperation::attest(
			edge.clone(),
			"pr-version",
			"reachability",
			closeout_plan(),
		)
		.expect("operation");
		let replay = SupersededCloseoutOperation::attest(
			edge.clone(),
			"pr-version",
			"reachability",
			closeout_plan(),
		)
		.expect("replay");
		let drift = SupersededCloseoutOperation::attest(
			edge,
			"changed-pr-version",
			"reachability",
			vec![
				CloseoutEffectPlanItem::new(
					LaneEffectKind::GithubPrClose,
					"changed-request",
					"pr-closed",
					"pr-facts",
				)
				.expect("changed plan"),
			],
		)
		.expect("drift plan");
		assert_eq!(first.operation_id(), replay.operation_id());
		assert!(first.has_same_plan(&replay));
		assert_eq!(first.operation_id(), drift.operation_id());
		assert!(!first.has_same_plan(&drift));
	}

	#[test]
	fn closeout_stages_are_ordered_and_epoch_fenced() {
		let edge = accept_supersession(&handoff(), &acceptance(), 7, None).expect("edge");
		let operation = SupersededCloseoutOperation::attest(
			edge,
			"pr-version",
			"reachability",
			closeout_plan(),
		)
		.expect("operation");
		assert_eq!(
			transition_superseded_closeout(
				&operation,
				0,
				SupersededCloseoutCommand::ReconcilePredecessorPr,
			),
			Err(SupersededCloseoutRejection::InvalidStage)
		);
		let committed = transition_superseded_closeout(
			&operation,
			0,
			SupersededCloseoutCommand::CommitTerminalAuthority,
		)
		.expect("commit stage");
		assert_eq!(committed.stage(), SupersededCloseoutStage::TerminalAuthorityCommitted);
		assert_eq!(
			transition_superseded_closeout(
				&committed,
				0,
				SupersededCloseoutCommand::ReconcilePredecessorPr,
			),
			Err(SupersededCloseoutRejection::StageEpochMismatch)
		);
	}

	#[test]
	fn pub_1704_to_pub_1705_fixture_uses_typed_lineage_not_projection_reconstruction() {
		let fixture = serde_json::from_str::<IncidentFixture>(include_str!(
			"../recovery/tests/fixtures/lane_authority_v2/pub_1704_superseded.json"
		))
		.expect("incident fixture");
		assert_eq!(fixture.schema, "decodex/supersession-incident-fixture/1");
		assert_eq!(fixture.predecessor.pr_state, "OPEN");
		assert_eq!(fixture.successor.pr_state, "MERGED");
		assert_eq!(fixture.predecessor.base_ref, fixture.successor.base_ref);
		assert!(fixture.observed_at.starts_with("2026-"));
		assert!(fixture.predecessor.branch.contains("pub-1704"));
		assert!(fixture.successor.branch.contains("pub-1705"));

		let predecessor_lane = LaneId::new("pubfi", &fixture.predecessor.issue_id).expect("lane");
		let successor_lane = LaneId::new("pubfi", &fixture.successor.issue_id).expect("lane");
		let handoff = RepairHandoffAuthority::new_for_test(
			"pub-1704-to-pub-1705",
			&fixture.repository_key,
			predecessor_lane,
			&fixture.predecessor.issue_id,
			&fixture.predecessor.pr_url,
			&fixture.predecessor.head_oid,
			7,
			"fixture-patch-set",
			BTreeSet::from([String::from("fixture-patch-unit")]),
			successor_lane.clone(),
			&fixture.successor.issue_id,
			"accepted-findings",
			"review-checkpoint",
			"operator",
			"event",
		)
		.expect("handoff");
		let edge = accept_supersession(
			&handoff,
			&SupersessionAcceptance {
				handoff_id: String::from("pub-1704-to-pub-1705"),
				repository_key: fixture.repository_key,
				successor_lane_id: successor_lane,
				successor_pr_url: fixture.successor.pr_url,
				successor_head_oid: fixture.successor.head_oid,
				successor_merge_oid: fixture.successor.merge_oid,
				default_branch_reachability: String::from("fixture-default-branch-reachability"),
				landed_successor: true,
				predecessor_operation_active: false,
				dispositions: vec![PatchDisposition::LandedInSuccessor {
					predecessor_patch_unit_digest: String::from("fixture-patch-unit"),
					reachability_evidence: String::from("fixture-default-branch-reachability"),
				}],
			},
			7,
			None,
		)
		.expect("typed edge");
		assert!(edge.edge_id().starts_with("sha256:"));
	}

	#[test]
	#[ignore = "requires DECODEX_PUBFI_MONO_REPO with the frozen public incident objects"]
	fn pub_1704_predecessor_replays_from_real_git_objects() {
		let fixture = serde_json::from_str::<IncidentFixture>(include_str!(
			"../recovery/tests/fixtures/lane_authority_v2/pub_1704_superseded.json"
		))
		.expect("incident fixture");
		let repository = std::env::var_os("DECODEX_PUBFI_MONO_REPO")
			.map(std::path::PathBuf::from)
			.expect("DECODEX_PUBFI_MONO_REPO");
		let patch_set = super::super::build_canonical_patch_set(
			&repository,
			&fixture.predecessor.base_oid,
			&fixture.predecessor.head_oid,
		)
		.expect("real canonical PatchSet");
		assert_eq!(patch_set.schema, fixture.patch_set.schema);
		assert_eq!(patch_set.digest, fixture.patch_set.digest);
		assert_eq!(patch_set.merge_base_oid_hex(), fixture.patch_set.merge_base_oid);
		assert_eq!(patch_set.ordered_commit_oids_hex(), fixture.patch_set.ordered_commit_oids);
		assert_eq!(patch_set.patch_unit_digests(), fixture.patch_set.patch_unit_digests);
		let predecessor_lane = LaneId::new("pubfi", &fixture.predecessor.issue_id).expect("lane");
		let successor_lane = LaneId::new("pubfi", &fixture.successor.issue_id).expect("lane");
		let handoff = RepairHandoffAuthority::new(
			"pub-1704-to-pub-1705-real",
			&fixture.repository_key,
			predecessor_lane,
			&fixture.predecessor.issue_id,
			&fixture.predecessor.pr_url,
			&fixture.predecessor.head_oid,
			7,
			&format!("refs/heads/{}", fixture.predecessor.base_ref),
			&fixture.predecessor.base_oid,
			&patch_set,
			successor_lane.clone(),
			&fixture.successor.issue_id,
			"accepted-findings",
			"review-checkpoint",
			"operator",
			"event",
		)
		.expect("real handoff");
		let dispositions = fixture
			.patch_set
			.patch_unit_digests
			.iter()
			.map(|digest| PatchDisposition::LandedInSuccessor {
				predecessor_patch_unit_digest: digest.clone(),
				reachability_evidence: String::from("fixture-default-branch-reachability"),
			})
			.collect();
		accept_supersession(
			&handoff,
			&SupersessionAcceptance {
				handoff_id: String::from("pub-1704-to-pub-1705-real"),
				repository_key: fixture.repository_key,
				successor_lane_id: successor_lane,
				successor_pr_url: fixture.successor.pr_url,
				successor_head_oid: fixture.successor.head_oid,
				successor_merge_oid: fixture.successor.merge_oid,
				default_branch_reachability: String::from("fixture-default-branch-reachability"),
				landed_successor: true,
				predecessor_operation_active: false,
				dispositions,
			},
			7,
			None,
		)
		.expect("real typed edge");
	}

	#[derive(Deserialize)]
	struct IncidentFixture {
		schema: String,
		observed_at: String,
		repository_key: String,
		predecessor: IncidentPredecessor,
		successor: IncidentSuccessor,
		patch_set: IncidentPatchSet,
	}

	#[derive(Deserialize)]
	struct IncidentPredecessor {
		issue_id: String,
		pr_url: String,
		pr_state: String,
		base_ref: String,
		base_oid: String,
		branch: String,
		head_oid: String,
	}

	#[derive(Deserialize)]
	struct IncidentSuccessor {
		issue_id: String,
		pr_url: String,
		pr_state: String,
		base_ref: String,
		branch: String,
		head_oid: String,
		merge_oid: String,
	}

	#[derive(Deserialize)]
	struct IncidentPatchSet {
		schema: String,
		digest: String,
		merge_base_oid: String,
		ordered_commit_oids: Vec<String>,
		patch_unit_digests: BTreeSet<String>,
	}
}
