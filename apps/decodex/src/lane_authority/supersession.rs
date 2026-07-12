use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::LaneId;
use crate::prelude::{Result, eyre};

pub const REPAIR_HANDOFF_SCHEMA: &str = "decodex/repair-handoff-authority/1";
pub const SUPERSESSION_EDGE_SCHEMA: &str = "decodex/supersession-edge/1";

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

	fn handoff() -> RepairHandoffAuthority {
		RepairHandoffAuthority::new(
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
}
