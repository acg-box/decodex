use crate::{
	autonomy_objective::{AutonomyObjectiveContract, AutonomyObjectiveState},
	autonomy_proposal::{AutonomyProposal, AutonomyProposalState},
	autonomy_signal::AutonomySignal,
	state::{AutonomyObjectiveRecord, AutonomyProposalRecord, AutonomySignalRecord},
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
