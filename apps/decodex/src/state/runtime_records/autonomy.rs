use crate::{
	autonomy_objective::{AutonomyObjectiveContract, AutonomyObjectiveState},
	autonomy_proposal::{AutonomyProposal, AutonomyProposalState},
	autonomy_signal::AutonomySignal,
	prelude::{Result, eyre},
	state::{
		AutonomyObjectiveRecord, AutonomyProposalRecord, AutonomyRuntimePolicyRecord,
		AutonomySignalRecord,
	},
};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct AutonomyObjectiveKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) objective_id: String,
	pub(in crate::state) version: u64,
}
impl AutonomyObjectiveKey {
	pub(in crate::state) fn new(project_id: &str, objective_id: &str, version: u64) -> Self {
		Self { project_id: project_id.to_owned(), objective_id: objective_id.to_owned(), version }
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct AutonomyObjectiveRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) objective: AutonomyObjectiveContract,
	pub(in crate::state) state: AutonomyObjectiveState,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl AutonomyObjectiveRuntimeRecord {
	#[allow(dead_code)]
	pub(in crate::state) fn key(&self) -> AutonomyObjectiveKey {
		AutonomyObjectiveKey::new(&self.project_id, self.objective.id(), self.objective.version())
	}

	#[allow(dead_code)]
	pub(in crate::state) fn as_public(&self) -> AutonomyObjectiveRecord {
		AutonomyObjectiveRecord {
			project_id: self.project_id.clone(),
			objective: self.objective.clone(),
			state: self.state,
			created_at: self.created_at.clone(),
			created_at_unix: self.created_at_unix,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct AutonomySignalKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) signal_id: String,
}
impl AutonomySignalKey {
	pub(in crate::state) fn new(project_id: &str, signal_id: &str) -> Self {
		Self { project_id: project_id.to_owned(), signal_id: signal_id.to_owned() }
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct AutonomySignalRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) signal: AutonomySignal,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl AutonomySignalRuntimeRecord {
	pub(in crate::state) fn key(&self) -> AutonomySignalKey {
		AutonomySignalKey::new(&self.project_id, self.signal.id())
	}

	pub(in crate::state) fn as_public(&self) -> AutonomySignalRecord {
		AutonomySignalRecord {
			project_id: self.project_id.clone(),
			signal: self.signal.clone(),
			created_at: self.created_at.clone(),
			created_at_unix: self.created_at_unix,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct AutonomyProposalKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) proposal_id: String,
}
impl AutonomyProposalKey {
	pub(in crate::state) fn new(project_id: &str, proposal_id: &str) -> Self {
		Self { project_id: project_id.to_owned(), proposal_id: proposal_id.to_owned() }
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct AutonomyProposalRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) proposal: AutonomyProposal,
	pub(in crate::state) state: AutonomyProposalState,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl AutonomyProposalRuntimeRecord {
	pub(in crate::state) fn key(&self) -> AutonomyProposalKey {
		AutonomyProposalKey::new(&self.project_id, self.proposal.id())
	}

	pub(in crate::state) fn as_public(&self) -> AutonomyProposalRecord {
		AutonomyProposalRecord {
			project_id: self.project_id.clone(),
			proposal: self.proposal.clone(),
			state: self.state,
			created_at: self.created_at.clone(),
			created_at_unix: self.created_at_unix,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::state) struct AutonomyRuntimePolicyKey {
	pub(in crate::state) project_id: String,
	pub(in crate::state) policy_id: String,
	pub(in crate::state) policy_version: String,
}
impl AutonomyRuntimePolicyKey {
	pub(in crate::state) fn new(project_id: &str, policy_id: &str, policy_version: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			policy_id: policy_id.to_owned(),
			policy_version: policy_version.to_owned(),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::state) struct AutonomyRuntimePolicyRuntimeRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) policy_id: String,
	pub(in crate::state) policy_version: String,
	pub(in crate::state) objective_id: String,
	pub(in crate::state) objective_version: u64,
	pub(in crate::state) objective_digest: String,
	pub(in crate::state) authority_ref: String,
	pub(in crate::state) accepted_by: String,
	pub(in crate::state) accepted_at: String,
	pub(in crate::state) acceptance_source: String,
	pub(in crate::state) public_non_goals: Vec<String>,
}
impl AutonomyRuntimePolicyRuntimeRecord {
	pub(in crate::state) fn key(&self) -> AutonomyRuntimePolicyKey {
		AutonomyRuntimePolicyKey::new(&self.project_id, &self.policy_id, &self.policy_version)
	}

	pub(in crate::state) fn as_public(&self) -> AutonomyRuntimePolicyRecord {
		AutonomyRuntimePolicyRecord {
			project_id: self.project_id.clone(),
			policy_id: self.policy_id.clone(),
			policy_version: self.policy_version.clone(),
			objective_id: self.objective_id.clone(),
			objective_version: self.objective_version,
			objective_digest: self.objective_digest.clone(),
			authority_ref: self.authority_ref.clone(),
			accepted_by: self.accepted_by.clone(),
			accepted_at: self.accepted_at.clone(),
			acceptance_source: self.acceptance_source.clone(),
			public_non_goals: self.public_non_goals.clone(),
		}
	}

	pub(in crate::state) fn ensure_exact_replay(&self, candidate: &Self) -> Result<()> {
		if self == candidate {
			return Ok(());
		}

		eyre::bail!(
			"Autonomy runtime policy `{}` version `{}` for project `{}` conflicts with its immutable accepted record.",
			self.policy_id,
			self.policy_version,
			self.project_id
		)
	}
}
impl From<AutonomyRuntimePolicyRecord> for AutonomyRuntimePolicyRuntimeRecord {
	fn from(record: AutonomyRuntimePolicyRecord) -> Self {
		Self {
			project_id: record.project_id,
			policy_id: record.policy_id,
			policy_version: record.policy_version,
			objective_id: record.objective_id,
			objective_version: record.objective_version,
			objective_digest: record.objective_digest,
			authority_ref: record.authority_ref,
			accepted_by: record.accepted_by,
			accepted_at: record.accepted_at,
			acceptance_source: record.acceptance_source,
			public_non_goals: record.public_non_goals,
		}
	}
}

pub(in crate::state) struct AutonomyObjectiveRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) objective_id: String,
	pub(in crate::state) version: i64,
	pub(in crate::state) state: String,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}

pub(in crate::state) struct AutonomySignalRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) signal_id: String,
	pub(in crate::state) objective_id: String,
	pub(in crate::state) objective_version: i64,
	pub(in crate::state) kind: String,
	pub(in crate::state) fingerprint: String,
	pub(in crate::state) freshness: String,
	pub(in crate::state) evidence_class: String,
	pub(in crate::state) confidence: String,
	pub(in crate::state) privacy: String,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}

pub(in crate::state) struct AutonomyProposalRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) proposal_id: String,
	pub(in crate::state) objective_id: String,
	pub(in crate::state) objective_version: i64,
	pub(in crate::state) state: String,
	pub(in crate::state) fingerprint: String,
	pub(in crate::state) source_family: String,
	pub(in crate::state) intended_surface: String,
	pub(in crate::state) payload_json: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}

pub(in crate::state) struct AutonomyRuntimePolicyRuntimeRowParts {
	pub(in crate::state) project_id: String,
	pub(in crate::state) policy_id: String,
	pub(in crate::state) policy_version: String,
	pub(in crate::state) objective_id: String,
	pub(in crate::state) objective_version: i64,
	pub(in crate::state) objective_digest: String,
	pub(in crate::state) authority_ref: String,
	pub(in crate::state) accepted_by: String,
	pub(in crate::state) accepted_at: String,
	pub(in crate::state) acceptance_source: String,
	pub(in crate::state) public_non_goals_json: String,
}
