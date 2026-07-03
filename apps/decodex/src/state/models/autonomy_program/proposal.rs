use crate::autonomy_proposal::{AutonomyProposal, AutonomyProposalState};

/// SQLite-backed autonomy proposal dry-run evidence retained by the local runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutonomyProposalRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) proposal: AutonomyProposal,
	pub(in crate::state) state: AutonomyProposalState,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
#[allow(dead_code)]
impl AutonomyProposalRecord {
	pub(crate) fn project_id(&self) -> &str {
		&self.project_id
	}

	pub(crate) fn proposal(&self) -> &AutonomyProposal {
		&self.proposal
	}

	pub(crate) fn proposal_id(&self) -> &str {
		self.proposal.id()
	}

	pub(crate) fn objective_id(&self) -> &str {
		self.proposal.objective_id()
	}

	pub(crate) fn objective_version(&self) -> u64 {
		self.proposal.objective_version()
	}

	pub(crate) fn state(&self) -> AutonomyProposalState {
		self.state
	}

	pub(crate) fn created_at(&self) -> &str {
		&self.created_at
	}

	pub(crate) fn created_at_unix(&self) -> i64 {
		self.created_at_unix
	}

	pub(crate) fn updated_at(&self) -> &str {
		&self.updated_at
	}

	pub(crate) fn updated_at_unix(&self) -> i64 {
		self.updated_at_unix
	}
}
